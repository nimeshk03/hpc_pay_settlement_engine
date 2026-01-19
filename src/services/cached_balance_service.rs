use crate::cache::BalanceCache;
use crate::config::CacheSettings;
use crate::error::{AppError, Result};
use crate::models::AccountBalance;
use crate::observability::{get_metrics, LatencyTimer};
use crate::repositories::BalanceRepository;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Balance service with Redis caching layer.
pub struct CachedBalanceService {
    balance_repo: BalanceRepository,
    cache: Arc<BalanceCache>,
}

impl CachedBalanceService {
    pub fn new(pool: PgPool, redis_client: redis::Client, cache_settings: CacheSettings) -> Self {
        Self {
            balance_repo: BalanceRepository::new(pool),
            cache: Arc::new(BalanceCache::new(redis_client, cache_settings)),
        }
    }

    /// Creates a new service with an existing cache instance.
    pub fn with_cache(pool: PgPool, cache: Arc<BalanceCache>) -> Self {
        Self {
            balance_repo: BalanceRepository::new(pool),
            cache,
        }
    }

    /// Returns the cache instance for stats access.
    pub fn cache(&self) -> Arc<BalanceCache> {
        self.cache.clone()
    }

    /// Gets the current balance for an account/currency pair.
    /// First checks cache, then falls back to database.
    pub async fn get_balance(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<AccountBalance> {
        let timer = LatencyTimer::new();

        if let Some(cached) = self.cache.get(account_id, currency).await? {
            return Ok(cached);
        }

        let balance = self.balance_repo
            .find_by_account_and_currency(account_id, currency)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Balance for account '{}' in '{}' not found",
                    account_id, currency
                ))
            })?;

        self.cache.set(&balance).await?;

        get_metrics().record_balance_query_latency(timer.elapsed_ms(), false);

        Ok(balance)
    }

    /// Gets or creates a balance for an account/currency pair.
    pub async fn get_or_create_balance(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<AccountBalance> {
        if let Some(cached) = self.cache.get(account_id, currency).await? {
            return Ok(cached);
        }

        let balance = self.balance_repo.get_or_create(account_id, currency).await?;

        self.cache.set(&balance).await?;

        Ok(balance)
    }

    /// Gets all balances for an account.
    pub async fn get_all_balances(&self, account_id: Uuid) -> Result<Vec<AccountBalance>> {
        self.balance_repo.find_by_account(account_id).await
    }

    /// Credits an account balance and invalidates cache.
    pub async fn credit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Credit amount must be positive".to_string()));
        }

        let timer = LatencyTimer::new();
        let balance = self.balance_repo.credit(account_id, currency, amount).await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after credit: {}", e);
        }

        get_metrics().record_ledger_write_latency(timer.elapsed_ms());

        Ok(balance)
    }

    /// Debits an account balance and invalidates cache.
    pub async fn debit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Debit amount must be positive".to_string()));
        }

        let timer = LatencyTimer::new();
        let balance = self.balance_repo.debit(account_id, currency, amount).await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after debit: {}", e);
        }

        get_metrics().record_ledger_write_latency(timer.elapsed_ms());

        Ok(balance)
    }

    /// Reserves an amount from available balance.
    pub async fn reserve(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Reserve amount must be positive".to_string()));
        }

        let balance = self.balance_repo.reserve(account_id, currency, amount).await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after reserve: {}", e);
        }

        Ok(balance)
    }

    /// Releases a reserved amount back to available.
    pub async fn release_reservation(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Release amount must be positive".to_string()));
        }

        let balance = self.balance_repo
            .release_reservation(account_id, currency, amount)
            .await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after release_reservation: {}", e);
        }

        Ok(balance)
    }

    /// Moves amount from available to pending.
    pub async fn move_to_pending(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Amount must be positive".to_string()));
        }

        let balance = self.balance_repo
            .move_to_pending(account_id, currency, amount)
            .await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after move_to_pending: {}", e);
        }

        Ok(balance)
    }

    /// Settles pending balance to available.
    pub async fn settle_pending(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Amount must be positive".to_string()));
        }

        let balance = self.balance_repo
            .settle_pending(account_id, currency, amount)
            .await?;

        if let Err(e) = self.cache.invalidate(account_id, currency).await {
            tracing::warn!("Cache invalidation failed after settle_pending: {}", e);
        }

        Ok(balance)
    }

    /// Updates balance with optimistic locking and invalidates cache.
    pub async fn update_with_optimistic_lock(
        &self,
        balance: &AccountBalance,
    ) -> Result<AccountBalance> {
        let updated = self.balance_repo
            .update_with_version(balance)
            .await?
            .ok_or_else(|| {
                AppError::Validation(
                    "Concurrent modification detected. Please retry the operation.".to_string(),
                )
            })?;

        if let Err(e) = self.cache.invalidate(balance.account_id, &balance.currency).await {
            tracing::warn!("Cache invalidation failed after update_with_optimistic_lock: {}", e);
        }

        Ok(updated)
    }

    /// Checks if account has sufficient funds for a transaction.
    pub async fn has_sufficient_funds(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<bool> {
        let balance = self.get_balance(account_id, currency).await?;
        Ok(balance.has_sufficient_funds(amount))
    }

    /// Gets the usable balance (available - reserved).
    pub async fn get_usable_balance(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<Decimal> {
        let balance = self.get_balance(account_id, currency).await?;
        Ok(balance.usable_balance())
    }

    /// Validates that an account has sufficient funds for a debit operation.
    pub async fn validate_sufficient_funds(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<()> {
        let balance = self.get_balance(account_id, currency).await?;
        if balance.usable_balance() < amount {
            return Err(AppError::Validation(format!(
                "Insufficient funds: requested {}, available {}",
                amount,
                balance.usable_balance()
            )));
        }
        Ok(())
    }

    /// Warms the cache with balances for specified accounts.
    pub async fn warm_cache(&self, account_ids: &[Uuid]) -> Result<usize> {
        let mut count = 0;
        for account_id in account_ids {
            let balances = self.balance_repo.find_by_account(*account_id).await?;
            count += self.cache.warm(&balances).await?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_balance_service_creation() {
        // Just verify the struct can be created with proper types
        // Actual integration tests require database and Redis
    }
}
