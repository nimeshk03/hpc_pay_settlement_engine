use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Type of financial transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "transaction_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionType {
    /// Standard payment from source to destination.
    Payment,
    /// Reversal of a previous payment.
    Refund,
    /// Disputed transaction reversal.
    Chargeback,
    /// Internal transfer between accounts.
    Transfer,
    /// Fee charged for services.
    Fee,
}

impl TransactionType {
    /// Returns true if this transaction type can be reversed.
    pub fn is_reversible(&self) -> bool {
        matches!(self, TransactionType::Payment | TransactionType::Transfer)
    }

    /// Returns the reversal type for this transaction.
    pub fn reversal_type(&self) -> Option<TransactionType> {
        match self {
            TransactionType::Payment => Some(TransactionType::Refund),
            TransactionType::Transfer => Some(TransactionType::Transfer),
            _ => None,
        }
    }
}

/// Status of a transaction in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "transaction_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionStatus {
    /// Transaction is created but not yet processed.
    Pending,
    /// Transaction has been successfully settled.
    Settled,
    /// Transaction processing failed.
    Failed,
    /// Transaction has been reversed.
    Reversed,
}

impl TransactionStatus {
    /// Returns true if the transaction is in a final state.
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            TransactionStatus::Settled | TransactionStatus::Failed | TransactionStatus::Reversed
        )
    }

    /// Returns true if the transaction can be reversed.
    pub fn can_be_reversed(&self) -> bool {
        matches!(self, TransactionStatus::Settled)
    }
}

/// Represents a financial transaction in the settlement system.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TransactionRecord {
    pub id: Uuid,
    pub external_id: String,
    #[sqlx(rename = "type")]
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub source_account_id: Uuid,
    pub destination_account_id: Uuid,
    /// Gross amount of the transaction.
    pub amount: Decimal,
    pub currency: String,
    /// Fee charged for this transaction.
    pub fee_amount: Decimal,
    /// Net amount after fees (amount - fee_amount).
    pub net_amount: Decimal,
    pub settlement_batch_id: Option<Uuid>,
    /// Unique key for idempotency checking.
    pub idempotency_key: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl TransactionRecord {
    /// Creates a new transaction record.
    pub fn new(
        external_id: String,
        transaction_type: TransactionType,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: String,
        fee_amount: Decimal,
        idempotency_key: String,
    ) -> Self {
        let net_amount = amount - fee_amount;
        Self {
            id: Uuid::new_v4(),
            external_id,
            transaction_type,
            status: TransactionStatus::Pending,
            source_account_id,
            destination_account_id,
            amount,
            currency,
            fee_amount,
            net_amount,
            settlement_batch_id: None,
            idempotency_key,
            metadata: None,
            created_at: Utc::now(),
            settled_at: None,
        }
    }

    /// Creates a payment transaction.
    pub fn payment(
        external_id: String,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: String,
        fee_amount: Decimal,
        idempotency_key: String,
    ) -> Self {
        Self::new(
            external_id,
            TransactionType::Payment,
            source_account_id,
            destination_account_id,
            amount,
            currency,
            fee_amount,
            idempotency_key,
        )
    }

    /// Creates a transfer transaction.
    pub fn transfer(
        external_id: String,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: String,
        idempotency_key: String,
    ) -> Self {
        Self::new(
            external_id,
            TransactionType::Transfer,
            source_account_id,
            destination_account_id,
            amount,
            currency,
            Decimal::ZERO,
            idempotency_key,
        )
    }

    /// Adds metadata to the transaction.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Marks the transaction as settled.
    pub fn settle(&mut self) {
        self.status = TransactionStatus::Settled;
        self.settled_at = Some(Utc::now());
    }

    /// Marks the transaction as failed.
    pub fn fail(&mut self) {
        self.status = TransactionStatus::Failed;
    }

    /// Marks the transaction as reversed.
    pub fn reverse(&mut self) {
        self.status = TransactionStatus::Reversed;
    }

    /// Assigns the transaction to a settlement batch.
    pub fn assign_to_batch(&mut self, batch_id: Uuid) {
        self.settlement_batch_id = Some(batch_id);
    }

    /// Checks if the transaction can be processed.
    pub fn can_process(&self) -> bool {
        self.status == TransactionStatus::Pending
    }

    /// Checks if the transaction can be reversed.
    pub fn can_reverse(&self) -> bool {
        self.status.can_be_reversed() && self.transaction_type.is_reversible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_transaction_type_reversible() {
        assert!(TransactionType::Payment.is_reversible());
        assert!(TransactionType::Transfer.is_reversible());
        assert!(!TransactionType::Refund.is_reversible());
        assert!(!TransactionType::Chargeback.is_reversible());
        assert!(!TransactionType::Fee.is_reversible());
    }

    #[test]
    fn test_transaction_type_reversal_type() {
        assert_eq!(
            TransactionType::Payment.reversal_type(),
            Some(TransactionType::Refund)
        );
        assert_eq!(
            TransactionType::Transfer.reversal_type(),
            Some(TransactionType::Transfer)
        );
        assert_eq!(TransactionType::Refund.reversal_type(), None);
    }

    #[test]
    fn test_transaction_status_final() {
        assert!(!TransactionStatus::Pending.is_final());
        assert!(TransactionStatus::Settled.is_final());
        assert!(TransactionStatus::Failed.is_final());
        assert!(TransactionStatus::Reversed.is_final());
    }

    #[test]
    fn test_transaction_status_can_be_reversed() {
        assert!(!TransactionStatus::Pending.can_be_reversed());
        assert!(TransactionStatus::Settled.can_be_reversed());
        assert!(!TransactionStatus::Failed.can_be_reversed());
        assert!(!TransactionStatus::Reversed.can_be_reversed());
    }

    #[test]
    fn test_transaction_creation() {
        let source = Uuid::new_v4();
        let dest = Uuid::new_v4();
        let tx = TransactionRecord::new(
            "EXT-001".to_string(),
            TransactionType::Payment,
            source,
            dest,
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        assert_eq!(tx.external_id, "EXT-001");
        assert_eq!(tx.transaction_type, TransactionType::Payment);
        assert_eq!(tx.status, TransactionStatus::Pending);
        assert_eq!(tx.amount, dec!(100));
        assert_eq!(tx.fee_amount, dec!(2.50));
        assert_eq!(tx.net_amount, dec!(97.50));
        assert!(tx.settlement_batch_id.is_none());
        assert!(tx.settled_at.is_none());
    }

    #[test]
    fn test_payment_creation() {
        let tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        assert_eq!(tx.transaction_type, TransactionType::Payment);
    }

    #[test]
    fn test_transfer_creation() {
        let tx = TransactionRecord::transfer(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            "idem-key-001".to_string(),
        );

        assert_eq!(tx.transaction_type, TransactionType::Transfer);
        assert_eq!(tx.fee_amount, Decimal::ZERO);
        assert_eq!(tx.net_amount, dec!(100));
    }

    #[test]
    fn test_transaction_settle() {
        let mut tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        assert!(tx.can_process());
        tx.settle();
        assert_eq!(tx.status, TransactionStatus::Settled);
        assert!(tx.settled_at.is_some());
        assert!(!tx.can_process());
    }

    #[test]
    fn test_transaction_fail() {
        let mut tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        tx.fail();
        assert_eq!(tx.status, TransactionStatus::Failed);
        assert!(!tx.can_reverse());
    }

    #[test]
    fn test_transaction_reverse() {
        let mut tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        tx.settle();
        assert!(tx.can_reverse());

        tx.reverse();
        assert_eq!(tx.status, TransactionStatus::Reversed);
        assert!(!tx.can_reverse());
    }

    #[test]
    fn test_transaction_batch_assignment() {
        let mut tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        let batch_id = Uuid::new_v4();
        tx.assign_to_batch(batch_id);
        assert_eq!(tx.settlement_batch_id, Some(batch_id));
    }

    #[test]
    fn test_transaction_with_metadata() {
        let tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        )
        .with_metadata(serde_json::json!({"merchant": "Test Shop"}));

        assert!(tx.metadata.is_some());
    }

    #[test]
    fn test_serialization() {
        let tx = TransactionRecord::payment(
            "EXT-001".to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100.50),
            "USD".to_string(),
            dec!(2.50),
            "idem-key-001".to_string(),
        );

        let json = serde_json::to_string(&tx).unwrap();
        let deserialized: TransactionRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.external_id, tx.external_id);
        assert_eq!(deserialized.amount, dec!(100.50));
        assert_eq!(deserialized.transaction_type, TransactionType::Payment);
    }
}
