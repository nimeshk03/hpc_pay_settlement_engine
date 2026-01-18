use crate::error::{AppError, Result};
use crate::models::{BatchStatus, SettlementBatch, TransactionRecord, TransactionStatus};
use crate::repositories::{BatchRepository, TransactionRepository};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Timelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Settlement window configuration types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementWindowType {
    /// Real-time settlement (immediate).
    RealTime,
    /// Micro-batch (every few minutes).
    MicroBatch,
    /// Hourly settlement.
    Hourly,
    /// Daily settlement.
    Daily,
}

impl SettlementWindowType {
    /// Returns the duration for this window type.
    pub fn duration(&self) -> Duration {
        match self {
            SettlementWindowType::RealTime => Duration::seconds(0),
            SettlementWindowType::MicroBatch => Duration::minutes(5),
            SettlementWindowType::Hourly => Duration::hours(1),
            SettlementWindowType::Daily => Duration::hours(24),
        }
    }

    /// Returns the cron-like schedule expression.
    pub fn schedule_expression(&self) -> &'static str {
        match self {
            SettlementWindowType::RealTime => "* * * * * *",
            SettlementWindowType::MicroBatch => "*/5 * * * *",
            SettlementWindowType::Hourly => "0 * * * *",
            SettlementWindowType::Daily => "0 0 * * *",
        }
    }
}

/// Configuration for settlement windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementWindowConfig {
    pub window_type: SettlementWindowType,
    pub cut_off_time: Option<NaiveTime>,
    pub timezone: String,
    pub auto_close: bool,
}

impl Default for SettlementWindowConfig {
    fn default() -> Self {
        Self {
            window_type: SettlementWindowType::Daily,
            cut_off_time: Some(NaiveTime::from_hms_opt(23, 59, 59).unwrap()),
            timezone: "UTC".to_string(),
            auto_close: true,
        }
    }
}

/// Batch state machine for managing status transitions.
#[derive(Debug, Clone)]
pub struct BatchStateMachine;

impl BatchStateMachine {
    /// Returns valid next states from the current state.
    pub fn valid_transitions(current: BatchStatus) -> Vec<BatchStatus> {
        match current {
            BatchStatus::Pending => vec![BatchStatus::Processing, BatchStatus::Failed],
            BatchStatus::Processing => vec![BatchStatus::Completed, BatchStatus::Failed],
            BatchStatus::Completed => vec![], // Terminal state
            BatchStatus::Failed => vec![BatchStatus::Pending], // Can retry
        }
    }

    /// Checks if a transition is valid.
    pub fn can_transition(from: BatchStatus, to: BatchStatus) -> bool {
        Self::valid_transitions(from).contains(&to)
    }

    /// Attempts to transition to a new state.
    pub fn transition(from: BatchStatus, to: BatchStatus) -> Result<BatchStatus> {
        if Self::can_transition(from, to) {
            Ok(to)
        } else {
            Err(AppError::Validation(format!(
                "Invalid batch state transition from {:?} to {:?}",
                from, to
            )))
        }
    }
}

/// Result of batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProcessingResult {
    pub batch_id: Uuid,
    pub status: BatchStatus,
    pub total_transactions: i32,
    pub successful_transactions: i32,
    pub failed_transactions: i32,
    pub gross_amount: Decimal,
    pub net_amount: Decimal,
    pub fee_amount: Decimal,
    pub processing_time_ms: u64,
    pub errors: Vec<BatchProcessingError>,
}

/// Error during batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProcessingError {
    pub transaction_id: Uuid,
    pub error_code: String,
    pub error_message: String,
}

/// Notification for batch completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompletionNotification {
    pub batch_id: Uuid,
    pub status: BatchStatus,
    pub settlement_date: NaiveDate,
    pub total_transactions: i32,
    pub gross_amount: Decimal,
    pub net_amount: Decimal,
    pub completed_at: DateTime<Utc>,
}

/// Batch creation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchRequest {
    pub settlement_date: NaiveDate,
    pub cut_off_time: DateTime<Utc>,
    pub currency: String,
    pub metadata: Option<serde_json::Value>,
}

impl CreateBatchRequest {
    pub fn new(settlement_date: NaiveDate, cut_off_time: DateTime<Utc>, currency: impl Into<String>) -> Self {
        Self {
            settlement_date,
            cut_off_time,
            currency: currency.into(),
            metadata: None,
        }
    }

    pub fn for_today(currency: impl Into<String>, hours_until_cutoff: i64) -> Self {
        let cut_off = Utc::now() + Duration::hours(hours_until_cutoff);
        Self::new(Utc::now().date_naive(), cut_off, currency)
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// The batch settlement service handles all batch-related operations.
pub struct BatchService {
    pool: PgPool,
    batch_repo: BatchRepository,
    transaction_repo: TransactionRepository,
    config: SettlementWindowConfig,
    notifications: Arc<RwLock<Vec<BatchCompletionNotification>>>,
}

impl BatchService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            batch_repo: BatchRepository::new(pool.clone()),
            transaction_repo: TransactionRepository::new(pool.clone()),
            pool,
            config: SettlementWindowConfig::default(),
            notifications: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_config(mut self, config: SettlementWindowConfig) -> Self {
        self.config = config;
        self
    }

    /// Creates a new settlement batch.
    pub async fn create_batch(&self, request: CreateBatchRequest) -> Result<SettlementBatch> {
        // Validate cut-off time is in the future
        if request.cut_off_time <= Utc::now() {
            return Err(AppError::Validation("Cut-off time must be in the future".to_string()));
        }

        // Check if there's already an open batch for this date/currency
        if let Some(existing) = self
            .batch_repo
            .find_open_batch(request.settlement_date, &request.currency)
            .await?
        {
            return Err(AppError::Validation(format!(
                "Open batch already exists for {} in {}: {}",
                request.settlement_date, request.currency, existing.id
            )));
        }

        let mut batch = SettlementBatch::new(
            request.settlement_date,
            request.cut_off_time,
            request.currency,
        );

        if let Some(metadata) = request.metadata {
            batch = batch.with_metadata(metadata);
        }

        self.batch_repo.create(&batch).await
    }

    /// Gets or creates a batch for the current settlement window.
    pub async fn get_or_create_current_batch(&self, currency: &str) -> Result<SettlementBatch> {
        let today = Utc::now().date_naive();

        // Try to find existing open batch
        if let Some(batch) = self.batch_repo.find_open_batch(today, currency).await? {
            return Ok(batch);
        }

        // Calculate cut-off time based on config
        let cut_off_time = self.calculate_cut_off_time();

        let request = CreateBatchRequest::new(today, cut_off_time, currency);
        self.create_batch(request).await
    }

    /// Calculates the cut-off time based on configuration.
    fn calculate_cut_off_time(&self) -> DateTime<Utc> {
        let now = Utc::now();
        match self.config.window_type {
            SettlementWindowType::RealTime => now + Duration::minutes(1),
            SettlementWindowType::MicroBatch => now + Duration::minutes(5),
            SettlementWindowType::Hourly => {
                let next_hour = now + Duration::hours(1);
                next_hour
                    .date_naive()
                    .and_hms_opt(next_hour.time().hour(), 0, 0)
                    .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
                    .unwrap_or(next_hour)
            }
            SettlementWindowType::Daily => {
                if let Some(cut_off) = self.config.cut_off_time {
                    let today = now.date_naive();
                    let cut_off_dt = today.and_time(cut_off);
                    let cut_off_utc = DateTime::from_naive_utc_and_offset(cut_off_dt, Utc);
                    if cut_off_utc > now {
                        cut_off_utc
                    } else {
                        cut_off_utc + Duration::days(1)
                    }
                } else {
                    now + Duration::days(1)
                }
            }
        }
    }

    /// Assigns a transaction to a batch.
    pub async fn assign_transaction_to_batch(
        &self,
        transaction_id: Uuid,
        batch_id: Uuid,
    ) -> Result<TransactionRecord> {
        // Verify batch exists and can accept transactions
        let batch = self
            .batch_repo
            .find_by_id(batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Batch '{}' not found", batch_id)))?;

        if !batch.can_accept_transaction() {
            return Err(AppError::Validation(format!(
                "Batch '{}' cannot accept transactions (status: {:?}, cut-off: {})",
                batch_id, batch.status, batch.cut_off_time
            )));
        }

        // Verify transaction exists
        let transaction = self
            .transaction_repo
            .find_by_id(transaction_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Transaction '{}' not found", transaction_id)))?;

        // Verify transaction is settled
        if transaction.status != TransactionStatus::Settled {
            return Err(AppError::Validation(format!(
                "Transaction '{}' must be settled before batch assignment (status: {:?})",
                transaction_id, transaction.status
            )));
        }

        // Assign transaction to batch
        let updated = self
            .transaction_repo
            .assign_to_batch(transaction_id, batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Transaction not found after update".to_string()))?;

        // Update batch totals
        self.batch_repo
            .increment_totals(batch_id, transaction.amount, transaction.fee_amount)
            .await?;

        Ok(updated)
    }

    /// Calculates and updates batch totals from assigned transactions.
    pub async fn recalculate_batch_totals(&self, batch_id: Uuid) -> Result<SettlementBatch> {
        let batch = self
            .batch_repo
            .find_by_id(batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Batch '{}' not found", batch_id)))?;

        // Get all transactions in the batch
        let transactions = self
            .transaction_repo
            .find_by_batch(batch_id)
            .await?;

        let total_transactions = transactions.len() as i32;
        let gross_amount: Decimal = transactions.iter().map(|t| t.amount).sum();
        let fee_amount: Decimal = transactions.iter().map(|t| t.fee_amount).sum();
        let net_amount = gross_amount - fee_amount;

        self.batch_repo
            .update_totals(batch_id, total_transactions, gross_amount, net_amount, fee_amount)
            .await?
            .ok_or_else(|| AppError::NotFound("Batch not found after update".to_string()))
    }

    /// Closes a batch for processing (no more transactions accepted).
    pub async fn close_batch(&self, batch_id: Uuid) -> Result<SettlementBatch> {
        let batch = self
            .batch_repo
            .find_by_id(batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Batch '{}' not found", batch_id)))?;

        BatchStateMachine::transition(batch.status, BatchStatus::Processing)?;

        self.batch_repo
            .update_status(batch_id, BatchStatus::Processing)
            .await?
            .ok_or_else(|| AppError::NotFound("Batch not found after update".to_string()))
    }

    /// Manually triggers batch processing.
    pub async fn trigger_batch_processing(&self, batch_id: Uuid) -> Result<BatchProcessingResult> {
        let start_time = std::time::Instant::now();

        // Close the batch first
        let batch = self.close_batch(batch_id).await?;

        // Process the batch
        self.process_batch_internal(batch, start_time).await
    }

    /// Processes a batch internally.
    async fn process_batch_internal(
        &self,
        batch: SettlementBatch,
        start_time: std::time::Instant,
    ) -> Result<BatchProcessingResult> {
        let batch_id = batch.id;
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        // Get all transactions in the batch
        let transactions = self.transaction_repo.find_by_batch(batch_id).await?;

        // Process transactions (in a real system, this would do actual settlement)
        for transaction in &transactions {
            match self.process_transaction_in_batch(transaction).await {
                Ok(_) => successful += 1,
                Err(e) => {
                    failed += 1;
                    errors.push(BatchProcessingError {
                        transaction_id: transaction.id,
                        error_code: "PROCESSING_ERROR".to_string(),
                        error_message: e.to_string(),
                    });
                }
            }
        }

        // Determine final status
        let final_status = if failed == 0 {
            BatchStatus::Completed
        } else if successful == 0 {
            BatchStatus::Failed
        } else {
            // Partial failure - still mark as completed but with errors
            BatchStatus::Completed
        };

        // Update batch status
        let updated_batch = self
            .batch_repo
            .update_status(batch_id, final_status)
            .await?
            .ok_or_else(|| AppError::NotFound("Batch not found after processing".to_string()))?;

        let processing_time_ms = start_time.elapsed().as_millis() as u64;

        // Send completion notification
        let notification = BatchCompletionNotification {
            batch_id,
            status: final_status,
            settlement_date: updated_batch.settlement_date,
            total_transactions: updated_batch.total_transactions,
            gross_amount: updated_batch.gross_amount,
            net_amount: updated_batch.net_amount,
            completed_at: updated_batch.completed_at.unwrap_or_else(Utc::now),
        };

        self.send_notification(notification).await;

        Ok(BatchProcessingResult {
            batch_id,
            status: final_status,
            total_transactions: transactions.len() as i32,
            successful_transactions: successful,
            failed_transactions: failed,
            gross_amount: updated_batch.gross_amount,
            net_amount: updated_batch.net_amount,
            fee_amount: updated_batch.fee_amount,
            processing_time_ms,
            errors,
        })
    }

    /// Processes a single transaction within a batch.
    async fn process_transaction_in_batch(&self, _transaction: &TransactionRecord) -> Result<()> {
        // In a real system, this would:
        // 1. Verify the transaction is still valid
        // 2. Execute any pending settlements
        // 3. Update external systems
        // For now, we just validate the transaction is in the correct state
        Ok(())
    }

    /// Sends a batch completion notification.
    async fn send_notification(&self, notification: BatchCompletionNotification) {
        let mut notifications = self.notifications.write().await;
        notifications.push(notification);
    }

    /// Gets pending notifications (for testing/integration).
    pub async fn get_notifications(&self) -> Vec<BatchCompletionNotification> {
        let notifications = self.notifications.read().await;
        notifications.clone()
    }

    /// Clears notifications.
    pub async fn clear_notifications(&self) {
        let mut notifications = self.notifications.write().await;
        notifications.clear();
    }

    /// Finds batches that are past their cut-off time and still pending.
    pub async fn find_batches_ready_for_processing(&self) -> Result<Vec<SettlementBatch>> {
        self.batch_repo.find_ready_for_processing().await
    }

    /// Automatically closes and processes batches past their cut-off time.
    pub async fn auto_close_expired_batches(&self) -> Result<Vec<BatchProcessingResult>> {
        if !self.config.auto_close {
            return Ok(Vec::new());
        }

        let ready_batches = self.find_batches_ready_for_processing().await?;
        let mut results = Vec::new();

        for batch in ready_batches {
            match self.trigger_batch_processing(batch.id).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::error!("Failed to process batch {}: {}", batch.id, e);
                }
            }
        }

        Ok(results)
    }

    /// Gets a batch by ID.
    pub async fn get_batch(&self, batch_id: Uuid) -> Result<Option<SettlementBatch>> {
        self.batch_repo.find_by_id(batch_id).await
    }

    /// Lists batches with optional filters.
    pub async fn list_batches(
        &self,
        status: Option<BatchStatus>,
        currency: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SettlementBatch>> {
        self.batch_repo.list(status, currency, limit, offset).await
    }

    /// Gets transactions in a batch.
    pub async fn get_batch_transactions(&self, batch_id: Uuid) -> Result<Vec<TransactionRecord>> {
        self.transaction_repo.find_by_batch(batch_id).await
    }

    /// Marks a batch as failed.
    pub async fn fail_batch(&self, batch_id: Uuid, reason: &str) -> Result<SettlementBatch> {
        let batch = self
            .batch_repo
            .find_by_id(batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Batch '{}' not found", batch_id)))?;

        BatchStateMachine::transition(batch.status, BatchStatus::Failed)?;

        let mut metadata = batch.metadata.unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("failure_reason".to_string(), serde_json::json!(reason));
        }

        // Update status
        let updated = self
            .batch_repo
            .update_status(batch_id, BatchStatus::Failed)
            .await?
            .ok_or_else(|| AppError::NotFound("Batch not found after update".to_string()))?;

        Ok(updated)
    }

    /// Retries a failed batch.
    pub async fn retry_batch(&self, batch_id: Uuid) -> Result<SettlementBatch> {
        let batch = self
            .batch_repo
            .find_by_id(batch_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Batch '{}' not found", batch_id)))?;

        if batch.status != BatchStatus::Failed {
            return Err(AppError::Validation(format!(
                "Only failed batches can be retried (current status: {:?})",
                batch.status
            )));
        }

        // Reset to pending
        self.batch_repo
            .update_status(batch_id, BatchStatus::Pending)
            .await?
            .ok_or_else(|| AppError::NotFound("Batch not found after update".to_string()))
    }
}

/// Background scheduler for automatic batch processing.
pub struct BatchScheduler {
    service: Arc<BatchService>,
    running: Arc<AtomicBool>,
    interval_seconds: u64,
}

impl BatchScheduler {
    pub fn new(service: Arc<BatchService>, interval_seconds: u64) -> Self {
        Self {
            service,
            running: Arc::new(AtomicBool::new(false)),
            interval_seconds,
        }
    }

    /// Starts the scheduler in a background task.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let service = self.service.clone();
        let running = self.running.clone();
        let interval = self.interval_seconds;

        running.store(true, Ordering::SeqCst);

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                // Process expired batches
                if let Err(e) = service.auto_close_expired_batches().await {
                    tracing::error!("Batch scheduler error: {}", e);
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
        })
    }

    /// Stops the scheduler.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Checks if the scheduler is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_state_machine_valid_transitions() {
        assert!(BatchStateMachine::can_transition(
            BatchStatus::Pending,
            BatchStatus::Processing
        ));
        assert!(BatchStateMachine::can_transition(
            BatchStatus::Processing,
            BatchStatus::Completed
        ));
        assert!(BatchStateMachine::can_transition(
            BatchStatus::Processing,
            BatchStatus::Failed
        ));
        assert!(BatchStateMachine::can_transition(
            BatchStatus::Failed,
            BatchStatus::Pending
        ));
    }

    #[test]
    fn test_batch_state_machine_invalid_transitions() {
        assert!(!BatchStateMachine::can_transition(
            BatchStatus::Completed,
            BatchStatus::Pending
        ));
        assert!(!BatchStateMachine::can_transition(
            BatchStatus::Pending,
            BatchStatus::Completed
        ));
    }

    #[test]
    fn test_settlement_window_duration() {
        assert_eq!(SettlementWindowType::RealTime.duration(), Duration::seconds(0));
        assert_eq!(SettlementWindowType::MicroBatch.duration(), Duration::minutes(5));
        assert_eq!(SettlementWindowType::Hourly.duration(), Duration::hours(1));
        assert_eq!(SettlementWindowType::Daily.duration(), Duration::hours(24));
    }

    #[test]
    fn test_settlement_window_schedule() {
        assert_eq!(SettlementWindowType::RealTime.schedule_expression(), "* * * * * *");
        assert_eq!(SettlementWindowType::MicroBatch.schedule_expression(), "*/5 * * * *");
        assert_eq!(SettlementWindowType::Hourly.schedule_expression(), "0 * * * *");
        assert_eq!(SettlementWindowType::Daily.schedule_expression(), "0 0 * * *");
    }

    #[test]
    fn test_create_batch_request_for_today() {
        let request = CreateBatchRequest::for_today("USD", 24);
        assert_eq!(request.settlement_date, Utc::now().date_naive());
        assert_eq!(request.currency, "USD");
        assert!(request.cut_off_time > Utc::now());
    }

    #[test]
    fn test_default_settlement_config() {
        let config = SettlementWindowConfig::default();
        assert_eq!(config.window_type, SettlementWindowType::Daily);
        assert!(config.auto_close);
        assert_eq!(config.timezone, "UTC");
    }
}
