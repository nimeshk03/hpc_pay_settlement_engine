use crate::config::CacheSettings;
use crate::error::{AppError, Result};
use crate::models::AccountBalance;
use crate::observability::get_metrics;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Cache statistics for monitoring.
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub invalidations: AtomicU64,
    pub errors: AtomicU64,
}

impl CacheStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    pub fn get_hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn get_misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn get_invalidations(&self) -> u64 {
        self.invalidations.load(Ordering::Relaxed)
    }

    pub fn get_errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
}

/// Cached balance entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBalance {
    balance: AccountBalance,
    cached_at: i64,
    version: i32,
}

/// Redis-based cache for account balances.
pub struct BalanceCache {
    client: redis::Client,
    settings: CacheSettings,
    stats: Arc<CacheStats>,
}

impl BalanceCache {
    pub fn new(client: redis::Client, settings: CacheSettings) -> Self {
        Self {
            client,
            settings,
            stats: Arc::new(CacheStats::new()),
        }
    }

    /// Returns cache statistics.
    pub fn stats(&self) -> Arc<CacheStats> {
        self.stats.clone()
    }

    /// Generates the cache key for a balance.
    fn cache_key(&self, account_id: Uuid, currency: &str) -> String {
        format!(
            "{}:balance:{}:{}",
            self.settings.key_prefix, account_id, currency
        )
    }

    /// Gets a balance from cache.
    pub async fn get(&self, account_id: Uuid, currency: &str) -> Result<Option<AccountBalance>> {
        if !self.settings.enabled {
            return Ok(None);
        }

        let key = self.cache_key(account_id, currency);
        let start = std::time::Instant::now();

        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis connection error in cache get: {}", e);
                return Ok(None);
            }
        };

        let result: Option<String> = match conn.get(&key).await {
            Ok(v) => v,
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis get error: {}", e);
                return Ok(None);
            }
        };

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Some(json) => {
                match serde_json::from_str::<CachedBalance>(&json) {
                    Ok(cached) => {
                        self.stats.record_hit();
                        get_metrics().record_balance_query_latency(duration_ms, true);
                        tracing::debug!(
                            account_id = %account_id,
                            currency = %currency,
                            "Cache hit for balance"
                        );
                        Ok(Some(cached.balance))
                    }
                    Err(e) => {
                        self.stats.record_error();
                        tracing::warn!("Failed to deserialize cached balance: {}", e);
                        self.invalidate(account_id, currency).await?;
                        Ok(None)
                    }
                }
            }
            None => {
                self.stats.record_miss();
                get_metrics().record_balance_query_latency(duration_ms, false);
                Ok(None)
            }
        }
    }

    /// Sets a balance in cache.
    pub async fn set(&self, balance: &AccountBalance) -> Result<()> {
        if !self.settings.enabled {
            return Ok(());
        }

        let key = self.cache_key(balance.account_id, &balance.currency);
        let cached = CachedBalance {
            balance: balance.clone(),
            cached_at: chrono::Utc::now().timestamp(),
            version: balance.version,
        };

        let json = serde_json::to_string(&cached)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize balance: {}", e)))?;

        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis connection error in cache set: {}", e);
                return Ok(());
            }
        };

        let ttl = self.settings.balance_ttl_secs as u64;
        match conn.set_ex::<_, _, ()>(&key, json, ttl).await {
            Ok(_) => {
                tracing::debug!(
                    account_id = %balance.account_id,
                    currency = %balance.currency,
                    ttl_secs = ttl,
                    "Cached balance"
                );
                Ok(())
            }
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis set error: {}", e);
                Err(AppError::Redis(e))
            }
        }
    }

    /// Invalidates a cached balance.
    pub async fn invalidate(&self, account_id: Uuid, currency: &str) -> Result<()> {
        if !self.settings.enabled {
            return Ok(());
        }

        let key = self.cache_key(account_id, currency);

        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis connection error in cache invalidate: {}", e);
                return Ok(());
            }
        };

        if let Err(e) = conn.del::<_, ()>(&key).await {
            self.stats.record_error();
            tracing::warn!("Redis del error: {}", e);
        } else {
            self.stats.record_invalidation();
            tracing::debug!(
                account_id = %account_id,
                currency = %currency,
                "Invalidated cached balance"
            );
        }

        Ok(())
    }

    /// Invalidates all cached balances for an account.
    pub async fn invalidate_account(&self, account_id: Uuid) -> Result<()> {
        if !self.settings.enabled {
            return Ok(());
        }

        let pattern = format!("{}:balance:{}:*", self.settings.key_prefix, account_id);

        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                self.stats.record_error();
                tracing::warn!("Redis connection error in cache invalidate_account: {}", e);
                return Ok(());
            }
        };

        let mut cursor: u64 = 0;
        loop {
            let scan_result: (u64, Vec<String>) = match redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    self.stats.record_error();
                    tracing::warn!("Redis SCAN error: {}", e);
                    return Ok(());
                }
            };

            let (new_cursor, keys) = scan_result;

            for key in keys {
                if let Err(e) = conn.del::<_, ()>(&key).await {
                    self.stats.record_error();
                    tracing::warn!("Redis del error for key {}: {}", key, e);
                } else {
                    self.stats.record_invalidation();
                }
            }

            cursor = new_cursor;
            if cursor == 0 {
                break;
            }
        }

        tracing::debug!(
            account_id = %account_id,
            "Invalidated all cached balances for account"
        );

        Ok(())
    }

    /// Warms the cache with a list of balances.
    pub async fn warm(&self, balances: &[AccountBalance]) -> Result<usize> {
        if !self.settings.enabled {
            return Ok(0);
        }

        let mut count = 0;
        for balance in balances {
            match self.set(balance).await {
                Ok(_) => count += 1,
                Err(e) => {
                    tracing::debug!("Cache warm failed for balance: {}", e);
                }
            }
        }

        tracing::info!(count = count, "Warmed balance cache");
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_stats() {
        let stats = CacheStats::new();
        
        assert_eq!(stats.get_hits(), 0);
        assert_eq!(stats.get_misses(), 0);
        assert_eq!(stats.hit_rate(), 0.0);

        stats.record_hit();
        stats.record_hit();
        stats.record_miss();

        assert_eq!(stats.get_hits(), 2);
        assert_eq!(stats.get_misses(), 1);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_cache_key_format() {
        let settings = CacheSettings {
            enabled: true,
            balance_ttl_secs: 60,
            key_prefix: "test".to_string(),
        };
        let client = redis::Client::open("redis://localhost:6379").unwrap();
        let cache = BalanceCache::new(client, settings);

        let account_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let key = cache.cache_key(account_id, "USD");

        assert_eq!(key, "test:balance:550e8400-e29b-41d4-a716-446655440000:USD");
    }
}
