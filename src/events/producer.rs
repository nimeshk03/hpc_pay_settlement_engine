use crate::error::{AppError, Result};
use anyhow::anyhow;
use chrono::Utc;
use rskafka::client::partition::{Compression, PartitionClient, UnknownTopicHandling};
use rskafka::client::ClientBuilder;
use rskafka::record::Record;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Configuration for the Kafka producer.
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    pub brokers: Vec<String>,
    pub default_topic: String,
    pub compression: CompressionType,
    pub retry_count: u32,
    pub retry_delay_ms: u64,
    pub request_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum CompressionType {
    #[default]
    None,
    Gzip,
    Snappy,
    Lz4,
    Zstd,
}

impl From<CompressionType> for Compression {
    fn from(ct: CompressionType) -> Self {
        match ct {
            CompressionType::None => Compression::NoCompression,
            CompressionType::Gzip => Compression::Gzip,
            CompressionType::Snappy => Compression::Snappy,
            CompressionType::Lz4 => Compression::Lz4,
            CompressionType::Zstd => Compression::Zstd,
        }
    }
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            brokers: vec!["localhost:9092".to_string()],
            default_topic: "settlement.transactions".to_string(),
            compression: CompressionType::default(),
            retry_count: 3,
            retry_delay_ms: 100,
            request_timeout_ms: 5000,
        }
    }
}

/// Kafka event producer for settlement events.
pub struct EventProducer {
    config: ProducerConfig,
    partition_clients: Arc<RwLock<BTreeMap<String, Arc<PartitionClient>>>>,
    client: Option<Arc<rskafka::client::Client>>,
}

impl EventProducer {
    /// Creates a new event producer with the given configuration.
    pub fn new(config: ProducerConfig) -> Self {
        Self {
            config,
            partition_clients: Arc::new(RwLock::new(BTreeMap::new())),
            client: None,
        }
    }

    /// Connects to the Kafka cluster.
    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Kafka brokers: {:?}", self.config.brokers);

        let client = ClientBuilder::new(self.config.brokers.clone())
            .build()
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to connect to Kafka: {}", e)))?;

        self.client = Some(Arc::new(client));
        info!("Successfully connected to Kafka");
        Ok(())
    }

    /// Gets or creates a partition client for the given topic.
    async fn get_partition_client(&self, topic: &str) -> Result<Arc<PartitionClient>> {
        // Check if we already have a client for this topic
        {
            let clients = self.partition_clients.read().await;
            if let Some(client) = clients.get(topic) {
                return Ok(client.clone());
            }
        }

        // Create a new partition client
        let kafka_client = self.client.as_ref().ok_or_else(|| {
            AppError::Internal(anyhow!("Kafka client not connected"))
        })?;

        let partition_client = kafka_client
            .partition_client(topic.to_string(), 0, UnknownTopicHandling::Retry)
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to get partition client: {}", e)))?;

        let client = Arc::new(partition_client);

        // Store the client
        {
            let mut clients = self.partition_clients.write().await;
            clients.insert(topic.to_string(), client.clone());
        }

        Ok(client)
    }

    /// Sends a message to the specified topic.
    pub async fn send<T: Serialize>(&self, topic: &str, key: Option<&str>, payload: &T) -> Result<i64> {
        let json = serde_json::to_vec(payload)
            .map_err(|e| AppError::Internal(anyhow!("Failed to serialize payload: {}", e)))?;

        self.send_raw(topic, key, json).await
    }

    /// Sends a raw message to the specified topic.
    pub async fn send_raw(&self, topic: &str, key: Option<&str>, payload: Vec<u8>) -> Result<i64> {
        let partition_client = self.get_partition_client(topic).await?;

        let record = Record {
            key: key.map(|k| k.as_bytes().to_vec()),
            value: Some(payload),
            headers: BTreeMap::new(),
            timestamp: Utc::now(),
        };

        let mut last_error = None;
        for attempt in 0..=self.config.retry_count {
            if attempt > 0 {
                warn!("Retrying Kafka send, attempt {}/{}", attempt, self.config.retry_count);
                tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms * attempt as u64)).await;
            }

            match partition_client
                .produce(vec![record.clone()], self.config.compression.into())
                .await
            {
                Ok(offsets) => {
                    let offset = offsets.first().copied().unwrap_or(0);
                    debug!("Message sent to topic {} at offset {}", topic, offset);
                    return Ok(offset);
                }
                Err(e) => {
                    error!("Failed to send message to Kafka: {}", e);
                    last_error = Some(e);
                }
            }
        }

        Err(AppError::Internal(anyhow!(
            "Failed to send message after {} retries: {:?}",
            self.config.retry_count,
            last_error
        )))
    }

    /// Sends multiple messages to the specified topic in a batch.
    pub async fn send_batch<T: Serialize>(
        &self,
        topic: &str,
        messages: &[(Option<String>, T)],
    ) -> Result<Vec<i64>> {
        let partition_client = self.get_partition_client(topic).await?;

        let records: Vec<Record> = messages
            .iter()
            .map(|(key, payload)| {
                let json = serde_json::to_vec(payload).unwrap_or_default();
                Record {
                    key: key.as_ref().map(|k| k.as_bytes().to_vec()),
                    value: Some(json),
                    headers: BTreeMap::new(),
                    timestamp: Utc::now(),
                }
            })
            .collect();

        let offsets = partition_client
            .produce(records, self.config.compression.into())
            .await
            .map_err(|e| AppError::Internal(anyhow!("Failed to send batch: {}", e)))?;

        debug!("Batch of {} messages sent to topic {}", messages.len(), topic);
        Ok(offsets)
    }

    /// Sends a message to the default topic.
    pub async fn send_default<T: Serialize>(&self, key: Option<&str>, payload: &T) -> Result<i64> {
        self.send(&self.config.default_topic, key, payload).await
    }

    /// Checks if the producer is connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    /// Gets the producer configuration.
    pub fn config(&self) -> &ProducerConfig {
        &self.config
    }
}

/// Builder for creating an EventProducer with custom configuration.
pub struct ProducerBuilder {
    config: ProducerConfig,
}

impl ProducerBuilder {
    pub fn new() -> Self {
        Self {
            config: ProducerConfig::default(),
        }
    }

    pub fn brokers(mut self, brokers: Vec<String>) -> Self {
        self.config.brokers = brokers;
        self
    }

    pub fn default_topic(mut self, topic: impl Into<String>) -> Self {
        self.config.default_topic = topic.into();
        self
    }

    pub fn compression(mut self, compression: CompressionType) -> Self {
        self.config.compression = compression;
        self
    }

    pub fn retry_count(mut self, count: u32) -> Self {
        self.config.retry_count = count;
        self
    }

    pub fn retry_delay_ms(mut self, delay: u64) -> Self {
        self.config.retry_delay_ms = delay;
        self
    }

    pub fn request_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.request_timeout_ms = timeout;
        self
    }

    pub fn build(self) -> EventProducer {
        EventProducer::new(self.config)
    }
}

impl Default for ProducerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_config_default() {
        let config = ProducerConfig::default();
        assert_eq!(config.brokers, vec!["localhost:9092".to_string()]);
        assert_eq!(config.retry_count, 3);
    }

    #[test]
    fn test_producer_builder() {
        let producer = ProducerBuilder::new()
            .brokers(vec!["kafka:9092".to_string()])
            .default_topic("test.topic")
            .compression(CompressionType::Gzip)
            .retry_count(5)
            .build();

        assert_eq!(producer.config.brokers, vec!["kafka:9092".to_string()]);
        assert_eq!(producer.config.default_topic, "test.topic");
        assert_eq!(producer.config.retry_count, 5);
    }

    #[test]
    fn test_compression_conversion() {
        assert!(matches!(Compression::from(CompressionType::None), Compression::NoCompression));
        assert!(matches!(Compression::from(CompressionType::Gzip), Compression::Gzip));
        assert!(matches!(Compression::from(CompressionType::Snappy), Compression::Snappy));
    }
}
