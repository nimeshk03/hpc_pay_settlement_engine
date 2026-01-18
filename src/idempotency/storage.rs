use crate::error::{AppError, Result};
use chrono::{DateTime, Duration, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Status of an idempotency record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "UPPERCASE")]
pub enum IdempotencyStatus {
    Processing,
    Completed,
    Failed,
}

/// Stored idempotency record.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IdempotencyRecord {
    pub id: Uuid,
    pub idempotency_key: String,
    pub client_id: String,
    pub operation_type: String,
    pub status: IdempotencyStatus,
    pub request_hash: String,
    pub response_data: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl IdempotencyRecord {
    pub fn new(
        idempotency_key: String,
        client_id: String,
        operation_type: String,
        request_hash: String,
        ttl_seconds: i64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            idempotency_key,
            client_id,
            operation_type,
            status: IdempotencyStatus::Processing,
            request_hash,
            response_data: None,
            error_message: None,
            created_at: now,
            expires_at: now + Duration::seconds(ttl_seconds),
            completed_at: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn is_completed(&self) -> bool {
        self.status == IdempotencyStatus::Completed
    }

    pub fn is_failed(&self) -> bool {
        self.status == IdempotencyStatus::Failed
    }
}

/// PostgreSQL-based idempotency storage.
pub struct PostgresIdempotencyStore {
    pool: PgPool,
}

impl PostgresIdempotencyStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Attempts to acquire an idempotency lock.
    /// Returns Ok(None) if the key doesn't exist (new request).
    /// Returns Ok(Some(record)) if the key exists (duplicate request).
    pub async fn try_acquire(&self, record: &IdempotencyRecord) -> Result<Option<IdempotencyRecord>> {
        // Try to insert, return existing if conflict
        let existing = sqlx::query_as::<_, IdempotencyRecord>(
            r#"
            INSERT INTO idempotency_keys (id, idempotency_key, client_id, operation_type, status, request_hash, response_data, error_message, created_at, expires_at, completed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (idempotency_key) DO UPDATE SET idempotency_key = idempotency_keys.idempotency_key
            RETURNING id, idempotency_key, client_id, operation_type, status, request_hash, response_data, error_message, created_at, expires_at, completed_at
            "#,
        )
        .bind(record.id)
        .bind(&record.idempotency_key)
        .bind(&record.client_id)
        .bind(&record.operation_type)
        .bind(&record.status)
        .bind(&record.request_hash)
        .bind(&record.response_data)
        .bind(&record.error_message)
        .bind(record.created_at)
        .bind(record.expires_at)
        .bind(record.completed_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        // If the returned ID matches our new record, it was inserted (new request)
        if existing.id == record.id {
            Ok(None)
        } else {
            // Existing record found (duplicate)
            Ok(Some(existing))
        }
    }

    /// Finds a record by idempotency key.
    pub async fn find_by_key(&self, key: &str) -> Result<Option<IdempotencyRecord>> {
        let record = sqlx::query_as::<_, IdempotencyRecord>(
            r#"
            SELECT id, idempotency_key, client_id, operation_type, status, request_hash, response_data, error_message, created_at, expires_at, completed_at
            FROM idempotency_keys
            WHERE idempotency_key = $1
            "#,
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(record)
    }

    /// Marks a record as completed with response data.
    pub async fn mark_completed(
        &self,
        key: &str,
        response_data: serde_json::Value,
    ) -> Result<Option<IdempotencyRecord>> {
        let record = sqlx::query_as::<_, IdempotencyRecord>(
            r#"
            UPDATE idempotency_keys
            SET status = 'COMPLETED', response_data = $2, completed_at = NOW()
            WHERE idempotency_key = $1
            RETURNING id, idempotency_key, client_id, operation_type, status, request_hash, response_data, error_message, created_at, expires_at, completed_at
            "#,
        )
        .bind(key)
        .bind(response_data)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(record)
    }

    /// Marks a record as failed with error message.
    pub async fn mark_failed(
        &self,
        key: &str,
        error_message: &str,
    ) -> Result<Option<IdempotencyRecord>> {
        let record = sqlx::query_as::<_, IdempotencyRecord>(
            r#"
            UPDATE idempotency_keys
            SET status = 'FAILED', error_message = $2, completed_at = NOW()
            WHERE idempotency_key = $1
            RETURNING id, idempotency_key, client_id, operation_type, status, request_hash, response_data, error_message, created_at, expires_at, completed_at
            "#,
        )
        .bind(key)
        .bind(error_message)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(record)
    }

    /// Deletes expired records.
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM idempotency_keys
            WHERE expires_at < NOW()
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(result.rows_affected())
    }

    /// Deletes a specific record by key.
    pub async fn delete(&self, key: &str) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM idempotency_keys
            WHERE idempotency_key = $1
            "#,
        )
        .bind(key)
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(result.rows_affected() > 0)
    }

    /// Counts records by status.
    pub async fn count_by_status(&self, status: IdempotencyStatus) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM idempotency_keys WHERE status = $1
            "#,
        )
        .bind(status)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }
}

/// Redis-based idempotency cache for fast lookups.
pub struct RedisIdempotencyCache {
    client: redis::Client,
    key_prefix: String,
}

impl RedisIdempotencyCache {
    pub fn new(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            key_prefix: key_prefix.into(),
        }
    }

    fn make_key(&self, idempotency_key: &str) -> String {
        format!("{}:{}", self.key_prefix, idempotency_key)
    }

    /// Attempts to set a key with NX (only if not exists) and TTL.
    /// Returns true if the key was set (new request), false if it already exists.
    pub async fn try_set(&self, idempotency_key: &str, ttl_seconds: i64) -> Result<bool> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(AppError::Redis)?;

        let key = self.make_key(idempotency_key);
        let result: Option<String> = conn
            .set_options(
                &key,
                "processing",
                redis::SetOptions::default()
                    .conditional_set(redis::ExistenceCheck::NX)
                    .with_expiration(redis::SetExpiry::EX(ttl_seconds as usize)),
            )
            .await
            .map_err(AppError::Redis)?;

        Ok(result.is_some())
    }

    /// Gets the cached response for a key.
    pub async fn get_response(&self, idempotency_key: &str) -> Result<Option<String>> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(AppError::Redis)?;

        let key = self.make_key(idempotency_key);
        let value: Option<String> = conn.get(&key).await.map_err(AppError::Redis)?;

        Ok(value)
    }

    /// Sets the response for a completed request.
    pub async fn set_response(
        &self,
        idempotency_key: &str,
        response: &str,
        ttl_seconds: i64,
    ) -> Result<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(AppError::Redis)?;

        let key = self.make_key(idempotency_key);
        let _: () = conn.set_ex(&key, response, ttl_seconds as u64)
            .await
            .map_err(AppError::Redis)?;

        Ok(())
    }

    /// Deletes a key from cache.
    pub async fn delete(&self, idempotency_key: &str) -> Result<bool> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(AppError::Redis)?;

        let key = self.make_key(idempotency_key);
        let deleted: i64 = conn.del(&key).await.map_err(AppError::Redis)?;

        Ok(deleted > 0)
    }

    /// Checks if a key exists.
    pub async fn exists(&self, idempotency_key: &str) -> Result<bool> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(AppError::Redis)?;

        let key = self.make_key(idempotency_key);
        let exists: bool = conn.exists(&key).await.map_err(AppError::Redis)?;

        Ok(exists)
    }
}

/// Combined storage that uses both PostgreSQL and Redis.
pub struct HybridIdempotencyStore {
    postgres: PostgresIdempotencyStore,
    redis: RedisIdempotencyCache,
    ttl_seconds: i64,
}

impl HybridIdempotencyStore {
    pub fn new(
        postgres: PostgresIdempotencyStore,
        redis: RedisIdempotencyCache,
        ttl_seconds: i64,
    ) -> Self {
        Self {
            postgres,
            redis,
            ttl_seconds,
        }
    }

    /// Checks if a request is a duplicate using Redis first, then PostgreSQL.
    pub async fn check_duplicate(&self, idempotency_key: &str) -> Result<Option<IdempotencyRecord>> {
        // Check Redis first (fast path)
        if self.redis.exists(idempotency_key).await? {
            // Found in Redis, get full record from PostgreSQL
            return self.postgres.find_by_key(idempotency_key).await;
        }

        // Not in Redis, check PostgreSQL
        self.postgres.find_by_key(idempotency_key).await
    }

    /// Attempts to acquire an idempotency lock.
    pub async fn try_acquire(&self, record: &IdempotencyRecord) -> Result<Option<IdempotencyRecord>> {
        // Try Redis first
        let is_new = self.redis.try_set(&record.idempotency_key, self.ttl_seconds).await?;

        if !is_new {
            // Key exists in Redis, get record from PostgreSQL
            return self.postgres.find_by_key(&record.idempotency_key).await;
        }

        // New key, try to insert into PostgreSQL
        match self.postgres.try_acquire(record).await {
            Ok(existing) => {
                if existing.is_some() {
                    // Race condition: another process inserted first
                    // Update Redis with the existing key
                    self.redis
                        .set_response(&record.idempotency_key, "processing", self.ttl_seconds)
                        .await?;
                }
                Ok(existing)
            }
            Err(e) => {
                // Clean up Redis on error
                self.redis.delete(&record.idempotency_key).await.ok();
                Err(e)
            }
        }
    }

    /// Marks a request as completed.
    pub async fn mark_completed(
        &self,
        idempotency_key: &str,
        response_data: serde_json::Value,
    ) -> Result<Option<IdempotencyRecord>> {
        // Update PostgreSQL
        let record = self.postgres.mark_completed(idempotency_key, response_data.clone()).await?;

        // Update Redis cache with response
        if record.is_some() {
            let response_str = serde_json::to_string(&response_data).unwrap_or_default();
            self.redis
                .set_response(idempotency_key, &response_str, self.ttl_seconds)
                .await?;
        }

        Ok(record)
    }

    /// Marks a request as failed.
    pub async fn mark_failed(
        &self,
        idempotency_key: &str,
        error_message: &str,
    ) -> Result<Option<IdempotencyRecord>> {
        // Update PostgreSQL
        let record = self.postgres.mark_failed(idempotency_key, error_message).await?;

        // Remove from Redis (failed requests shouldn't be cached)
        self.redis.delete(idempotency_key).await.ok();

        Ok(record)
    }

    /// Runs cleanup of expired records.
    pub async fn cleanup_expired(&self) -> Result<u64> {
        self.postgres.cleanup_expired().await
    }
}
