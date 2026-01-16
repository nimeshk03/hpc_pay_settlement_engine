use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Status of a settlement batch in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "batch_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BatchStatus {
    /// Batch is open and accepting transactions.
    Pending,
    /// Batch is being processed (netting, settlement).
    Processing,
    /// Batch has been successfully settled.
    Completed,
    /// Batch processing failed.
    Failed,
}

impl BatchStatus {
    /// Returns true if the batch can accept new transactions.
    pub fn can_accept_transactions(&self) -> bool {
        matches!(self, BatchStatus::Pending)
    }

    /// Returns true if the batch is in a final state.
    pub fn is_final(&self) -> bool {
        matches!(self, BatchStatus::Completed | BatchStatus::Failed)
    }

    /// Returns true if the batch can be processed.
    pub fn can_process(&self) -> bool {
        matches!(self, BatchStatus::Pending)
    }
}

/// Represents a settlement batch that groups transactions for batch processing.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SettlementBatch {
    pub id: Uuid,
    pub status: BatchStatus,
    /// The date for which this batch settles transactions.
    pub settlement_date: NaiveDate,
    /// The cut-off time after which no more transactions are accepted.
    pub cut_off_time: DateTime<Utc>,
    /// Total number of transactions in the batch.
    pub total_transactions: i32,
    /// Gross amount before netting.
    pub gross_amount: Decimal,
    /// Net amount after netting.
    pub net_amount: Decimal,
    /// Total fees collected.
    pub fee_amount: Decimal,
    pub currency: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl SettlementBatch {
    /// Creates a new settlement batch.
    pub fn new(settlement_date: NaiveDate, cut_off_time: DateTime<Utc>, currency: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: BatchStatus::Pending,
            settlement_date,
            cut_off_time,
            total_transactions: 0,
            gross_amount: Decimal::ZERO,
            net_amount: Decimal::ZERO,
            fee_amount: Decimal::ZERO,
            currency,
            metadata: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    /// Creates a batch for today with a specific cut-off time.
    pub fn for_today(cut_off_time: DateTime<Utc>, currency: String) -> Self {
        Self::new(Utc::now().date_naive(), cut_off_time, currency)
    }

    /// Adds metadata to the batch.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Checks if the batch can accept a new transaction.
    pub fn can_accept_transaction(&self) -> bool {
        self.status.can_accept_transactions() && Utc::now() < self.cut_off_time
    }

    /// Adds a transaction to the batch totals.
    pub fn add_transaction(&mut self, amount: Decimal, fee: Decimal) {
        self.total_transactions += 1;
        self.gross_amount += amount;
        self.fee_amount += fee;
    }

    /// Removes a transaction from the batch totals.
    pub fn remove_transaction(&mut self, amount: Decimal, fee: Decimal) {
        self.total_transactions = (self.total_transactions - 1).max(0);
        self.gross_amount -= amount;
        self.fee_amount -= fee;
    }

    /// Sets the net amount after netting calculation.
    pub fn set_net_amount(&mut self, net_amount: Decimal) {
        self.net_amount = net_amount;
    }

    /// Starts processing the batch.
    pub fn start_processing(&mut self) -> Result<(), BatchError> {
        if !self.status.can_process() {
            return Err(BatchError::InvalidStateTransition {
                from: self.status,
                to: BatchStatus::Processing,
            });
        }
        self.status = BatchStatus::Processing;
        Ok(())
    }

    /// Marks the batch as completed.
    pub fn complete(&mut self) -> Result<(), BatchError> {
        if self.status != BatchStatus::Processing {
            return Err(BatchError::InvalidStateTransition {
                from: self.status,
                to: BatchStatus::Completed,
            });
        }
        self.status = BatchStatus::Completed;
        self.completed_at = Some(Utc::now());
        Ok(())
    }

    /// Marks the batch as failed.
    pub fn fail(&mut self) -> Result<(), BatchError> {
        if self.status != BatchStatus::Processing {
            return Err(BatchError::InvalidStateTransition {
                from: self.status,
                to: BatchStatus::Failed,
            });
        }
        self.status = BatchStatus::Failed;
        self.completed_at = Some(Utc::now());
        Ok(())
    }

    /// Calculates the netting efficiency (reduction percentage).
    pub fn netting_efficiency(&self) -> Decimal {
        if self.gross_amount.is_zero() {
            return Decimal::ZERO;
        }
        let reduction = self.gross_amount - self.net_amount;
        (reduction / self.gross_amount) * Decimal::from(100)
    }
}

#[derive(Debug, Clone)]
pub enum BatchError {
    InvalidStateTransition { from: BatchStatus, to: BatchStatus },
    CutOffTimePassed,
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchError::InvalidStateTransition { from, to } => {
                write!(f, "Invalid batch state transition from {:?} to {:?}", from, to)
            }
            BatchError::CutOffTimePassed => write!(f, "Batch cut-off time has passed"),
        }
    }
}

impl std::error::Error for BatchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    #[test]
    fn test_batch_status_can_accept_transactions() {
        assert!(BatchStatus::Pending.can_accept_transactions());
        assert!(!BatchStatus::Processing.can_accept_transactions());
        assert!(!BatchStatus::Completed.can_accept_transactions());
        assert!(!BatchStatus::Failed.can_accept_transactions());
    }

    #[test]
    fn test_batch_status_is_final() {
        assert!(!BatchStatus::Pending.is_final());
        assert!(!BatchStatus::Processing.is_final());
        assert!(BatchStatus::Completed.is_final());
        assert!(BatchStatus::Failed.is_final());
    }

    #[test]
    fn test_batch_creation() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
        let cut_off = Utc::now() + Duration::hours(2);
        let batch = SettlementBatch::new(date, cut_off, "USD".to_string());

        assert_eq!(batch.status, BatchStatus::Pending);
        assert_eq!(batch.settlement_date, date);
        assert_eq!(batch.total_transactions, 0);
        assert_eq!(batch.gross_amount, Decimal::ZERO);
        assert!(batch.completed_at.is_none());
    }

    #[test]
    fn test_batch_for_today() {
        let cut_off = Utc::now() + Duration::hours(2);
        let batch = SettlementBatch::for_today(cut_off, "USD".to_string());

        assert_eq!(batch.settlement_date, Utc::now().date_naive());
    }

    #[test]
    fn test_batch_add_transaction() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        batch.add_transaction(dec!(100), dec!(2.50));
        assert_eq!(batch.total_transactions, 1);
        assert_eq!(batch.gross_amount, dec!(100));
        assert_eq!(batch.fee_amount, dec!(2.50));

        batch.add_transaction(dec!(200), dec!(5.00));
        assert_eq!(batch.total_transactions, 2);
        assert_eq!(batch.gross_amount, dec!(300));
        assert_eq!(batch.fee_amount, dec!(7.50));
    }

    #[test]
    fn test_batch_remove_transaction() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        batch.add_transaction(dec!(100), dec!(2.50));
        batch.add_transaction(dec!(200), dec!(5.00));
        batch.remove_transaction(dec!(100), dec!(2.50));

        assert_eq!(batch.total_transactions, 1);
        assert_eq!(batch.gross_amount, dec!(200));
        assert_eq!(batch.fee_amount, dec!(5.00));
    }

    #[test]
    fn test_batch_lifecycle() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        // Start processing
        assert!(batch.start_processing().is_ok());
        assert_eq!(batch.status, BatchStatus::Processing);

        // Complete
        assert!(batch.complete().is_ok());
        assert_eq!(batch.status, BatchStatus::Completed);
        assert!(batch.completed_at.is_some());
    }

    #[test]
    fn test_batch_fail() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        batch.start_processing().unwrap();
        assert!(batch.fail().is_ok());
        assert_eq!(batch.status, BatchStatus::Failed);
    }

    #[test]
    fn test_batch_invalid_state_transition() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        // Cannot complete from Pending
        assert!(batch.complete().is_err());

        // Cannot fail from Pending
        assert!(batch.fail().is_err());

        batch.start_processing().unwrap();
        batch.complete().unwrap();

        // Cannot start processing from Completed
        assert!(batch.start_processing().is_err());
    }

    #[test]
    fn test_batch_netting_efficiency() {
        let mut batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        batch.gross_amount = dec!(1000);
        batch.net_amount = dec!(150);

        // (1000 - 150) / 1000 * 100 = 85%
        assert_eq!(batch.netting_efficiency(), dec!(85));
    }

    #[test]
    fn test_batch_netting_efficiency_zero_gross() {
        let batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        assert_eq!(batch.netting_efficiency(), Decimal::ZERO);
    }

    #[test]
    fn test_batch_can_accept_transaction() {
        let batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        assert!(batch.can_accept_transaction());

        // Batch with past cut-off
        let past_batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() - Duration::hours(1),
            "USD".to_string(),
        );

        assert!(!past_batch.can_accept_transaction());
    }

    #[test]
    fn test_serialization() {
        let batch = SettlementBatch::new(
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            Utc::now() + Duration::hours(2),
            "USD".to_string(),
        );

        let json = serde_json::to_string(&batch).unwrap();
        let deserialized: SettlementBatch = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.status, BatchStatus::Pending);
        assert_eq!(deserialized.currency, "USD");
    }
}
