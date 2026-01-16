use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represents the balance of an account in a specific currency.
/// Uses optimistic locking via the `version` field to handle concurrent updates.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AccountBalance {
    pub account_id: Uuid,
    pub currency: String,
    /// Balance available for immediate use in transactions.
    pub available_balance: Decimal,
    /// Balance that is pending settlement (not yet available).
    pub pending_balance: Decimal,
    /// Balance reserved for pending operations (e.g., holds).
    pub reserved_balance: Decimal,
    /// Version number for optimistic locking.
    pub version: i32,
    pub last_updated: DateTime<Utc>,
}

impl AccountBalance {
    /// Creates a new AccountBalance with zero balances.
    pub fn new(account_id: Uuid, currency: String) -> Self {
        Self {
            account_id,
            currency,
            available_balance: Decimal::ZERO,
            pending_balance: Decimal::ZERO,
            reserved_balance: Decimal::ZERO,
            version: 1,
            last_updated: Utc::now(),
        }
    }

    /// Creates a new AccountBalance with an initial available balance.
    pub fn with_available_balance(account_id: Uuid, currency: String, amount: Decimal) -> Self {
        Self {
            account_id,
            currency,
            available_balance: amount,
            pending_balance: Decimal::ZERO,
            reserved_balance: Decimal::ZERO,
            version: 1,
            last_updated: Utc::now(),
        }
    }

    /// Returns the total balance (available + pending + reserved).
    pub fn total_balance(&self) -> Decimal {
        self.available_balance + self.pending_balance + self.reserved_balance
    }

    /// Returns the usable balance (available - reserved).
    pub fn usable_balance(&self) -> Decimal {
        self.available_balance - self.reserved_balance
    }

    /// Checks if there are sufficient funds for a given amount.
    pub fn has_sufficient_funds(&self, amount: Decimal) -> bool {
        self.usable_balance() >= amount
    }

    /// Credits the available balance (increases it).
    pub fn credit(&mut self, amount: Decimal) {
        self.available_balance += amount;
        self.version += 1;
        self.last_updated = Utc::now();
    }

    /// Debits the available balance (decreases it).
    /// Returns an error if insufficient funds.
    pub fn debit(&mut self, amount: Decimal) -> Result<(), InsufficientFundsError> {
        if !self.has_sufficient_funds(amount) {
            return Err(InsufficientFundsError {
                requested: amount,
                available: self.usable_balance(),
            });
        }
        self.available_balance -= amount;
        self.version += 1;
        self.last_updated = Utc::now();
        Ok(())
    }

    /// Reserves an amount from available balance.
    pub fn reserve(&mut self, amount: Decimal) -> Result<(), InsufficientFundsError> {
        if self.available_balance < amount {
            return Err(InsufficientFundsError {
                requested: amount,
                available: self.available_balance,
            });
        }
        self.available_balance -= amount;
        self.reserved_balance += amount;
        self.version += 1;
        self.last_updated = Utc::now();
        Ok(())
    }

    /// Releases a reserved amount back to available balance.
    pub fn release_reservation(&mut self, amount: Decimal) {
        let release_amount = amount.min(self.reserved_balance);
        self.reserved_balance -= release_amount;
        self.available_balance += release_amount;
        self.version += 1;
        self.last_updated = Utc::now();
    }

    /// Moves an amount from available to pending.
    pub fn move_to_pending(&mut self, amount: Decimal) -> Result<(), InsufficientFundsError> {
        if self.available_balance < amount {
            return Err(InsufficientFundsError {
                requested: amount,
                available: self.available_balance,
            });
        }
        self.available_balance -= amount;
        self.pending_balance += amount;
        self.version += 1;
        self.last_updated = Utc::now();
        Ok(())
    }

    /// Settles pending balance to available.
    pub fn settle_pending(&mut self, amount: Decimal) {
        let settle_amount = amount.min(self.pending_balance);
        self.pending_balance -= settle_amount;
        self.available_balance += settle_amount;
        self.version += 1;
        self.last_updated = Utc::now();
    }
}

#[derive(Debug, Clone)]
pub struct InsufficientFundsError {
    pub requested: Decimal,
    pub available: Decimal,
}

impl std::fmt::Display for InsufficientFundsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Insufficient funds: requested {}, available {}",
            self.requested, self.available
        )
    }
}

impl std::error::Error for InsufficientFundsError {}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_new_balance() {
        let balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
        assert_eq!(balance.available_balance, Decimal::ZERO);
        assert_eq!(balance.pending_balance, Decimal::ZERO);
        assert_eq!(balance.reserved_balance, Decimal::ZERO);
        assert_eq!(balance.version, 1);
    }

    #[test]
    fn test_with_available_balance() {
        let balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(1000));
        assert_eq!(balance.available_balance, dec!(1000));
    }

    #[test]
    fn test_total_balance() {
        let mut balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
        balance.available_balance = dec!(100);
        balance.pending_balance = dec!(50);
        balance.reserved_balance = dec!(25);
        assert_eq!(balance.total_balance(), dec!(175));
    }

    #[test]
    fn test_usable_balance() {
        let mut balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
        balance.available_balance = dec!(100);
        balance.reserved_balance = dec!(25);
        assert_eq!(balance.usable_balance(), dec!(75));
    }

    #[test]
    fn test_credit() {
        let mut balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
        balance.credit(dec!(100));
        assert_eq!(balance.available_balance, dec!(100));
        assert_eq!(balance.version, 2);
    }

    #[test]
    fn test_debit_success() {
        let mut balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100));
        let result = balance.debit(dec!(50));
        assert!(result.is_ok());
        assert_eq!(balance.available_balance, dec!(50));
    }

    #[test]
    fn test_debit_insufficient_funds() {
        let mut balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100));
        let result = balance.debit(dec!(150));
        assert!(result.is_err());
        assert_eq!(balance.available_balance, dec!(100)); // Unchanged
    }

    #[test]
    fn test_reserve_and_release() {
        let mut balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100));

        // Reserve
        let result = balance.reserve(dec!(30));
        assert!(result.is_ok());
        assert_eq!(balance.available_balance, dec!(70));
        assert_eq!(balance.reserved_balance, dec!(30));

        // Release
        balance.release_reservation(dec!(20));
        assert_eq!(balance.available_balance, dec!(90));
        assert_eq!(balance.reserved_balance, dec!(10));
    }

    #[test]
    fn test_pending_operations() {
        let mut balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100));

        // Move to pending
        let result = balance.move_to_pending(dec!(40));
        assert!(result.is_ok());
        assert_eq!(balance.available_balance, dec!(60));
        assert_eq!(balance.pending_balance, dec!(40));

        // Settle pending
        balance.settle_pending(dec!(25));
        assert_eq!(balance.available_balance, dec!(85));
        assert_eq!(balance.pending_balance, dec!(15));
    }

    #[test]
    fn test_has_sufficient_funds_with_reservation() {
        let mut balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100));
        balance.reserved_balance = dec!(30);

        assert!(balance.has_sufficient_funds(dec!(70)));
        assert!(!balance.has_sufficient_funds(dec!(71)));
    }

    #[test]
    fn test_decimal_precision() {
        let mut balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
        balance.credit(dec!(0.0001));
        balance.credit(dec!(0.0002));
        assert_eq!(balance.available_balance, dec!(0.0003));
    }

    #[test]
    fn test_serialization() {
        let balance =
            AccountBalance::with_available_balance(Uuid::new_v4(), "USD".to_string(), dec!(100.50));
        let json = serde_json::to_string(&balance).unwrap();
        let deserialized: AccountBalance = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.available_balance, dec!(100.50));
    }
}
