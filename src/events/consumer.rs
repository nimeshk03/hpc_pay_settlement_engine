use crate::error::{AppError, Result};
use anyhow::anyhow;
use async_trait::async_trait;
use rskafka::client::partition::{PartitionClient, UnknownTopicHandling};
use rskafka::client::ClientBuilder;
use rskafka::record::RecordAndOffset;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Configuration for the Kafka consumer.
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    pub brokers: Vec<String>,
    pub topics: Vec<String>,
    pub group_id: String,
    pub auto_offset_reset: OffsetReset,
    pub fetch_min_bytes: i32,
    pub fetch_max_wait_ms: i32,
    pub max_poll_records: usize,
    pub enable_auto_commit: bool,
    pub dead_letter_topic: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum OffsetReset {
    #[default]
    Earliest,
    Latest,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            brokers: vec!["localhost:9092".to_string()],
            topics: vec!["settlement.transactions".to_string()],
            group_id: "settlement-engine".to_string(),
            auto_offset_reset: OffsetReset::default(),
            fetch_min_bytes: 1,
            fetch_max_wait_ms: 500,
            max_poll_records: 100,
            enable_auto_commit: true,
            dead_letter_topic: Some("settlement.dlq".to_string()),
        }
    }
}

/// Message received from Kafka.
#[derive(Debug, Clone)]
pub struct ConsumedMessage {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: Option<Vec<u8>>,
    pub value: Vec<u8>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ConsumedMessage {
    /// Deserializes the message value as JSON.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.value)
            .map_err(|e| AppError::Internal(anyhow!("Failed to deserialize message: {}", e)))
    }

    /// Gets the key as a string.
    pub fn key_str(&self) -> Option<String> {
        self.key.as_ref().and_then(|k| String::from_utf8(k.clone()).ok())
    }
}

/// Handler trait for processing consumed messages.
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Processes a single message. Returns Ok(()) if successful, Err if the message should be sent to DLQ.
    async fn handle(&self, message: &ConsumedMessage) -> Result<()>;

    /// Called when a message fails processing and is sent to DLQ.
    async fn on_dead_letter(&self, message: &ConsumedMessage, error: &AppError) {
        error!(
            "Message sent to DLQ: topic={}, partition={}, offset={}, error={}",
            message.topic, message.partition, message.offset, error
        );
    }
}

/// Kafka event consumer for settlement events.
pub struct EventConsumer {
    config: ConsumerConfig,
    client: Option<Arc<rskafka::client::Client>>,
    partition_clients: Arc<RwLock<BTreeMap<String, Arc<PartitionClient>>>>,
    offsets: Arc<RwLock<BTreeMap<String, AtomicI64>>>,
    running: Arc<AtomicBool>,
}

impl EventConsumer {
    /// Creates a new event consumer with the given configuration.
    pub fn new(config: ConsumerConfig) -> Self {
        Self {
            config,
            client: None,
            partition_clients: Arc::new(RwLock::new(BTreeMap::new())),
            offsets: Arc::new(RwLock::new(BTreeMap::new())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Connects to the Kafka cluster.
    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting consumer to Kafka brokers: {:?}", self.config.brokers);

        let client = ClientBuilder::new(self.config.brokers.clone())
            .build()
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to connect to Kafka: {}", e)))?;

        self.client = Some(Arc::new(client));

        // Initialize partition clients for each topic
        for topic in &self.config.topics {
            self.get_partition_client(topic).await?;
        }

        info!("Consumer successfully connected to Kafka");
        Ok(())
    }

    /// Gets or creates a partition client for the given topic.
    async fn get_partition_client(&self, topic: &str) -> Result<Arc<PartitionClient>> {
        {
            let clients = self.partition_clients.read().await;
            if let Some(client) = clients.get(topic) {
                return Ok(client.clone());
            }
        }

        let kafka_client = self.client.as_ref().ok_or_else(|| {
            AppError::Internal(anyhow!("Kafka client not connected"))
        })?;

        let partition_client = kafka_client
            .partition_client(topic.to_string(), 0, UnknownTopicHandling::Retry)
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to get partition client: {}", e)))?;

        let client = Arc::new(partition_client);

        {
            let mut clients = self.partition_clients.write().await;
            clients.insert(topic.to_string(), client.clone());
        }

        // Initialize offset for this topic
        {
            let mut offsets = self.offsets.write().await;
            if !offsets.contains_key(topic) {
                let initial_offset = match self.config.auto_offset_reset {
                    OffsetReset::Earliest => 0,
                    OffsetReset::Latest => -1, // Will be resolved on first fetch
                };
                offsets.insert(topic.to_string(), AtomicI64::new(initial_offset));
            }
        }

        Ok(client)
    }

    /// Gets the current offset for a topic.
    async fn get_offset(&self, topic: &str) -> i64 {
        let offsets = self.offsets.read().await;
        offsets
            .get(topic)
            .map(|o| o.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// Updates the offset for a topic.
    async fn update_offset(&self, topic: &str, offset: i64) {
        let offsets = self.offsets.read().await;
        if let Some(o) = offsets.get(topic) {
            o.store(offset, Ordering::SeqCst);
        }
    }

    /// Polls for messages from a specific topic.
    pub async fn poll(&self, topic: &str) -> Result<Vec<ConsumedMessage>> {
        let partition_client = self.get_partition_client(topic).await?;
        let current_offset = self.get_offset(topic).await;

        let (records, _high_watermark) = partition_client
            .fetch_records(
                current_offset,
                1..1_000_000, // min..max bytes
                self.config.fetch_max_wait_ms,
            )
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to fetch records: {}", e)))?;

        let messages: Vec<ConsumedMessage> = records
            .into_iter()
            .take(self.config.max_poll_records)
            .map(|r: RecordAndOffset| {
                ConsumedMessage {
                    topic: topic.to_string(),
                    partition: 0,
                    offset: r.offset,
                    key: r.record.key,
                    value: r.record.value.unwrap_or_default(),
                    timestamp: r.record.timestamp,
                }
            })
            .collect();

        // Update offset to the last message + 1
        if let Some(last) = messages.last() {
            self.update_offset(topic, last.offset + 1).await;
        }

        debug!("Polled {} messages from topic {}", messages.len(), topic);
        Ok(messages)
    }

    /// Polls for messages from all configured topics.
    pub async fn poll_all(&self) -> Result<Vec<ConsumedMessage>> {
        let mut all_messages = Vec::new();

        for topic in &self.config.topics.clone() {
            match self.poll(topic).await {
                Ok(messages) => all_messages.extend(messages),
                Err(e) => {
                    warn!("Failed to poll topic {}: {}", topic, e);
                }
            }
        }

        Ok(all_messages)
    }

    /// Starts consuming messages with the given handler.
    pub async fn start<H: MessageHandler + 'static>(&self, handler: Arc<H>) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        info!("Starting consumer for topics: {:?}", self.config.topics);

        while self.running.load(Ordering::SeqCst) {
            let messages = self.poll_all().await?;

            for message in messages {
                match handler.handle(&message).await {
                    Ok(()) => {
                        debug!("Successfully processed message at offset {}", message.offset);
                    }
                    Err(e) => {
                        error!("Failed to process message: {}", e);
                        handler.on_dead_letter(&message, &e).await;

                        // Send to DLQ if configured
                        if let Some(dlq_topic) = &self.config.dead_letter_topic {
                            if let Err(dlq_err) = self.send_to_dlq(dlq_topic, &message).await {
                                error!("Failed to send message to DLQ: {}", dlq_err);
                            }
                        }
                    }
                }
            }

            // Small delay to prevent busy-waiting when no messages
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        info!("Consumer stopped");
        Ok(())
    }

    /// Sends a failed message to the dead letter queue.
    async fn send_to_dlq(&self, dlq_topic: &str, message: &ConsumedMessage) -> Result<()> {
        let partition_client = self.get_partition_client(dlq_topic).await?;

        let record = rskafka::record::Record {
            key: message.key.clone(),
            value: Some(message.value.clone()),
            headers: BTreeMap::from([
                ("original_topic".to_string(), message.topic.as_bytes().to_vec()),
                ("original_offset".to_string(), message.offset.to_string().into_bytes()),
            ]),
            timestamp: chrono::Utc::now(),
        };

        partition_client
            .produce(vec![record], rskafka::client::partition::Compression::NoCompression)
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to send to DLQ: {}", e)))?;

        warn!("Message sent to DLQ: {}", dlq_topic);
        Ok(())
    }

    /// Stops the consumer.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Consumer stop requested");
    }

    /// Checks if the consumer is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Checks if the consumer is connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    /// Gets the consumer configuration.
    pub fn config(&self) -> &ConsumerConfig {
        &self.config
    }

    /// Commits the current offsets (no-op for rskafka as it doesn't support consumer groups).
    pub async fn commit(&self) -> Result<()> {
        // rskafka doesn't support consumer groups, so offset management is manual
        debug!("Offset commit requested (manual offset management)");
        Ok(())
    }
}

/// Builder for creating an EventConsumer with custom configuration.
pub struct ConsumerBuilder {
    config: ConsumerConfig,
}

impl ConsumerBuilder {
    pub fn new() -> Self {
        Self {
            config: ConsumerConfig::default(),
        }
    }

    pub fn brokers(mut self, brokers: Vec<String>) -> Self {
        self.config.brokers = brokers;
        self
    }

    pub fn topics(mut self, topics: Vec<String>) -> Self {
        self.config.topics = topics;
        self
    }

    pub fn group_id(mut self, group_id: impl Into<String>) -> Self {
        self.config.group_id = group_id.into();
        self
    }

    pub fn auto_offset_reset(mut self, reset: OffsetReset) -> Self {
        self.config.auto_offset_reset = reset;
        self
    }

    pub fn fetch_max_wait_ms(mut self, ms: i32) -> Self {
        self.config.fetch_max_wait_ms = ms;
        self
    }

    pub fn max_poll_records(mut self, max: usize) -> Self {
        self.config.max_poll_records = max;
        self
    }

    pub fn dead_letter_topic(mut self, topic: Option<String>) -> Self {
        self.config.dead_letter_topic = topic;
        self
    }

    pub fn build(self) -> EventConsumer {
        EventConsumer::new(self.config)
    }
}

impl Default for ConsumerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_config_default() {
        let config = ConsumerConfig::default();
        assert_eq!(config.brokers, vec!["localhost:9092".to_string()]);
        assert_eq!(config.group_id, "settlement-engine");
        assert!(config.dead_letter_topic.is_some());
    }

    #[test]
    fn test_consumer_builder() {
        let consumer = ConsumerBuilder::new()
            .brokers(vec!["kafka:9092".to_string()])
            .topics(vec!["test.topic".to_string()])
            .group_id("test-group")
            .max_poll_records(50)
            .build();

        assert_eq!(consumer.config.brokers, vec!["kafka:9092".to_string()]);
        assert_eq!(consumer.config.topics, vec!["test.topic".to_string()]);
        assert_eq!(consumer.config.group_id, "test-group");
        assert_eq!(consumer.config.max_poll_records, 50);
    }

    #[test]
    fn test_consumed_message_key_str() {
        let message = ConsumedMessage {
            topic: "test".to_string(),
            partition: 0,
            offset: 0,
            key: Some(b"test-key".to_vec()),
            value: vec![],
            timestamp: chrono::Utc::now(),
        };

        assert_eq!(message.key_str(), Some("test-key".to_string()));
    }

    #[test]
    fn test_consumed_message_deserialize() {
        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct TestPayload {
            id: i32,
            name: String,
        }

        let message = ConsumedMessage {
            topic: "test".to_string(),
            partition: 0,
            offset: 0,
            key: None,
            value: br#"{"id": 1, "name": "test"}"#.to_vec(),
            timestamp: chrono::Utc::now(),
        };

        let payload: TestPayload = message.deserialize().unwrap();
        assert_eq!(payload.id, 1);
        assert_eq!(payload.name, "test");
    }
}
