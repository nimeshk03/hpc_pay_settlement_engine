use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{BatchStatus, TransactionStatus, TransactionType};

/// Topics for settlement events.
pub mod topics {
    pub const TRANSACTIONS: &str = "settlement.transactions";
    pub const BATCHES: &str = "settlement.batches";
    pub const POSITIONS: &str = "settlement.positions";
    pub const COMPLETED: &str = "settlement.completed";
}

/// Type of settlement event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    TransactionCreated,
    TransactionSettled,
    TransactionFailed,
    TransactionReversed,
    BatchCreated,
    BatchProcessing,
    BatchCompleted,
    BatchFailed,
    PositionCalculated,
    NettingCompleted,
    SettlementCompleted,
}

/// Envelope wrapping all events with common metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub event_id: Uuid,
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
    pub source: String,
    pub correlation_id: Option<String>,
    pub payload: T,
}

impl<T> EventEnvelope<T> {
    pub fn new(event_type: EventType, payload: T) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_type,
            timestamp: Utc::now(),
            source: "settlement-engine".to_string(),
            correlation_id: None,
            payload,
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }
}

/// Event payload for transaction-related events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEvent {
    pub transaction_id: Uuid,
    pub external_id: String,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub source_account_id: Uuid,
    pub destination_account_id: Uuid,
    pub amount: Decimal,
    pub currency: String,
    pub fee_amount: Decimal,
    pub net_amount: Decimal,
    pub batch_id: Option<Uuid>,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl TransactionEvent {
    pub fn topic() -> &'static str {
        topics::TRANSACTIONS
    }
}

/// Event payload for batch-related events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvent {
    pub batch_id: Uuid,
    pub status: BatchStatus,
    pub settlement_date: chrono::NaiveDate,
    pub currency: String,
    pub total_transactions: i32,
    pub gross_amount: Decimal,
    pub net_amount: Decimal,
    pub fee_amount: Decimal,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl BatchEvent {
    pub fn topic() -> &'static str {
        topics::BATCHES
    }
}

/// Event payload for netting position events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionEvent {
    pub batch_id: Uuid,
    pub participant_id: Uuid,
    pub currency: String,
    pub gross_receivable: Decimal,
    pub gross_payable: Decimal,
    pub net_position: Decimal,
    pub transaction_count: i32,
    pub calculated_at: DateTime<Utc>,
}

impl PositionEvent {
    pub fn topic() -> &'static str {
        topics::POSITIONS
    }
}

/// Event payload for netting completion events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NettingEvent {
    pub batch_id: Uuid,
    pub currency: String,
    pub participant_count: i32,
    pub total_transactions: i32,
    pub gross_volume: Decimal,
    pub net_volume: Decimal,
    pub reduction_amount: Decimal,
    pub reduction_percentage: Decimal,
    pub net_receivers: i32,
    pub net_payers: i32,
    pub completed_at: DateTime<Utc>,
}

impl NettingEvent {
    pub fn topic() -> &'static str {
        topics::POSITIONS
    }
}

/// Event payload for settlement completion events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementEvent {
    pub batch_id: Uuid,
    pub currency: String,
    pub settlement_date: chrono::NaiveDate,
    pub total_transactions: i32,
    pub gross_amount: Decimal,
    pub net_amount: Decimal,
    pub netting_efficiency: Decimal,
    pub processing_time_ms: u64,
    pub completed_at: DateTime<Utc>,
}

impl SettlementEvent {
    pub fn topic() -> &'static str {
        topics::COMPLETED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_event_envelope_creation() {
        let payload = TransactionEvent {
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

        let envelope = EventEnvelope::new(EventType::TransactionSettled, payload);

        assert_eq!(envelope.event_type, EventType::TransactionSettled);
        assert_eq!(envelope.source, "settlement-engine");
        assert!(envelope.correlation_id.is_none());
    }

    #[test]
    fn test_event_envelope_with_correlation_id() {
        let payload = BatchEvent {
            batch_id: Uuid::new_v4(),
            status: BatchStatus::Completed,
            settlement_date: Utc::now().date_naive(),
            currency: "USD".to_string(),
            total_transactions: 10,
            gross_amount: dec!(1000),
            net_amount: dec!(900),
            fee_amount: dec!(10),
            created_at: Utc::now(),
            completed_at: Some(Utc::now()),
        };

        let envelope = EventEnvelope::new(EventType::BatchCompleted, payload)
            .with_correlation_id("corr-123".to_string());

        assert_eq!(envelope.correlation_id, Some("corr-123".to_string()));
    }

    #[test]
    fn test_event_serialization() {
        let event = NettingEvent {
            batch_id: Uuid::new_v4(),
            currency: "USD".to_string(),
            participant_count: 5,
            total_transactions: 100,
            gross_volume: dec!(100000),
            net_volume: dec!(15000),
            reduction_amount: dec!(85000),
            reduction_percentage: dec!(85),
            net_receivers: 2,
            net_payers: 3,
            completed_at: Utc::now(),
        };

        let envelope = EventEnvelope::new(EventType::NettingCompleted, event);
        let json = serde_json::to_string(&envelope).expect("Failed to serialize");
        
        assert!(json.contains("NETTING_COMPLETED"));
        assert!(json.contains("settlement-engine"));
    }

    #[test]
    fn test_topic_constants() {
        assert_eq!(topics::TRANSACTIONS, "settlement.transactions");
        assert_eq!(topics::BATCHES, "settlement.batches");
        assert_eq!(topics::POSITIONS, "settlement.positions");
        assert_eq!(topics::COMPLETED, "settlement.completed");
    }
}
