use crate::error::{AppError, Result};
use crate::models::{AccountBalance, AccountType, EntryType, LedgerEntry, TransactionRecord, TransactionStatus, TransactionType};
use crate::repositories::{AccountRepository, BalanceRepository, LedgerRepository, TransactionRepository};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Result of a double-entry transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub transaction: TransactionRecord,
    pub debit_entry: LedgerEntry,
    pub credit_entry: LedgerEntry,
    pub source_balance: AccountBalance,
    pub destination_balance: AccountBalance,
}

/// Request to execute a double-entry transaction.
#[derive(Debug, Clone)]
pub struct TransactionRequest {
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
}

/// Request to reverse a transaction.
#[derive(Debug, Clone)]
pub struct ReversalRequest {
    pub original_transaction_id: Uuid,
    pub external_id: String,
    pub idempotency_key: String,
    pub reason: Option<String>,
}

/// The core double-entry bookkeeping engine.
/// Ensures that every transaction creates balanced debit and credit entries.
pub struct DoubleEntryEngine {
    pool: PgPool,
    account_repo: AccountRepository,
    balance_repo: BalanceRepository,
    ledger_repo: LedgerRepository,
    transaction_repo: TransactionRepository,
}

impl DoubleEntryEngine {
    pub fn new(pool: PgPool) -> Self {
        Self {
            account_repo: AccountRepository::new(pool.clone()),
            balance_repo: BalanceRepository::new(pool.clone()),
            ledger_repo: LedgerRepository::new(pool.clone()),
            transaction_repo: TransactionRepository::new(pool.clone()),
            pool,
        }
    }

    /// Executes a double-entry transaction atomically.
    /// Creates debit entry for source, credit entry for destination.
    pub async fn execute_transaction(
        &self,
        request: TransactionRequest,
    ) -> Result<TransactionResult> {
        // Validate request
        self.validate_transaction_request(&request)?;

        // Check idempotency - return existing if found
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&request.idempotency_key)
            .await?
        {
            return self.build_existing_result(existing).await;
        }

        // Validate accounts exist and are operational
        let source_account = self
            .account_repo
            .find_by_id(request.source_account_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Source account '{}' not found",
                    request.source_account_id
                ))
            })?;

        let dest_account = self
            .account_repo
            .find_by_id(request.destination_account_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Destination account '{}' not found",
                    request.destination_account_id
                ))
            })?;

        if !source_account.status.is_operational() {
            return Err(AppError::Validation(format!(
                "Source account '{}' is not operational",
                request.source_account_id
            )));
        }

        if !dest_account.status.is_operational() {
            return Err(AppError::Validation(format!(
                "Destination account '{}' is not operational",
                request.destination_account_id
            )));
        }

        // Get or create balances
        let source_balance = self
            .balance_repo
            .get_or_create(request.source_account_id, &request.currency)
            .await?;

        let _dest_balance = self
            .balance_repo
            .get_or_create(request.destination_account_id, &request.currency)
            .await?;

        // Check sufficient funds for source account
        if !source_balance.has_sufficient_funds(request.amount) {
            return Err(AppError::Validation(format!(
                "Insufficient funds: requested {}, available {}",
                request.amount,
                source_balance.usable_balance()
            )));
        }

        // Extract values before moving request fields
        let net_amount = request.net_amount();
        let effective_date = request.effective_date.unwrap_or_else(|| Utc::now().date_naive());
        let source_account_id = request.source_account_id;
        let destination_account_id = request.destination_account_id;
        let amount = request.amount;
        let currency = request.currency.clone();

        // Execute the transaction atomically
        let mut tx = self.pool.begin().await.map_err(AppError::Database)?;

        // Create the transaction record
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

        // Debit source account
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

        // Credit destination account
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

        // Create ledger entries
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

        // Commit the transaction
        tx.commit().await.map_err(AppError::Database)?;

        Ok(TransactionResult {
            transaction,
            debit_entry,
            credit_entry,
            source_balance: updated_source,
            destination_balance: updated_dest,
        })
    }

    /// Reverses a previously settled transaction.
    pub async fn reverse_transaction(
        &self,
        request: ReversalRequest,
    ) -> Result<TransactionResult> {
        // Check idempotency
        if let Some(existing) = self
            .transaction_repo
            .find_by_idempotency_key(&request.idempotency_key)
            .await?
        {
            return self.build_existing_result(existing).await;
        }

        // Find original transaction
        let original = self
            .transaction_repo
            .find_by_id(request.original_transaction_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Original transaction '{}' not found",
                    request.original_transaction_id
                ))
            })?;

        // Validate original can be reversed
        if original.status != TransactionStatus::Settled {
            return Err(AppError::Validation(format!(
                "Cannot reverse transaction with status {:?}",
                original.status
            )));
        }

        if !original.transaction_type.is_reversible() {
            return Err(AppError::Validation(format!(
                "Transaction type {:?} cannot be reversed",
                original.transaction_type
            )));
        }

        // Create reversal (swap source and destination)
        let reversal_type = original
            .transaction_type
            .reversal_type()
            .unwrap_or(TransactionType::Refund);

        let mut metadata = serde_json::json!({
            "original_transaction_id": original.id.to_string(),
        });

        if let Some(reason) = request.reason {
            metadata["reversal_reason"] = serde_json::Value::String(reason);
        }

        let reversal_request = TransactionRequest {
            external_id: request.external_id,
            transaction_type: reversal_type,
            source_account_id: original.destination_account_id, // Swap
            destination_account_id: original.source_account_id, // Swap
            amount: original.net_amount, // Reverse the net amount
            currency: original.currency.clone(),
            fee_amount: Decimal::ZERO, // No fee on reversal
            idempotency_key: request.idempotency_key,
            effective_date: None,
            metadata: Some(metadata),
        };

        // Execute the reversal
        let result = self.execute_transaction(reversal_request).await?;

        // Mark original as reversed
        self.transaction_repo
            .update_status(original.id, TransactionStatus::Reversed)
            .await?;

        Ok(result)
    }

    /// Verifies that a transaction's ledger entries are balanced.
    pub async fn verify_transaction_balance(&self, transaction_id: Uuid) -> Result<bool> {
        self.ledger_repo.verify_transaction_balance(transaction_id).await
    }

    /// Gets all ledger entries for a transaction.
    pub async fn get_transaction_entries(&self, transaction_id: Uuid) -> Result<Vec<LedgerEntry>> {
        self.ledger_repo.find_by_transaction(transaction_id).await
    }

    /// Calculates the effect of an entry on an account based on account type.
    pub fn calculate_balance_effect(
        account_type: AccountType,
        entry_type: EntryType,
        amount: Decimal,
    ) -> Decimal {
        let is_debit_normal = account_type.is_debit_normal();

        match (is_debit_normal, entry_type) {
            (true, EntryType::Debit) => amount,   // Asset/Expense: Debit increases
            (true, EntryType::Credit) => -amount, // Asset/Expense: Credit decreases
            (false, EntryType::Debit) => -amount, // Liability/Revenue: Debit decreases
            (false, EntryType::Credit) => amount, // Liability/Revenue: Credit increases
        }
    }

    fn validate_transaction_request(&self, request: &TransactionRequest) -> Result<()> {
        if request.amount <= Decimal::ZERO {
            return Err(AppError::Validation("Amount must be positive".to_string()));
        }

        if request.fee_amount < Decimal::ZERO {
            return Err(AppError::Validation("Fee amount cannot be negative".to_string()));
        }

        if request.fee_amount >= request.amount {
            return Err(AppError::Validation(
                "Fee amount cannot be greater than or equal to transaction amount".to_string(),
            ));
        }

        if request.source_account_id == request.destination_account_id {
            return Err(AppError::Validation(
                "Source and destination accounts must be different".to_string(),
            ));
        }

        if request.external_id.trim().is_empty() {
            return Err(AppError::Validation("External ID cannot be empty".to_string()));
        }

        if request.idempotency_key.trim().is_empty() {
            return Err(AppError::Validation("Idempotency key cannot be empty".to_string()));
        }

        if request.currency.len() != 3 {
            return Err(AppError::Validation(
                "Currency must be a 3-letter ISO 4217 code".to_string(),
            ));
        }

        Ok(())
    }

    async fn build_existing_result(&self, transaction: TransactionRecord) -> Result<TransactionResult> {
        let entries = self.ledger_repo.find_by_transaction(transaction.id).await?;

        let debit_entry = entries
            .iter()
            .find(|e| e.entry_type == EntryType::Debit)
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Debit entry not found")))?;

        let credit_entry = entries
            .iter()
            .find(|e| e.entry_type == EntryType::Credit)
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Credit entry not found")))?;

        let source_balance = self
            .balance_repo
            .find_by_account_and_currency(transaction.source_account_id, &transaction.currency)
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Source balance not found")))?;

        let destination_balance = self
            .balance_repo
            .find_by_account_and_currency(transaction.destination_account_id, &transaction.currency)
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Destination balance not found")))?;

        Ok(TransactionResult {
            transaction,
            debit_entry,
            credit_entry,
            source_balance,
            destination_balance,
        })
    }
}

impl TransactionRequest {
    /// Calculates the net amount (amount - fee).
    pub fn net_amount(&self) -> Decimal {
        self.amount - self.fee_amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_request_net_amount() {
        let request = TransactionRequest {
            external_id: "EXT-001".to_string(),
            transaction_type: TransactionType::Payment,
            source_account_id: Uuid::new_v4(),
            destination_account_id: Uuid::new_v4(),
            amount: Decimal::from(100),
            currency: "USD".to_string(),
            fee_amount: Decimal::from(5),
            idempotency_key: "IDEM-001".to_string(),
            effective_date: None,
            metadata: None,
        };

        assert_eq!(request.net_amount(), Decimal::from(95));
    }

    #[test]
    fn test_calculate_balance_effect_asset_debit() {
        let effect = DoubleEntryEngine::calculate_balance_effect(
            AccountType::Asset,
            EntryType::Debit,
            Decimal::from(100),
        );
        assert_eq!(effect, Decimal::from(100));
    }

    #[test]
    fn test_calculate_balance_effect_asset_credit() {
        let effect = DoubleEntryEngine::calculate_balance_effect(
            AccountType::Asset,
            EntryType::Credit,
            Decimal::from(100),
        );
        assert_eq!(effect, Decimal::from(-100));
    }

    #[test]
    fn test_calculate_balance_effect_liability_credit() {
        let effect = DoubleEntryEngine::calculate_balance_effect(
            AccountType::Liability,
            EntryType::Credit,
            Decimal::from(100),
        );
        assert_eq!(effect, Decimal::from(100));
    }

    #[test]
    fn test_calculate_balance_effect_liability_debit() {
        let effect = DoubleEntryEngine::calculate_balance_effect(
            AccountType::Liability,
            EntryType::Debit,
            Decimal::from(100),
        );
        assert_eq!(effect, Decimal::from(-100));
    }
}
