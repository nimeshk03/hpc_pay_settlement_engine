use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, LedgerEntry, TransactionRecord, TransactionStatus, TransactionType,
};
use crate::repositories::{AccountRepository, BalanceRepository, LedgerRepository, TransactionRepository};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Validation error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub code: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code: code.into(),
        }
    }
}

/// Result of transaction validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
        }
    }

    pub fn add_error(&mut self, error: ValidationError) {
        self.is_valid = false;
        self.errors.push(error);
    }
}

/// Transaction state machine for managing status transitions.
#[derive(Debug, Clone)]
pub struct TransactionStateMachine;

impl TransactionStateMachine {
    /// Returns valid next states from the current state.
    pub fn valid_transitions(current: TransactionStatus) -> Vec<TransactionStatus> {
        match current {
            TransactionStatus::Pending => vec![
                TransactionStatus::Settled,
                TransactionStatus::Failed,
            ],
            TransactionStatus::Settled => vec![
                TransactionStatus::Reversed,
            ],
            TransactionStatus::Failed => vec![], // Terminal state
            TransactionStatus::Reversed => vec![], // Terminal state
        }
    }

    /// Checks if a transition is valid.
    pub fn can_transition(from: TransactionStatus, to: TransactionStatus) -> bool {
        Self::valid_transitions(from).contains(&to)
    }

    /// Attempts to transition to a new state.
    pub fn transition(from: TransactionStatus, to: TransactionStatus) -> Result<TransactionStatus> {
        if Self::can_transition(from, to) {
            Ok(to)
        } else {
            Err(AppError::Validation(format!(
                "Invalid state transition from {:?} to {:?}",
                from, to
            )))
        }
    }
}

/// Request for processing a ledger transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTransactionRequest {
    pub external_id: String,
    pub transaction_type: TransactionType,
    pub source_account_id: Uuid,
    pub destination_account_id: Uuid,
    pub amount: Decimal,
    pub currency: String,
    pub fee_amount: Decimal,
    pub idempotency_key: String,
    pub effective_date: Option<NaiveDate>,
    pub metadata: Option<serde_json::Value>,
    pub original_transaction_id: Option<Uuid>,
}

impl LedgerTransactionRequest {
    pub fn payment(
        external_id: impl Into<String>,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            transaction_type: TransactionType::Payment,
            source_account_id,
            destination_account_id,
            amount,
            currency: currency.into(),
            fee_amount: Decimal::ZERO,
            idempotency_key: idempotency_key.into(),
            effective_date: None,
            metadata: None,
            original_transaction_id: None,
        }
    }

    pub fn transfer(
        external_id: impl Into<String>,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            transaction_type: TransactionType::Transfer,
            source_account_id,
            destination_account_id,
            amount,
            currency: currency.into(),
            fee_amount: Decimal::ZERO,
            idempotency_key: idempotency_key.into(),
            effective_date: None,
            metadata: None,
            original_transaction_id: None,
        }
    }

    pub fn fee(
        external_id: impl Into<String>,
        source_account_id: Uuid,
        fee_account_id: Uuid,
        amount: Decimal,
        currency: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            transaction_type: TransactionType::Fee,
            source_account_id,
            destination_account_id: fee_account_id,
            amount,
            currency: currency.into(),
            fee_amount: Decimal::ZERO,
            idempotency_key: idempotency_key.into(),
            effective_date: None,
            metadata: None,
            original_transaction_id: None,
        }
    }

    pub fn refund(
        external_id: impl Into<String>,
        original_transaction_id: Uuid,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            transaction_type: TransactionType::Refund,
            source_account_id,
            destination_account_id,
            amount,
            currency: currency.into(),
            fee_amount: Decimal::ZERO,
            idempotency_key: idempotency_key.into(),
            effective_date: None,
            metadata: None,
            original_transaction_id: Some(original_transaction_id),
        }
    }

    pub fn chargeback(
        external_id: impl Into<String>,
        original_transaction_id: Uuid,
        source_account_id: Uuid,
        destination_account_id: Uuid,
        amount: Decimal,
        currency: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            external_id: external_id.into(),
            transaction_type: TransactionType::Chargeback,
            source_account_id,
            destination_account_id,
            amount,
            currency: currency.into(),
            fee_amount: Decimal::ZERO,
            idempotency_key: idempotency_key.into(),
            effective_date: None,
            metadata: None,
            original_transaction_id: Some(original_transaction_id),
        }
    }

    pub fn with_fee(mut self, fee_amount: Decimal) -> Self {
        self.fee_amount = fee_amount;
        self
    }

    pub fn with_effective_date(mut self, date: NaiveDate) -> Self {
        self.effective_date = Some(date);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn net_amount(&self) -> Decimal {
        self.amount - self.fee_amount
    }
}

/// Result of a ledger transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTransactionResult {
    pub transaction: TransactionRecord,
    pub entries: Vec<LedgerEntry>,
    pub source_balance: AccountBalance,
    pub destination_balance: AccountBalance,
}

/// The ledger service handles all ledger operations including transaction processing,
/// validation, and ledger entry creation with ACID compliance.
pub struct LedgerService {
    pool: PgPool,
    account_repo: AccountRepository,
    balance_repo: BalanceRepository,
    ledger_repo: LedgerRepository,
    transaction_repo: TransactionRepository,
}

impl LedgerService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            account_repo: AccountRepository::new(pool.clone()),
            balance_repo: BalanceRepository::new(pool.clone()),
            ledger_repo: LedgerRepository::new(pool.clone()),
            transaction_repo: TransactionRepository::new(pool.clone()),
            pool,
        }
    }

    /// Validates a transaction request through the validation pipeline.
    pub async fn validate_transaction(&self, request: &LedgerTransactionRequest) -> Result<ValidationResult> {
        let mut result = ValidationResult::valid();

        // Basic field validation
        if request.external_id.is_empty() {
            result.add_error(ValidationError::new(
                "external_id",
                "External ID is required",
                "REQUIRED_FIELD",
            ));
        }

        if request.amount <= Decimal::ZERO {
            result.add_error(ValidationError::new(
                "amount",
                "Amount must be positive",
                "INVALID_AMOUNT",
            ));
        }

        if request.fee_amount < Decimal::ZERO {
            result.add_error(ValidationError::new(
                "fee_amount",
                "Fee amount cannot be negative",
                "INVALID_FEE",
            ));
        }

        if request.fee_amount > request.amount {
            result.add_error(ValidationError::new(
                "fee_amount",
                "Fee cannot exceed transaction amount",
                "FEE_EXCEEDS_AMOUNT",
            ));
        }

        if request.source_account_id == request.destination_account_id {
            result.add_error(ValidationError::new(
                "destination_account_id",
                "Source and destination accounts must be different",
                "SAME_ACCOUNT",
            ));
        }

        if request.currency.len() != 3 {
            result.add_error(ValidationError::new(
                "currency",
                "Currency must be a 3-letter ISO code",
                "INVALID_CURRENCY",
            ));
        }

        if request.idempotency_key.is_empty() {
            result.add_error(ValidationError::new(
                "idempotency_key",
                "Idempotency key is required",
                "REQUIRED_FIELD",
            ));
        }

        // Transaction type specific validation
        match request.transaction_type {
            TransactionType::Refund | TransactionType::Chargeback => {
                if request.original_transaction_id.is_none() {
                    result.add_error(ValidationError::new(
                        "original_transaction_id",
                        "Original transaction ID is required for refunds and chargebacks",
                        "REQUIRED_FIELD",
                    ));
                }
            }
            _ => {}
        }

        Ok(result)
    }

    /// Verifies that an account exists and is operational.
    pub async fn verify_account(&self, account_id: Uuid) -> Result<Account> {
        let account = self
            .account_repo
            .find_by_id(account_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account '{}' not found", account_id)))?;

        if !account.status.is_operational() {
            return Err(AppError::Validation(format!(
                "Account '{}' is not operational (status: {:?})",
                account_id, account.status
            )));
        }

        Ok(account)
    }

    /// Checks if an account has sufficient funds for a transaction.
    pub async fn check_sufficient_funds(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let balance = self
            .balance_repo
            .find_by_account_and_currency(account_id, currency)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Balance not found for account '{}' in currency '{}'",
                    account_id, currency
                ))
            })?;

        if !balance.has_sufficient_funds(amount) {
            return Err(AppError::Validation(format!(
                "Insufficient funds: requested {}, available {}",
                amount,
                balance.usable_balance()
            )));
        }

        Ok(balance)
    }

    /// Processes a payment transaction.
    pub async fn process_payment(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        if request.transaction_type != TransactionType::Payment {
            return Err(AppError::Validation("Invalid transaction type for payment".to_string()));
        }
        self.execute_transaction(request).await
    }

    /// Processes a transfer transaction.
    pub async fn process_transfer(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        if request.transaction_type != TransactionType::Transfer {
            return Err(AppError::Validation("Invalid transaction type for transfer".to_string()));
        }
        self.execute_transaction(request).await
    }

    /// Processes a fee transaction.
    pub async fn process_fee(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        if request.transaction_type != TransactionType::Fee {
            return Err(AppError::Validation("Invalid transaction type for fee".to_string()));
        }
        self.execute_transaction(request).await
    }

    /// Processes a refund transaction.
    pub async fn process_refund(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        if request.transaction_type != TransactionType::Refund {
            return Err(AppError::Validation("Invalid transaction type for refund".to_string()));
        }

        // Verify original transaction
        let original_id = request.original_transaction_id.ok_or_else(|| {
            AppError::Validation("Original transaction ID is required for refund".to_string())
        })?;

        let original = self
            .transaction_repo
            .find_by_id(original_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Original transaction '{}' not found", original_id)))?;

        // Verify original transaction can be refunded
        if original.status != TransactionStatus::Settled {
            return Err(AppError::Validation(format!(
                "Cannot refund transaction with status {:?}",
                original.status
            )));
        }

        if !original.transaction_type.is_reversible() {
            return Err(AppError::Validation(format!(
                "Transaction type {:?} cannot be refunded",
                original.transaction_type
            )));
        }

        // Verify refund amount doesn't exceed original
        if request.amount > original.amount {
            return Err(AppError::Validation(format!(
                "Refund amount {} exceeds original transaction amount {}",
                request.amount, original.amount
            )));
        }

        self.execute_transaction(request).await
    }

    /// Processes a chargeback transaction.
    pub async fn process_chargeback(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        if request.transaction_type != TransactionType::Chargeback {
            return Err(AppError::Validation("Invalid transaction type for chargeback".to_string()));
        }

        // Verify original transaction
        let original_id = request.original_transaction_id.ok_or_else(|| {
            AppError::Validation("Original transaction ID is required for chargeback".to_string())
        })?;

        let original = self
            .transaction_repo
            .find_by_id(original_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Original transaction '{}' not found", original_id)))?;

        // Verify original transaction can be charged back
        if original.status != TransactionStatus::Settled {
            return Err(AppError::Validation(format!(
                "Cannot chargeback transaction with status {:?}",
                original.status
            )));
        }

        self.execute_transaction(request).await
    }

    /// Executes a transaction with full validation and ACID compliance.
    pub async fn execute_transaction(&self, request: LedgerTransactionRequest) -> Result<LedgerTransactionResult> {
        // Run validation pipeline
        let validation = self.validate_transaction(&request).await?;
        if !validation.is_valid {
            let error_messages: Vec<String> = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect();
            return Err(AppError::Validation(error_messages.join("; ")));
        }

        // Check idempotency
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&request.idempotency_key)
            .await?
        {
            return self.build_result_from_existing(existing).await;
        }

        // Verify accounts
        let _source_account = self.verify_account(request.source_account_id).await?;
        let _dest_account = self.verify_account(request.destination_account_id).await?;

        // Get or create balances
        let _source_balance = self
            .balance_repo
            .get_or_create(request.source_account_id, &request.currency)
            .await?;

        let _dest_balance = self
            .balance_repo
            .get_or_create(request.destination_account_id, &request.currency)
            .await?;

        // Check sufficient funds (except for refunds/chargebacks where destination pays back)
        match request.transaction_type {
            TransactionType::Refund | TransactionType::Chargeback => {
                // For refunds/chargebacks, the destination (original receiver) pays back
                self.check_sufficient_funds(
                    request.source_account_id,
                    &request.currency,
                    request.amount,
                )
                .await?;
            }
            _ => {
                self.check_sufficient_funds(
                    request.source_account_id,
                    &request.currency,
                    request.amount,
                )
                .await?;
            }
        }

        // Extract values before moving
        let net_amount = request.net_amount();
        let effective_date = request.effective_date.unwrap_or_else(|| Utc::now().date_naive());
        let source_account_id = request.source_account_id;
        let destination_account_id = request.destination_account_id;
        let amount = request.amount;
        let currency = request.currency.clone();

        // Execute atomically with SERIALIZABLE isolation
        let mut tx: sqlx::Transaction<'_, sqlx::Postgres> = self.pool.begin().await.map_err(AppError::Database)?;

        // Set transaction isolation level
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;

        // Create transaction record
        let mut transaction = TransactionRecord::new(
            request.external_id,
            request.transaction_type,
            source_account_id,
            destination_account_id,
            amount,
            currency.clone(),
            request.fee_amount,
            request.idempotency_key,
        );

        if let Some(metadata) = request.metadata {
            transaction = transaction.with_metadata(metadata);
        }

        let transaction = sqlx::query_as::<_, TransactionRecord>(
            r#"
            INSERT INTO transactions (id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            RETURNING id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            "#,
        )
        .bind(transaction.id)
        .bind(&transaction.external_id)
        .bind(&transaction.transaction_type)
        .bind(&transaction.status)
        .bind(transaction.source_account_id)
        .bind(transaction.destination_account_id)
        .bind(transaction.amount)
        .bind(&transaction.currency)
        .bind(transaction.fee_amount)
        .bind(transaction.net_amount)
        .bind(transaction.settlement_batch_id)
        .bind(&transaction.idempotency_key)
        .bind(&transaction.metadata)
        .bind(transaction.created_at)
        .bind(transaction.settled_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Update balances atomically
        let updated_source = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance - $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
              AND available_balance - reserved_balance >= $3
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(source_account_id)
        .bind(&currency)
        .bind(amount)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Validation("Insufficient funds during transaction".to_string()))?;

        let updated_dest = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance + $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(destination_account_id)
        .bind(&currency)
        .bind(net_amount)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Create ledger entries with balance_after
        let debit_entry = LedgerEntry::debit(
            transaction.id,
            source_account_id,
            amount,
            currency.clone(),
            updated_source.available_balance,
            effective_date,
        );

        let credit_entry = LedgerEntry::credit(
            transaction.id,
            destination_account_id,
            net_amount,
            currency.clone(),
            updated_dest.available_balance,
            effective_date,
        );

        // Insert debit entry
        let debit_entry = sqlx::query_as::<_, LedgerEntry>(
            r#"
            INSERT INTO ledger_entries (id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            "#,
        )
        .bind(debit_entry.id)
        .bind(debit_entry.transaction_id)
        .bind(debit_entry.account_id)
        .bind(&debit_entry.entry_type)
        .bind(debit_entry.amount)
        .bind(&debit_entry.currency)
        .bind(debit_entry.balance_after)
        .bind(debit_entry.effective_date)
        .bind(&debit_entry.metadata)
        .bind(debit_entry.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Insert credit entry
        let credit_entry = sqlx::query_as::<_, LedgerEntry>(
            r#"
            INSERT INTO ledger_entries (id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            "#,
        )
        .bind(credit_entry.id)
        .bind(credit_entry.transaction_id)
        .bind(credit_entry.account_id)
        .bind(&credit_entry.entry_type)
        .bind(credit_entry.amount)
        .bind(&credit_entry.currency)
        .bind(credit_entry.balance_after)
        .bind(credit_entry.effective_date)
        .bind(&credit_entry.metadata)
        .bind(credit_entry.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Update transaction status to settled
        let transaction = sqlx::query_as::<_, TransactionRecord>(
            r#"
            UPDATE transactions
            SET status = 'SETTLED', settled_at = NOW()
            WHERE id = $1
            RETURNING id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            "#,
        )
        .bind(transaction.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Commit transaction
        tx.commit().await.map_err(AppError::Database)?;

        Ok(LedgerTransactionResult {
            transaction,
            entries: vec![debit_entry, credit_entry],
            source_balance: updated_source,
            destination_balance: updated_dest,
        })
    }

    /// Builds a result from an existing transaction (for idempotency).
    async fn build_result_from_existing(&self, transaction: TransactionRecord) -> Result<LedgerTransactionResult> {
        let entries = self.ledger_repo.find_by_transaction(transaction.id).await?;

        let source_balance = self
            .balance_repo
            .find_by_account_and_currency(transaction.source_account_id, &transaction.currency)
            .await?
            .ok_or_else(|| AppError::NotFound("Source balance not found".to_string()))?;

        let dest_balance = self
            .balance_repo
            .find_by_account_and_currency(transaction.destination_account_id, &transaction.currency)
            .await?
            .ok_or_else(|| AppError::NotFound("Destination balance not found".to_string()))?;

        Ok(LedgerTransactionResult {
            transaction,
            entries,
            source_balance,
            destination_balance: dest_balance,
        })
    }

    /// Gets the transaction history for an account.
    pub async fn get_account_history(
        &self,
        account_id: Uuid,
        limit: i64,
    ) -> Result<Vec<LedgerEntry>> {
        self.ledger_repo.find_by_account(account_id, limit, 0).await
    }

    /// Verifies that a transaction's ledger entries are balanced.
    pub async fn verify_transaction_balance(&self, transaction_id: Uuid) -> Result<bool> {
        self.ledger_repo.verify_transaction_balance(transaction_id).await
    }

    /// Gets the running balance for an account at a specific point in time.
    pub async fn get_balance_at_entry(&self, entry_id: Uuid) -> Result<Option<Decimal>> {
        let entry = self
            .ledger_repo
            .find_by_id(entry_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Ledger entry '{}' not found", entry_id)))?;

        Ok(Some(entry.balance_after))
    }

    /// Updates transaction status with state machine validation.
    pub async fn update_transaction_status(
        &self,
        transaction_id: Uuid,
        new_status: TransactionStatus,
    ) -> Result<TransactionRecord> {
        let transaction = self
            .transaction_repo
            .find_by_id(transaction_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Transaction '{}' not found", transaction_id)))?;

        // Validate state transition
        TransactionStateMachine::transition(transaction.status, new_status)?;

        self.transaction_repo
            .update_status(transaction_id, new_status)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Transaction '{}' not found after update", transaction_id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_machine_valid_transitions() {
        assert!(TransactionStateMachine::can_transition(
            TransactionStatus::Pending,
            TransactionStatus::Settled
        ));
        assert!(TransactionStateMachine::can_transition(
            TransactionStatus::Pending,
            TransactionStatus::Failed
        ));
        assert!(TransactionStateMachine::can_transition(
            TransactionStatus::Settled,
            TransactionStatus::Reversed
        ));
    }

    #[test]
    fn test_state_machine_invalid_transitions() {
        assert!(!TransactionStateMachine::can_transition(
            TransactionStatus::Failed,
            TransactionStatus::Settled
        ));
        assert!(!TransactionStateMachine::can_transition(
            TransactionStatus::Reversed,
            TransactionStatus::Pending
        ));
        assert!(!TransactionStateMachine::can_transition(
            TransactionStatus::Pending,
            TransactionStatus::Reversed
        ));
    }

    #[test]
    fn test_validation_result() {
        let mut result = ValidationResult::valid();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());

        result.add_error(ValidationError::new("field", "message", "CODE"));
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_payment_request_builder() {
        let request = LedgerTransactionRequest::payment(
            "EXT-001",
            Uuid::new_v4(),
            Uuid::new_v4(),
            Decimal::new(10000, 2),
            "USD",
            "IDEM-001",
        )
        .with_fee(Decimal::new(100, 2));

        assert_eq!(request.transaction_type, TransactionType::Payment);
        assert_eq!(request.amount, Decimal::new(10000, 2));
        assert_eq!(request.fee_amount, Decimal::new(100, 2));
        assert_eq!(request.net_amount(), Decimal::new(9900, 2));
    }

    #[test]
    fn test_refund_request_builder() {
        let original_id = Uuid::new_v4();
        let request = LedgerTransactionRequest::refund(
            "REF-001",
            original_id,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Decimal::new(5000, 2),
            "USD",
            "IDEM-REF-001",
        );

        assert_eq!(request.transaction_type, TransactionType::Refund);
        assert_eq!(request.original_transaction_id, Some(original_id));
    }

    #[test]
    fn test_chargeback_request_builder() {
        let original_id = Uuid::new_v4();
        let request = LedgerTransactionRequest::chargeback(
            "CB-001",
            original_id,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Decimal::new(5000, 2),
            "USD",
            "IDEM-CB-001",
        );

        assert_eq!(request.transaction_type, TransactionType::Chargeback);
        assert_eq!(request.original_transaction_id, Some(original_id));
    }
}
