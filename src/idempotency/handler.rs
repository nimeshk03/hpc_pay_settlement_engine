use crate::error::{AppError, Result};
use crate::idempotency::key_generator::{IdempotencyAttributes, IdempotencyKeyGenerator, KeyGeneratorConfig};
use crate::idempotency::storage::{
    HybridIdempotencyStore, IdempotencyRecord, IdempotencyStatus, PostgresIdempotencyStore,
    RedisIdempotencyCache,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Metrics for idempotency handling.
#[derive(Debug, Default)]
pub struct IdempotencyMetrics {
    pub total_requests: AtomicU64,
    pub duplicate_requests: AtomicU64,
    pub new_requests: AtomicU64,
    pub completed_requests: AtomicU64,
    pub failed_requests: AtomicU64,
}

impl IdempotencyMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_duplicate(&self) {
        self.duplicate_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_new(&self) {
        self.new_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_completed(&self) {
        self.completed_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failed(&self) {
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn duplicate_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        let duplicates = self.duplicate_requests.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            duplicates as f64 / total as f64
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            duplicate_requests: self.duplicate_requests.load(Ordering::Relaxed),
            new_requests: self.new_requests.load(Ordering::Relaxed),
            completed_requests: self.completed_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub total_requests: u64,
    pub duplicate_requests: u64,
    pub new_requests: u64,
    pub completed_requests: u64,
    pub failed_requests: u64,
}

impl MetricsSnapshot {
    pub fn duplicate_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.duplicate_requests as f64 / self.total_requests as f64
        }
    }
}

/// Result of an idempotency check.
#[derive(Debug)]
pub enum IdempotencyCheckResult<T> {
    /// New request, proceed with processing
    New,
    /// Duplicate request with cached response
    Duplicate(T),
    /// Duplicate request still processing
    Processing,
}

/// Configuration for the idempotency handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyHandlerConfig {
    pub ttl_seconds: i64,
    pub key_prefix: String,
    pub include_timestamp_in_key: bool,
}

impl Default for IdempotencyHandlerConfig {
    fn default() -> Self {
        Self {
            ttl_seconds: 86400, // 24 hours
            key_prefix: "idem".to_string(),
            include_timestamp_in_key: false,
        }
    }
}

/// Handler for idempotent request processing.
pub struct IdempotencyHandler {
    store: HybridIdempotencyStore,
    key_generator: IdempotencyKeyGenerator,
    metrics: Arc<IdempotencyMetrics>,
    config: IdempotencyHandlerConfig,
}

impl IdempotencyHandler {
    pub fn new(
        pool: PgPool,
        redis_client: redis::Client,
        config: IdempotencyHandlerConfig,
    ) -> Self {
        let postgres_store = PostgresIdempotencyStore::new(pool);
        let redis_cache = RedisIdempotencyCache::new(redis_client, &config.key_prefix);
        let store = HybridIdempotencyStore::new(postgres_store, redis_cache, config.ttl_seconds);

        let key_config = KeyGeneratorConfig {
            time_window_seconds: config.ttl_seconds,
            include_timestamp: config.include_timestamp_in_key,
            key_prefix: config.key_prefix.clone(),
        };
        let key_generator = IdempotencyKeyGenerator::new(key_config);

        Self {
            store,
            key_generator,
            metrics: Arc::new(IdempotencyMetrics::new()),
            config,
        }
    }

    /// Gets the metrics for this handler.
    pub fn metrics(&self) -> Arc<IdempotencyMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Generates an idempotency key from attributes.
    pub fn generate_key(&self, attributes: &IdempotencyAttributes) -> String {
        self.key_generator.generate(attributes)
    }

    /// Normalizes a client-provided idempotency key.
    pub fn normalize_client_key(&self, client_key: &str) -> String {
        self.key_generator.from_client_key(client_key)
    }

    /// Computes a hash of the request body for verification.
    pub fn hash_request<T: Serialize>(&self, request: &T) -> String {
        let json = serde_json::to_string(request).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Checks if a request is a duplicate and returns the cached response if available.
    pub async fn check<T: for<'de> Deserialize<'de>>(
        &self,
        idempotency_key: &str,
        client_id: &str,
        operation_type: &str,
        request_hash: &str,
    ) -> Result<IdempotencyCheckResult<T>> {
        self.metrics.record_request();

        // Check for existing record
        if let Some(existing) = self.store.check_duplicate(idempotency_key).await? {
            self.metrics.record_duplicate();

            // Verify request hash matches (same request)
            if existing.request_hash != request_hash {
                return Err(AppError::Validation(
                    "Idempotency key reused with different request parameters".to_string(),
                ));
            }

            match existing.status {
                IdempotencyStatus::Completed => {
                    if let Some(response_data) = existing.response_data {
                        let response: T = serde_json::from_value(response_data)
                            .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to deserialize cached response: {}", e)))?;
                        return Ok(IdempotencyCheckResult::Duplicate(response));
                    }
                    return Err(AppError::Internal(anyhow::anyhow!(
                        "Completed idempotency record has no response data"
                    )));
                }
                IdempotencyStatus::Processing => {
                    return Ok(IdempotencyCheckResult::Processing);
                }
                IdempotencyStatus::Failed => {
                    // Allow retry of failed requests
                    self.metrics.record_new();
                    return Ok(IdempotencyCheckResult::New);
                }
            }
        }

        // New request, try to acquire lock
        let record = IdempotencyRecord::new(
            idempotency_key.to_string(),
            client_id.to_string(),
            operation_type.to_string(),
            request_hash.to_string(),
            self.config.ttl_seconds,
        );

        match self.store.try_acquire(&record).await? {
            Some(existing) => {
                self.metrics.record_duplicate();

                if existing.request_hash != request_hash {
                    return Err(AppError::Validation(
                        "Idempotency key reused with different request parameters".to_string(),
                    ));
                }

                match existing.status {
                    IdempotencyStatus::Completed => {
                        if let Some(response_data) = existing.response_data {
                            let response: T = serde_json::from_value(response_data)
                                .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to deserialize cached response: {}", e)))?;
                            return Ok(IdempotencyCheckResult::Duplicate(response));
                        }
                        return Err(AppError::Internal(anyhow::anyhow!(
                            "Completed idempotency record has no response data"
                        )));
                    }
                    IdempotencyStatus::Processing => {
                        return Ok(IdempotencyCheckResult::Processing);
                    }
                    IdempotencyStatus::Failed => {
                        self.metrics.record_new();
                        return Ok(IdempotencyCheckResult::New);
                    }
                }
            }
            None => {
                self.metrics.record_new();
                Ok(IdempotencyCheckResult::New)
            }
        }
    }

    /// Marks a request as completed with the response.
    pub async fn complete<T: Serialize>(
        &self,
        idempotency_key: &str,
        response: &T,
    ) -> Result<()> {
        let response_data = serde_json::to_value(response)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize response: {}", e)))?;

        self.store.mark_completed(idempotency_key, response_data).await?;
        self.metrics.record_completed();

        Ok(())
    }

    /// Marks a request as failed.
    pub async fn fail(&self, idempotency_key: &str, error_message: &str) -> Result<()> {
        self.store.mark_failed(idempotency_key, error_message).await?;
        self.metrics.record_failed();

        Ok(())
    }

    /// Executes an operation with idempotency handling.
    /// This is the main entry point for idempotent request processing.
    pub async fn execute<T, F, Fut>(
        &self,
        idempotency_key: &str,
        client_id: &str,
        operation_type: &str,
        request_hash: &str,
        operation: F,
    ) -> Result<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        // Check for duplicate
        match self.check::<T>(idempotency_key, client_id, operation_type, request_hash).await? {
            IdempotencyCheckResult::Duplicate(response) => {
                return Ok(response);
            }
            IdempotencyCheckResult::Processing => {
                return Err(AppError::Validation(
                    "Request is currently being processed. Please retry later.".to_string(),
                ));
            }
            IdempotencyCheckResult::New => {
                // Proceed with processing
            }
        }

        // Execute the operation
        match operation().await {
            Ok(response) => {
                // Mark as completed
                if let Err(e) = self.complete(idempotency_key, &response).await {
                    tracing::error!("Failed to mark idempotency key as completed: {}", e);
                }
                Ok(response)
            }
            Err(e) => {
                // Mark as failed
                if let Err(mark_err) = self.fail(idempotency_key, &e.to_string()).await {
                    tracing::error!("Failed to mark idempotency key as failed: {}", mark_err);
                }
                Err(e)
            }
        }
    }

    /// Runs cleanup of expired idempotency records.
    pub async fn cleanup_expired(&self) -> Result<u64> {
        self.store.cleanup_expired().await
    }
}

/// Background cleanup job for expired idempotency records.
pub struct IdempotencyCleanupJob {
    handler: Arc<IdempotencyHandler>,
    interval_seconds: u64,
}

impl IdempotencyCleanupJob {
    pub fn new(handler: Arc<IdempotencyHandler>, interval_seconds: u64) -> Self {
        Self {
            handler,
            interval_seconds,
        }
    }

    /// Runs the cleanup job once.
    pub async fn run_once(&self) -> Result<u64> {
        self.handler.cleanup_expired().await
    }

    /// Starts the cleanup job in a background task.
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(self.interval_seconds)
            );

            loop {
                interval.tick().await;

                match self.handler.cleanup_expired().await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!("Cleaned up {} expired idempotency records", count);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to cleanup expired idempotency records: {}", e);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_snapshot() {
        let metrics = IdempotencyMetrics::new();
        metrics.record_request();
        metrics.record_request();
        metrics.record_duplicate();
        metrics.record_new();
        metrics.record_completed();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.duplicate_requests, 1);
        assert_eq!(snapshot.new_requests, 1);
        assert_eq!(snapshot.completed_requests, 1);
        assert_eq!(snapshot.duplicate_rate(), 0.5);
    }

    #[test]
    fn test_default_config() {
        let config = IdempotencyHandlerConfig::default();
        assert_eq!(config.ttl_seconds, 86400);
        assert_eq!(config.key_prefix, "idem");
        assert!(!config.include_timestamp_in_key);
    }
}
