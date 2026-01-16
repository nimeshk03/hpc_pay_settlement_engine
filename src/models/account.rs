use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Account types following double-entry bookkeeping principles.
/// Each type has a "normal balance" that determines how debits and credits affect it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "account_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountType {
    /// Assets: Resources owned. Normal balance is DEBIT.
    /// Debits increase, Credits decrease.
    Asset,
    /// Liabilities: Amounts owed. Normal balance is CREDIT.
    /// Credits increase, Debits decrease.
    Liability,
    /// Revenue: Income earned. Normal balance is CREDIT.
    /// Credits increase, Debits decrease.
    Revenue,
    /// Expenses: Costs incurred. Normal balance is DEBIT.
    /// Debits increase, Credits decrease.
    Expense,
}

impl AccountType {
    /// Returns true if the account type has a normal debit balance.
    pub fn is_debit_normal(&self) -> bool {
        matches!(self, AccountType::Asset | AccountType::Expense)
    }

    /// Returns true if the account type has a normal credit balance.
    pub fn is_credit_normal(&self) -> bool {
        matches!(self, AccountType::Liability | AccountType::Revenue)
    }
}

/// Account status indicating the operational state of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "account_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountStatus {
    /// Account is active and can participate in transactions.
    Active,
    /// Account is frozen and cannot participate in new transactions.
    Frozen,
    /// Account is closed and permanently inactive.
    Closed,
}

impl AccountStatus {
    /// Returns true if the account can participate in transactions.
    pub fn is_operational(&self) -> bool {
        matches!(self, AccountStatus::Active)
    }
}

/// Represents a financial account in the settlement system.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub external_id: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub account_type: AccountType,
    pub status: AccountStatus,
    pub currency: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Account {
    /// Creates a new Account with the given parameters.
    pub fn new(
        external_id: String,
        name: String,
        account_type: AccountType,
        currency: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            external_id,
            name,
            account_type,
            status: AccountStatus::Active,
            currency,
            metadata: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates a new Account with metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Checks if the account can be debited (used as source).
    pub fn can_be_debited(&self) -> bool {
        self.status.is_operational()
    }

    /// Checks if the account can be credited (used as destination).
    pub fn can_be_credited(&self) -> bool {
        self.status.is_operational()
    }

    /// Freezes the account, preventing new transactions.
    pub fn freeze(&mut self) {
        self.status = AccountStatus::Frozen;
        self.updated_at = Utc::now();
    }

    /// Closes the account permanently.
    pub fn close(&mut self) {
        self.status = AccountStatus::Closed;
        self.updated_at = Utc::now();
    }

    /// Reactivates a frozen account.
    pub fn activate(&mut self) {
        if self.status == AccountStatus::Frozen {
            self.status = AccountStatus::Active;
            self.updated_at = Utc::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_type_normal_balance() {
        assert!(AccountType::Asset.is_debit_normal());
        assert!(AccountType::Expense.is_debit_normal());
        assert!(AccountType::Liability.is_credit_normal());
        assert!(AccountType::Revenue.is_credit_normal());
    }

    #[test]
    fn test_account_status_operational() {
        assert!(AccountStatus::Active.is_operational());
        assert!(!AccountStatus::Frozen.is_operational());
        assert!(!AccountStatus::Closed.is_operational());
    }

    #[test]
    fn test_account_creation() {
        let account = Account::new(
            "EXT-001".to_string(),
            "Test Account".to_string(),
            AccountType::Asset,
            "USD".to_string(),
        );

        assert_eq!(account.external_id, "EXT-001");
        assert_eq!(account.name, "Test Account");
        assert_eq!(account.account_type, AccountType::Asset);
        assert_eq!(account.status, AccountStatus::Active);
        assert_eq!(account.currency, "USD");
        assert!(account.metadata.is_none());
    }

    #[test]
    fn test_account_with_metadata() {
        let metadata = serde_json::json!({"owner": "John Doe"});
        let account = Account::new(
            "EXT-001".to_string(),
            "Test Account".to_string(),
            AccountType::Asset,
            "USD".to_string(),
        )
        .with_metadata(metadata.clone());

        assert_eq!(account.metadata, Some(metadata));
    }

    #[test]
    fn test_account_freeze_and_activate() {
        let mut account = Account::new(
            "EXT-001".to_string(),
            "Test Account".to_string(),
            AccountType::Asset,
            "USD".to_string(),
        );

        assert!(account.can_be_debited());
        assert!(account.can_be_credited());

        account.freeze();
        assert_eq!(account.status, AccountStatus::Frozen);
        assert!(!account.can_be_debited());

        account.activate();
        assert_eq!(account.status, AccountStatus::Active);
        assert!(account.can_be_debited());
    }

    #[test]
    fn test_account_close() {
        let mut account = Account::new(
            "EXT-001".to_string(),
            "Test Account".to_string(),
            AccountType::Asset,
            "USD".to_string(),
        );

        account.close();
        assert_eq!(account.status, AccountStatus::Closed);
        assert!(!account.can_be_debited());

        // Activate should not work on closed accounts
        account.activate();
        assert_eq!(account.status, AccountStatus::Closed);
    }

    #[test]
    fn test_account_serialization() {
        let account = Account::new(
            "EXT-001".to_string(),
            "Test Account".to_string(),
            AccountType::Asset,
            "USD".to_string(),
        );

        let json = serde_json::to_string(&account).unwrap();
        let deserialized: Account = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.external_id, account.external_id);
        assert_eq!(deserialized.account_type, account.account_type);
    }
}
