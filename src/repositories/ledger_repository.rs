use crate::error::{AppError, Result};
use crate::models::{EntryType, LedgerEntry};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for LedgerEntry operations.
pub struct LedgerRepository {
    pool: PgPool,
}

impl LedgerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new ledger entry.
    pub async fn create(&self, entry: &LedgerEntry) -> Result<LedgerEntry> {
        let row = sqlx::query_as::<_, LedgerEntry>(
            r#"
            INSERT INTO ledger_entries (id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            "#,
        )
        .bind(entry.id)
        .bind(entry.transaction_id)
        .bind(entry.account_id)
        .bind(&entry.entry_type)
        .bind(entry.amount)
        .bind(&entry.currency)
        .bind(entry.balance_after)
        .bind(entry.effective_date)
        .bind(&entry.metadata)
        .bind(entry.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Creates multiple ledger entries in a single transaction.
    pub async fn create_batch(&self, entries: &[LedgerEntry]) -> Result<Vec<LedgerEntry>> {
        let mut tx = self.pool.begin().await.map_err(AppError::Database)?;
        let mut created = Vec::with_capacity(entries.len());

        for entry in entries {
            let row = sqlx::query_as::<_, LedgerEntry>(
                r#"
                INSERT INTO ledger_entries (id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
                "#,
            )
            .bind(entry.id)
            .bind(entry.transaction_id)
            .bind(entry.account_id)
            .bind(&entry.entry_type)
            .bind(entry.amount)
            .bind(&entry.currency)
            .bind(entry.balance_after)
            .bind(entry.effective_date)
            .bind(&entry.metadata)
            .bind(entry.created_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?;

            created.push(row);
        }

        tx.commit().await.map_err(AppError::Database)?;
        Ok(created)
    }

    /// Finds a ledger entry by ID.
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<LedgerEntry>> {
        let row = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds all entries for a transaction.
    pub async fn find_by_transaction(&self, transaction_id: Uuid) -> Result<Vec<LedgerEntry>> {
        let rows = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE transaction_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(transaction_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds entries for an account with pagination.
    pub async fn find_by_account(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LedgerEntry>> {
        let rows = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE account_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(account_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Counts entries for an account for pagination.
    pub async fn count_by_account(&self, account_id: Uuid) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM ledger_entries
            WHERE account_id = $1
            "#,
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }

    /// Finds entries for an account within a date range.
    pub async fn find_by_account_and_date_range(
        &self,
        account_id: Uuid,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<LedgerEntry>> {
        let rows = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE account_id = $1
              AND effective_date >= $2
              AND effective_date <= $3
            ORDER BY created_at
            "#,
        )
        .bind(account_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Calculates the sum of entries for an account by type.
    pub async fn sum_by_account_and_type(
        &self,
        account_id: Uuid,
        currency: &str,
        entry_type: EntryType,
    ) -> Result<Decimal> {
        let row: (Option<Decimal>,) = sqlx::query_as(
            r#"
            SELECT COALESCE(SUM(amount), 0)
            FROM ledger_entries
            WHERE account_id = $1 AND currency = $2 AND entry_type = $3
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(entry_type)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0.unwrap_or(Decimal::ZERO))
    }

    /// Gets the latest entry for an account (for balance verification).
    pub async fn get_latest_by_account(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<Option<LedgerEntry>> {
        let row = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE account_id = $1 AND currency = $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Verifies that debits equal credits for a transaction.
    pub async fn verify_transaction_balance(&self, transaction_id: Uuid) -> Result<bool> {
        let row: (Decimal, Decimal) = sqlx::query_as(
            r#"
            SELECT 
                COALESCE(SUM(CASE WHEN entry_type = 'DEBIT' THEN amount ELSE 0 END), 0) as debits,
                COALESCE(SUM(CASE WHEN entry_type = 'CREDIT' THEN amount ELSE 0 END), 0) as credits
            FROM ledger_entries
            WHERE transaction_id = $1
            "#,
        )
        .bind(transaction_id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0 == row.1)
    }

    /// Gets entries created within a time range (for batch processing).
    pub async fn find_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<LedgerEntry>> {
        let rows = sqlx::query_as::<_, LedgerEntry>(
            r#"
            SELECT id, transaction_id, account_id, entry_type, amount, currency, balance_after, effective_date, metadata, created_at
            FROM ledger_entries
            WHERE created_at >= $1 AND created_at < $2
            ORDER BY created_at
            LIMIT $3
            "#,
        )
        .bind(start)
        .bind(end)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }
}
