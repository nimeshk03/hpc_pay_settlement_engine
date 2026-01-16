use crate::error::{AppError, Result};
use crate::models::{TransactionRecord, TransactionStatus, TransactionType};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for TransactionRecord operations.
pub struct TransactionRepository {
    pool: PgPool,
}

impl TransactionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new transaction record.
    pub async fn create(&self, transaction: &TransactionRecord) -> Result<TransactionRecord> {
        let row = sqlx::query_as::<_, TransactionRecord>(
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
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds a transaction by ID.
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<TransactionRecord>> {
        let row = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds a transaction by external ID.
    pub async fn find_by_external_id(&self, external_id: &str) -> Result<Option<TransactionRecord>> {
        let row = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE external_id = $1
            "#,
        )
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds a transaction by idempotency key.
    pub async fn find_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<TransactionRecord>> {
        let row = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE idempotency_key = $1
            "#,
        )
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Lists transactions with filters.
    pub async fn list(
        &self,
        transaction_type: Option<TransactionType>,
        status: Option<TransactionStatus>,
        source_account_id: Option<Uuid>,
        destination_account_id: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TransactionRecord>> {
        let rows = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE ($1::transaction_type IS NULL OR type = $1)
              AND ($2::transaction_status IS NULL OR status = $2)
              AND ($3::uuid IS NULL OR source_account_id = $3)
              AND ($4::uuid IS NULL OR destination_account_id = $4)
            ORDER BY created_at DESC
            LIMIT $5 OFFSET $6
            "#,
        )
        .bind(transaction_type)
        .bind(status)
        .bind(source_account_id)
        .bind(destination_account_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds transactions by settlement batch.
    pub async fn find_by_batch(&self, batch_id: Uuid) -> Result<Vec<TransactionRecord>> {
        let rows = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE settlement_batch_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(batch_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Updates transaction status.
    pub async fn update_status(
        &self,
        id: Uuid,
        status: TransactionStatus,
    ) -> Result<Option<TransactionRecord>> {
        let settled_at = if status == TransactionStatus::Settled {
            Some(Utc::now())
        } else {
            None
        };

        let row = sqlx::query_as::<_, TransactionRecord>(
            r#"
            UPDATE transactions
            SET status = $2, settled_at = COALESCE($3, settled_at)
            WHERE id = $1
            RETURNING id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(settled_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Assigns a transaction to a settlement batch.
    pub async fn assign_to_batch(
        &self,
        id: Uuid,
        batch_id: Uuid,
    ) -> Result<Option<TransactionRecord>> {
        let row = sqlx::query_as::<_, TransactionRecord>(
            r#"
            UPDATE transactions
            SET settlement_batch_id = $2
            WHERE id = $1
            RETURNING id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            "#,
        )
        .bind(id)
        .bind(batch_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds pending transactions not yet assigned to a batch.
    pub async fn find_pending_unassigned(&self, limit: i64) -> Result<Vec<TransactionRecord>> {
        let rows = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE status = 'PENDING' AND settlement_batch_id IS NULL
            ORDER BY created_at
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds transactions for an account (as source or destination).
    pub async fn find_by_account(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TransactionRecord>> {
        let rows = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
            WHERE source_account_id = $1 OR destination_account_id = $1
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

    /// Counts transactions by status.
    pub async fn count_by_status(&self, status: TransactionStatus) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM transactions
            WHERE status = $1
            "#,
        )
        .bind(status)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }

    /// Checks if an idempotency key exists.
    pub async fn exists_by_idempotency_key(&self, idempotency_key: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            r#"
            SELECT EXISTS(SELECT 1 FROM transactions WHERE idempotency_key = $1)
            "#,
        )
        .bind(idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }

    /// Finds transactions within a time range.
    pub async fn find_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<TransactionRecord>> {
        let rows = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT id, external_id, type, status, source_account_id, destination_account_id, amount, currency, fee_amount, net_amount, settlement_batch_id, idempotency_key, metadata, created_at, settled_at
            FROM transactions
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
