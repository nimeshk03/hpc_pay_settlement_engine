use crate::error::{AppError, Result};
use crate::models::{BatchStatus, SettlementBatch};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for SettlementBatch lifecycle management.
pub struct BatchRepository {
    pool: PgPool,
}

impl BatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new settlement batch.
    pub async fn create(&self, batch: &SettlementBatch) -> Result<SettlementBatch> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            INSERT INTO settlement_batches (id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            "#,
        )
        .bind(batch.id)
        .bind(&batch.status)
        .bind(batch.settlement_date)
        .bind(batch.cut_off_time)
        .bind(batch.total_transactions)
        .bind(batch.gross_amount)
        .bind(batch.net_amount)
        .bind(batch.fee_amount)
        .bind(&batch.currency)
        .bind(&batch.metadata)
        .bind(batch.created_at)
        .bind(batch.completed_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds a batch by ID.
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<SettlementBatch>> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds batches by status.
    pub async fn find_by_status(&self, status: BatchStatus) -> Result<Vec<SettlementBatch>> {
        let rows = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE status = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds the current open batch for a settlement date and currency.
    pub async fn find_open_batch(
        &self,
        settlement_date: NaiveDate,
        currency: &str,
    ) -> Result<Option<SettlementBatch>> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE settlement_date = $1 AND currency = $2 AND status = 'PENDING'
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(settlement_date)
        .bind(currency)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Lists batches with pagination.
    pub async fn list(
        &self,
        status: Option<BatchStatus>,
        currency: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SettlementBatch>> {
        let rows = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE ($1::batch_status IS NULL OR status = $1)
              AND ($2::text IS NULL OR currency = $2)
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(status)
        .bind(currency)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Updates batch status.
    pub async fn update_status(
        &self,
        id: Uuid,
        status: BatchStatus,
    ) -> Result<Option<SettlementBatch>> {
        let completed_at = if status == BatchStatus::Completed || status == BatchStatus::Failed {
            Some(Utc::now())
        } else {
            None
        };

        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            UPDATE settlement_batches
            SET status = $2, completed_at = COALESCE($3, completed_at)
            WHERE id = $1
            RETURNING id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(completed_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Updates batch totals (transaction count, amounts).
    pub async fn update_totals(
        &self,
        id: Uuid,
        total_transactions: i32,
        gross_amount: Decimal,
        net_amount: Decimal,
        fee_amount: Decimal,
    ) -> Result<Option<SettlementBatch>> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            UPDATE settlement_batches
            SET total_transactions = $2, gross_amount = $3, net_amount = $4, fee_amount = $5
            WHERE id = $1
            RETURNING id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            "#,
        )
        .bind(id)
        .bind(total_transactions)
        .bind(gross_amount)
        .bind(net_amount)
        .bind(fee_amount)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Increments batch totals atomically when adding a transaction.
    pub async fn increment_totals(
        &self,
        id: Uuid,
        amount: Decimal,
        fee: Decimal,
    ) -> Result<Option<SettlementBatch>> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            UPDATE settlement_batches
            SET total_transactions = total_transactions + 1,
                gross_amount = gross_amount + $2,
                fee_amount = fee_amount + $3
            WHERE id = $1 AND status = 'PENDING'
            RETURNING id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            "#,
        )
        .bind(id)
        .bind(amount)
        .bind(fee)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Decrements batch totals atomically when removing a transaction.
    pub async fn decrement_totals(
        &self,
        id: Uuid,
        amount: Decimal,
        fee: Decimal,
    ) -> Result<Option<SettlementBatch>> {
        let row = sqlx::query_as::<_, SettlementBatch>(
            r#"
            UPDATE settlement_batches
            SET total_transactions = GREATEST(total_transactions - 1, 0),
                gross_amount = gross_amount - $2,
                fee_amount = fee_amount - $3
            WHERE id = $1 AND status = 'PENDING'
            RETURNING id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            "#,
        )
        .bind(id)
        .bind(amount)
        .bind(fee)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds batches ready for processing (pending with past cut-off time).
    pub async fn find_ready_for_processing(&self) -> Result<Vec<SettlementBatch>> {
        let rows = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE status = 'PENDING' AND cut_off_time <= NOW()
            ORDER BY cut_off_time
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds batches by settlement date.
    pub async fn find_by_settlement_date(
        &self,
        settlement_date: NaiveDate,
    ) -> Result<Vec<SettlementBatch>> {
        let rows = sqlx::query_as::<_, SettlementBatch>(
            r#"
            SELECT id, status, settlement_date, cut_off_time, total_transactions, gross_amount, net_amount, fee_amount, currency, metadata, created_at, completed_at
            FROM settlement_batches
            WHERE settlement_date = $1
            ORDER BY created_at
            "#,
        )
        .bind(settlement_date)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Counts batches by status.
    pub async fn count_by_status(&self, status: BatchStatus) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM settlement_batches
            WHERE status = $1
            "#,
        )
        .bind(status)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }

    /// Gets or creates a batch for the given date and currency.
    pub async fn get_or_create(
        &self,
        settlement_date: NaiveDate,
        cut_off_time: DateTime<Utc>,
        currency: &str,
    ) -> Result<SettlementBatch> {
        let existing = self.find_open_batch(settlement_date, currency).await?;

        if let Some(batch) = existing {
            return Ok(batch);
        }

        let new_batch = SettlementBatch::new(settlement_date, cut_off_time, currency.to_string());
        self.create(&new_batch).await
    }
}
