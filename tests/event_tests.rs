mod common;

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal_macros::dec;
use settlement_engine::error::Result;
use settlement_engine::events::{
    BatchEvent, ConsumerConfig, EventConsumer, EventEnvelope, EventProducer, EventType,
    MessageHandler, NettingEvent, PositionEvent, ProducerConfig, SettlementEvent,
    TransactionEvent,
};
use settlement_engine::events::consumer::ConsumedMessage;
use settlement_engine::events::types::topics;
use settlement_engine::models::{BatchStatus, TransactionStatus, TransactionType};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn get_kafka_brokers() -> Vec<String> {
    std::env::var("KAFKA_BROKERS")
        .unwrap_or_else(|_| "localhost:9092".to_string())
        .split(',')
        .map(|s| s.to_string())
        .collect()
}

fn unique_topic() -> String {
    format!("test.{}", Uuid::new_v4().to_string().replace("-", "")[..8].to_string())
}

#[tokio::test]
async fn test_producer_config_builder() {
    let config = ProducerConfig {
        brokers: vec!["kafka:9092".to_string()],
        default_topic: "test.topic".to_string(),
        ..Default::default()
    };

    assert_eq!(config.brokers, vec!["kafka:9092".to_string()]);
    assert_eq!(config.default_topic, "test.topic");
    assert_eq!(config.retry_count, 3);
}

#[tokio::test]
async fn test_consumer_config_builder() {
    let config = ConsumerConfig {
        brokers: vec!["kafka:9092".to_string()],
        topics: vec!["test.topic".to_string()],
        group_id: "test-group".to_string(),
        ..Default::default()
    };

    assert_eq!(config.brokers, vec!["kafka:9092".to_string()]);
    assert_eq!(config.group_id, "test-group");
    assert!(config.dead_letter_topic.is_some());
}

#[tokio::test]
async fn test_transaction_event_creation() {
    let event = TransactionEvent {
        transaction_id: Uuid::new_v4(),
        external_id: "TX-001".to_string(),
        transaction_type: TransactionType::Payment,
        status: TransactionStatus::Settled,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(1),
        net_amount: dec!(99),
        batch_id: None,
        idempotency_key: "IDEM-001".to_string(),
        created_at: Utc::now(),
        settled_at: Some(Utc::now()),
    };

    let envelope = EventEnvelope::new(EventType::TransactionSettled, event);

    assert_eq!(envelope.event_type, EventType::TransactionSettled);
    assert_eq!(envelope.source, "settlement-engine");

    // Test serialization
    let json = serde_json::to_string(&envelope).expect("Failed to serialize");
    assert!(json.contains("TRANSACTION_SETTLED"));
    assert!(json.contains("TX-001"));
}

#[tokio::test]
async fn test_batch_event_creation() {
    let event = BatchEvent {
        batch_id: Uuid::new_v4(),
        status: BatchStatus::Completed,
        settlement_date: Utc::now().date_naive(),
        currency: "USD".to_string(),
        total_transactions: 100,
        gross_amount: dec!(100000),
        net_amount: dec!(15000),
        fee_amount: dec!(100),
        created_at: Utc::now(),
        completed_at: Some(Utc::now()),
    };

    let envelope = EventEnvelope::new(EventType::BatchCompleted, event)
        .with_correlation_id("batch-123".to_string());

    assert_eq!(envelope.event_type, EventType::BatchCompleted);
    assert_eq!(envelope.correlation_id, Some("batch-123".to_string()));
}

#[tokio::test]
async fn test_position_event_creation() {
    let event = PositionEvent {
        batch_id: Uuid::new_v4(),
        participant_id: Uuid::new_v4(),
        currency: "USD".to_string(),
        gross_receivable: dec!(50000),
        gross_payable: dec!(30000),
        net_position: dec!(20000),
        transaction_count: 25,
        calculated_at: Utc::now(),
    };

    let envelope = EventEnvelope::new(EventType::PositionCalculated, event);

    assert_eq!(envelope.event_type, EventType::PositionCalculated);
    assert_eq!(PositionEvent::topic(), topics::POSITIONS);
}

#[tokio::test]
async fn test_netting_event_creation() {
    let event = NettingEvent {
        batch_id: Uuid::new_v4(),
        currency: "USD".to_string(),
        participant_count: 10,
        total_transactions: 500,
        gross_volume: dec!(1000000),
        net_volume: dec!(150000),
        reduction_amount: dec!(850000),
        reduction_percentage: dec!(85),
        net_receivers: 4,
        net_payers: 6,
        completed_at: Utc::now(),
    };

    let envelope = EventEnvelope::new(EventType::NettingCompleted, event);

    assert_eq!(envelope.event_type, EventType::NettingCompleted);
}

#[tokio::test]
async fn test_settlement_event_creation() {
    let event = SettlementEvent {
        batch_id: Uuid::new_v4(),
        currency: "USD".to_string(),
        settlement_date: Utc::now().date_naive(),
        total_transactions: 1000,
        gross_amount: dec!(5000000),
        net_amount: dec!(750000),
        netting_efficiency: dec!(85),
        processing_time_ms: 1234,
        completed_at: Utc::now(),
    };

    let envelope = EventEnvelope::new(EventType::SettlementCompleted, event);

    assert_eq!(envelope.event_type, EventType::SettlementCompleted);
    assert_eq!(SettlementEvent::topic(), topics::COMPLETED);
}

#[tokio::test]
async fn test_event_topics() {
    assert_eq!(topics::TRANSACTIONS, "settlement.transactions");
    assert_eq!(topics::BATCHES, "settlement.batches");
    assert_eq!(topics::POSITIONS, "settlement.positions");
    assert_eq!(topics::COMPLETED, "settlement.completed");
}

#[tokio::test]
async fn test_producer_not_connected() {
    let producer = EventProducer::new(ProducerConfig::default());

    assert!(!producer.is_connected());
}

#[tokio::test]
async fn test_consumer_not_connected() {
    let consumer = EventConsumer::new(ConsumerConfig::default());

    assert!(!consumer.is_connected());
    assert!(!consumer.is_running());
}

#[tokio::test]
async fn test_consumed_message_deserialization() {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct TestPayload {
        id: String,
        value: i32,
    }

    let message = ConsumedMessage {
        topic: "test".to_string(),
        partition: 0,
        offset: 42,
        key: Some(b"test-key".to_vec()),
        value: br#"{"id": "test-123", "value": 42}"#.to_vec(),
        timestamp: Utc::now(),
    };

    assert_eq!(message.key_str(), Some("test-key".to_string()));

    let payload: TestPayload = message.deserialize().expect("Failed to deserialize");
    assert_eq!(payload.id, "test-123");
    assert_eq!(payload.value, 42);
}

struct TestMessageHandler {
    message_count: AtomicUsize,
}

#[async_trait]
impl MessageHandler for TestMessageHandler {
    async fn handle(&self, _message: &ConsumedMessage) -> Result<()> {
        self.message_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn test_message_handler_trait() {
    let handler = TestMessageHandler {
        message_count: AtomicUsize::new(0),
    };

    let message = ConsumedMessage {
        topic: "test".to_string(),
        partition: 0,
        offset: 0,
        key: None,
        value: b"test".to_vec(),
        timestamp: Utc::now(),
    };

    handler.handle(&message).await.expect("Handler failed");
    assert_eq!(handler.message_count.load(Ordering::SeqCst), 1);

    handler.handle(&message).await.expect("Handler failed");
    assert_eq!(handler.message_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
#[ignore = "Requires running Kafka"]
async fn test_producer_connect_and_send() {
    let brokers = get_kafka_brokers();
    let topic = unique_topic();

    let mut producer = EventProducer::new(ProducerConfig {
        brokers,
        default_topic: topic.clone(),
        ..Default::default()
    });

    producer.connect().await.expect("Failed to connect producer");
    assert!(producer.is_connected());

    let event = TransactionEvent {
        transaction_id: Uuid::new_v4(),
        external_id: "TX-TEST".to_string(),
        transaction_type: TransactionType::Payment,
        status: TransactionStatus::Settled,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        net_amount: dec!(100),
        batch_id: None,
        idempotency_key: "IDEM-TEST".to_string(),
        created_at: Utc::now(),
        settled_at: Some(Utc::now()),
    };

    let envelope = EventEnvelope::new(EventType::TransactionSettled, event);
    let offset = producer.send(&topic, Some("test-key"), &envelope).await
        .expect("Failed to send message");

    assert!(offset >= 0);
}

#[tokio::test]
#[ignore = "Requires running Kafka"]
async fn test_producer_send_batch() {
    let brokers = get_kafka_brokers();
    let topic = unique_topic();

    let mut producer = EventProducer::new(ProducerConfig {
        brokers,
        default_topic: topic.clone(),
        ..Default::default()
    });

    producer.connect().await.expect("Failed to connect producer");

    let messages: Vec<(Option<String>, EventEnvelope<TransactionEvent>)> = (0..5)
        .map(|i| {
            let event = TransactionEvent {
                transaction_id: Uuid::new_v4(),
                external_id: format!("TX-BATCH-{}", i),
                transaction_type: TransactionType::Payment,
                status: TransactionStatus::Settled,
                source_account_id: Uuid::new_v4(),
                destination_account_id: Uuid::new_v4(),
                amount: dec!(100),
                currency: "USD".to_string(),
                fee_amount: dec!(0),
                net_amount: dec!(100),
                batch_id: None,
                idempotency_key: format!("IDEM-BATCH-{}", i),
                created_at: Utc::now(),
                settled_at: Some(Utc::now()),
            };
            (Some(format!("key-{}", i)), EventEnvelope::new(EventType::TransactionSettled, event))
        })
        .collect();

    let offsets = producer.send_batch(&topic, &messages).await
        .expect("Failed to send batch");

    assert_eq!(offsets.len(), 5);
}

#[tokio::test]
#[ignore = "Requires running Kafka"]
async fn test_consumer_connect_and_poll() {
    let brokers = get_kafka_brokers();
    let topic = unique_topic();

    // First, produce a message
    let mut producer = EventProducer::new(ProducerConfig {
        brokers: brokers.clone(),
        default_topic: topic.clone(),
        ..Default::default()
    });
    producer.connect().await.expect("Failed to connect producer");

    let event = TransactionEvent {
        transaction_id: Uuid::new_v4(),
        external_id: "TX-CONSUMER-TEST".to_string(),
        transaction_type: TransactionType::Payment,
        status: TransactionStatus::Settled,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        net_amount: dec!(100),
        batch_id: None,
        idempotency_key: "IDEM-CONSUMER-TEST".to_string(),
        created_at: Utc::now(),
        settled_at: Some(Utc::now()),
    };

    let envelope = EventEnvelope::new(EventType::TransactionSettled, event);
    producer.send(&topic, Some("consumer-test"), &envelope).await
        .expect("Failed to send message");

    // Now consume
    let mut consumer = EventConsumer::new(ConsumerConfig {
        brokers,
        topics: vec![topic.clone()],
        group_id: format!("test-group-{}", Uuid::new_v4()),
        ..Default::default()
    });
    consumer.connect().await.expect("Failed to connect consumer");
    assert!(consumer.is_connected());

    let messages = consumer.poll(&topic).await.expect("Failed to poll");
    assert!(!messages.is_empty());

    let received: EventEnvelope<TransactionEvent> = messages[0].deserialize()
        .expect("Failed to deserialize");
    assert_eq!(received.event_type, EventType::TransactionSettled);
    assert_eq!(received.payload.external_id, "TX-CONSUMER-TEST");
}

#[tokio::test]
#[ignore = "Requires running Kafka"]
async fn test_end_to_end_event_flow() {
    let brokers = get_kafka_brokers();
    let topic = unique_topic();

    // Producer
    let mut producer = EventProducer::new(ProducerConfig {
        brokers: brokers.clone(),
        default_topic: topic.clone(),
        ..Default::default()
    });
    producer.connect().await.expect("Failed to connect producer");

    // Send multiple event types
    let batch_id = Uuid::new_v4();

    // 1. Batch created
    let batch_event = BatchEvent {
        batch_id,
        status: BatchStatus::Pending,
        settlement_date: Utc::now().date_naive(),
        currency: "USD".to_string(),
        total_transactions: 0,
        gross_amount: dec!(0),
        net_amount: dec!(0),
        fee_amount: dec!(0),
        created_at: Utc::now(),
        completed_at: None,
    };
    producer.send(&topic, Some(&batch_id.to_string()), 
        &EventEnvelope::new(EventType::BatchCreated, batch_event)).await.unwrap();

    // 2. Transaction added
    let tx_event = TransactionEvent {
        transaction_id: Uuid::new_v4(),
        external_id: "TX-E2E".to_string(),
        transaction_type: TransactionType::Payment,
        status: TransactionStatus::Settled,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(1000),
        currency: "USD".to_string(),
        fee_amount: dec!(10),
        net_amount: dec!(990),
        batch_id: Some(batch_id),
        idempotency_key: "IDEM-E2E".to_string(),
        created_at: Utc::now(),
        settled_at: Some(Utc::now()),
    };
    producer.send(&topic, Some(&batch_id.to_string()),
        &EventEnvelope::new(EventType::TransactionSettled, tx_event)).await.unwrap();

    // 3. Netting completed
    let netting_event = NettingEvent {
        batch_id,
        currency: "USD".to_string(),
        participant_count: 2,
        total_transactions: 1,
        gross_volume: dec!(1000),
        net_volume: dec!(1000),
        reduction_amount: dec!(0),
        reduction_percentage: dec!(0),
        net_receivers: 1,
        net_payers: 1,
        completed_at: Utc::now(),
    };
    producer.send(&topic, Some(&batch_id.to_string()),
        &EventEnvelope::new(EventType::NettingCompleted, netting_event)).await.unwrap();

    // Consumer
    let mut consumer = EventConsumer::new(ConsumerConfig {
        brokers,
        topics: vec![topic.clone()],
        group_id: format!("e2e-test-{}", Uuid::new_v4()),
        ..Default::default()
    });
    consumer.connect().await.expect("Failed to connect consumer");

    let messages = consumer.poll(&topic).await.expect("Failed to poll");
    assert_eq!(messages.len(), 3);
}
