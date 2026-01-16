use crate::error::{AppError, Result};
use crate::models::NettingPosition;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for NettingPosition storage and queries.
pub struct NettingRepository {
    pool: PgPool,
}

impl NettingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new netting position.
    pub async fn create(&self, position: &NettingPosition) -> Result<NettingPosition> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            INSERT INTO netting_positions (batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            "#,
        )
        .bind(position.batch_id)
        .bind(position.participant_id)
        .bind(&position.currency)
        .bind(position.gross_receivable)
        .bind(position.gross_payable)
        .bind(position.net_position)
        .bind(position.transaction_count)
        .bind(position.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Creates multiple netting positions in a single transaction.
    pub async fn create_batch(&self, positions: &[NettingPosition]) -> Result<Vec<NettingPosition>> {
        let mut tx = self.pool.begin().await.map_err(AppError::Database)?;
        let mut created = Vec::with_capacity(positions.len());

        for position in positions {
            let row = sqlx::query_as::<_, NettingPosition>(
                r#"
                INSERT INTO netting_positions (batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
                "#,
            )
            .bind(position.batch_id)
            .bind(position.participant_id)
            .bind(&position.currency)
            .bind(position.gross_receivable)
            .bind(position.gross_payable)
            .bind(position.net_position)
            .bind(position.transaction_count)
            .bind(position.created_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?;

            created.push(row);
        }

        tx.commit().await.map_err(AppError::Database)?;
        Ok(created)
    }

    /// Finds a position by batch and participant.
    pub async fn find_by_batch_and_participant(
        &self,
        batch_id: Uuid,
        participant_id: Uuid,
        currency: &str,
    ) -> Result<Option<NettingPosition>> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            SELECT batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            FROM netting_positions
            WHERE batch_id = $1 AND participant_id = $2 AND currency = $3
            "#,
        )
        .bind(batch_id)
        .bind(participant_id)
        .bind(currency)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds all positions for a batch.
    pub async fn find_by_batch(&self, batch_id: Uuid) -> Result<Vec<NettingPosition>> {
        let rows = sqlx::query_as::<_, NettingPosition>(
            r#"
            SELECT batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            FROM netting_positions
            WHERE batch_id = $1
            ORDER BY net_position DESC
            "#,
        )
        .bind(batch_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds all positions for a participant across batches.
    pub async fn find_by_participant(&self, participant_id: Uuid) -> Result<Vec<NettingPosition>> {
        let rows = sqlx::query_as::<_, NettingPosition>(
            r#"
            SELECT batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            FROM netting_positions
            WHERE participant_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(participant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Updates a netting position.
    pub async fn update(&self, position: &NettingPosition) -> Result<Option<NettingPosition>> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            UPDATE netting_positions
            SET gross_receivable = $4,
                gross_payable = $5,
                net_position = $6,
                transaction_count = $7
            WHERE batch_id = $1 AND participant_id = $2 AND currency = $3
            RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            "#,
        )
        .bind(position.batch_id)
        .bind(position.participant_id)
        .bind(&position.currency)
        .bind(position.gross_receivable)
        .bind(position.gross_payable)
        .bind(position.net_position)
        .bind(position.transaction_count)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Upserts a netting position (insert or update).
    pub async fn upsert(&self, position: &NettingPosition) -> Result<NettingPosition> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            INSERT INTO netting_positions (batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (batch_id, participant_id, currency)
            DO UPDATE SET
                gross_receivable = EXCLUDED.gross_receivable,
                gross_payable = EXCLUDED.gross_payable,
                net_position = EXCLUDED.net_position,
                transaction_count = EXCLUDED.transaction_count
            RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            "#,
        )
        .bind(position.batch_id)
        .bind(position.participant_id)
        .bind(&position.currency)
        .bind(position.gross_receivable)
        .bind(position.gross_payable)
        .bind(position.net_position)
        .bind(position.transaction_count)
        .bind(position.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Adds a receivable to a position atomically.
    pub async fn add_receivable(
        &self,
        batch_id: Uuid,
        participant_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<NettingPosition> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            UPDATE netting_positions
            SET gross_receivable = gross_receivable + $4,
                net_position = net_position + $4,
                transaction_count = transaction_count + 1
            WHERE batch_id = $1 AND participant_id = $2 AND currency = $3
            RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            "#,
        )
        .bind(batch_id)
        .bind(participant_id)
        .bind(currency)
        .bind(amount)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Adds a payable to a position atomically.
    pub async fn add_payable(
        &self,
        batch_id: Uuid,
        participant_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<NettingPosition> {
        let row = sqlx::query_as::<_, NettingPosition>(
            r#"
            UPDATE netting_positions
            SET gross_payable = gross_payable + $4,
                net_position = net_position - $4,
                transaction_count = transaction_count + 1
            WHERE batch_id = $1 AND participant_id = $2 AND currency = $3
            RETURNING batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            "#,
        )
        .bind(batch_id)
        .bind(participant_id)
        .bind(currency)
        .bind(amount)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds net receivers for a batch (positive net position).
    pub async fn find_net_receivers(&self, batch_id: Uuid) -> Result<Vec<NettingPosition>> {
        let rows = sqlx::query_as::<_, NettingPosition>(
            r#"
            SELECT batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            FROM netting_positions
            WHERE batch_id = $1 AND net_position > 0
            ORDER BY net_position DESC
            "#,
        )
        .bind(batch_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Finds net payers for a batch (negative net position).
    pub async fn find_net_payers(&self, batch_id: Uuid) -> Result<Vec<NettingPosition>> {
        let rows = sqlx::query_as::<_, NettingPosition>(
            r#"
            SELECT batch_id, participant_id, currency, gross_receivable, gross_payable, net_position, transaction_count, created_at
            FROM netting_positions
            WHERE batch_id = $1 AND net_position < 0
            ORDER BY net_position ASC
            "#,
        )
        .bind(batch_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Calculates batch summary statistics.
    pub async fn get_batch_summary(
        &self,
        batch_id: Uuid,
    ) -> Result<BatchNettingSummary> {
        let row: (i64, Option<Decimal>, Option<Decimal>, i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT 
                COUNT(*) as participant_count,
                SUM(gross_receivable + gross_payable) as total_gross_volume,
                SUM(ABS(net_position)) as total_net_volume,
                COALESCE(SUM(transaction_count), 0) as total_transactions,
                COUNT(*) FILTER (WHERE net_position > 0) as net_receivers,
                COUNT(*) FILTER (WHERE net_position < 0) as net_payers,
                COUNT(*) FILTER (WHERE net_position = 0) as balanced
            FROM netting_positions
            WHERE batch_id = $1
            "#,
        )
        .bind(batch_id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(BatchNettingSummary {
            batch_id,
            participant_count: row.0 as i32,
            total_gross_volume: row.1.unwrap_or(Decimal::ZERO),
            total_net_volume: row.2.unwrap_or(Decimal::ZERO),
            total_transactions: row.3 as i32,
            net_receivers: row.4 as i32,
            net_payers: row.5 as i32,
            balanced_participants: row.6 as i32,
        })
    }

    /// Deletes all positions for a batch.
    pub async fn delete_by_batch(&self, batch_id: Uuid) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM netting_positions
            WHERE batch_id = $1
            "#,
        )
        .bind(batch_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(result.rows_affected())
    }
}

/// Summary of netting results for a batch.
#[derive(Debug, Clone)]
pub struct BatchNettingSummary {
    pub batch_id: Uuid,
    pub participant_count: i32,
    pub total_gross_volume: Decimal,
    pub total_net_volume: Decimal,
    pub total_transactions: i32,
    pub net_receivers: i32,
    pub net_payers: i32,
    pub balanced_participants: i32,
}

impl BatchNettingSummary {
    /// Returns the netting efficiency as a percentage.
    pub fn netting_efficiency(&self) -> Decimal {
        if self.total_gross_volume.is_zero() {
            return Decimal::ZERO;
        }
        let reduction = self.total_gross_volume - self.total_net_volume;
        (reduction / self.total_gross_volume) * Decimal::from(100)
    }
}
