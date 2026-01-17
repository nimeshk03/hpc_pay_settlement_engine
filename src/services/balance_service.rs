use crate::error::{AppError, Result};
use crate::models::AccountBalance;
use crate::repositories::BalanceRepository;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Balance snapshot for a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub account_id: Uuid,
    pub currency: String,
    pub available_balance: Decimal,
    pub pending_balance: Decimal,
    pub reserved_balance: Decimal,
    pub total_balance: Decimal,
    pub usable_balance: Decimal,
    pub snapshot_at: DateTime<Utc>,
    pub version: i32,
}

impl From<AccountBalance> for BalanceSnapshot {
    fn from(balance: AccountBalance) -> Self {
        Self {
            account_id: balance.account_id,
            currency: balance.currency.clone(),
            available_balance: balance.available_balance,
            pending_balance: balance.pending_balance,
            reserved_balance: balance.reserved_balance,
            total_balance: balance.total_balance(),
            usable_balance: balance.usable_balance(),
            snapshot_at: Utc::now(),
            version: balance.version,
        }
    }
}

/// Service for balance management operations.
pub struct BalanceService {
    balance_repo: BalanceRepository,
}

impl BalanceService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            balance_repo: BalanceRepository::new(pool),
        }
    }

    /// Gets the current balance for an account/currency pair.
    pub async fn get_balance(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<AccountBalance> {
        self.balance_repo
            .find_by_account_and_currency(account_id, currency)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Balance for account '{}' in '{}' not found",
                    account_id, currency
                ))
            })
    }

    /// Gets or creates a balance for an account/currency pair.
    pub async fn get_or_create_balance(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<AccountBalance> {
        self.balance_repo.get_or_create(account_id, currency).await
    }

    /// Gets all balances for an account.
    pub async fn get_all_balances(&self, account_id: Uuid) -> Result<Vec<AccountBalance>> {
        self.balance_repo.find_by_account(account_id).await
    }

    /// Creates a snapshot of the current balance.
    pub async fn create_snapshot(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<BalanceSnapshot> {
        let balance = self.get_balance(account_id, currency).await?;
        Ok(BalanceSnapshot::from(balance))
    }

    /// Credits an account balance.
    pub async fn credit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Credit amount must be positive".to_string()));
        }

        self.balance_repo.credit(account_id, currency, amount).await
    }

    /// Debits an account balance.
    pub async fn debit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        if amount <= Decimal::ZERO {
            return Err(AppError::Validation("Debit amount must be positive".to_string()));
        }

        self.balance_repo.debit(account_id, currency, amount).await
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

        self.balance_repo.reserve(account_id, currency, amount).await
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

        self.balance_repo
            .release_reservation(account_id, currency, amount)
            .await
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

        self.balance_repo
            .move_to_pending(account_id, currency, amount)
            .await
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

        self.balance_repo
            .settle_pending(account_id, currency, amount)
            .await
    }

    /// Updates balance with optimistic locking.
    /// Returns error if version mismatch (concurrent modification).
    pub async fn update_with_optimistic_lock(
        &self,
        balance: &AccountBalance,
    ) -> Result<AccountBalance> {
        self.balance_repo
            .update_with_version(balance)
            .await?
            .ok_or_else(|| {
                AppError::Validation(
                    "Concurrent modification detected. Please retry the operation.".to_string(),
                )
            })
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
        if !self.has_sufficient_funds(account_id, currency, amount).await? {
            let balance = self.get_balance(account_id, currency).await?;
            return Err(AppError::Validation(format!(
                "Insufficient funds: requested {}, available {}",
                amount,
                balance.usable_balance()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_snapshot_from_account_balance() {
        let balance = AccountBalance::with_available_balance(
            Uuid::new_v4(),
            "USD".to_string(),
            Decimal::from(1000),
        );

        let snapshot = BalanceSnapshot::from(balance.clone());

        assert_eq!(snapshot.account_id, balance.account_id);
        assert_eq!(snapshot.available_balance, Decimal::from(1000));
        assert_eq!(snapshot.total_balance, Decimal::from(1000));
        assert_eq!(snapshot.usable_balance, Decimal::from(1000));
    }
}
