use crate::error::{AppError, Result};
use crate::models::{Account, AccountBalance, AccountStatus, AccountType};
use crate::repositories::{AccountRepository, BalanceRepository};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Request to create a new account.
#[derive(Debug, Clone)]
pub struct CreateAccountRequest {
    pub external_id: String,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub initial_balance: Option<Decimal>,
    pub metadata: Option<serde_json::Value>,
}

/// Service for account management operations.
pub struct AccountService {
    account_repo: AccountRepository,
    balance_repo: BalanceRepository,
}

impl AccountService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            account_repo: AccountRepository::new(pool.clone()),
            balance_repo: BalanceRepository::new(pool),
        }
    }

    /// Creates a new account with validation.
    pub async fn create_account(&self, request: CreateAccountRequest) -> Result<Account> {
        // Validate external_id is not empty
        if request.external_id.trim().is_empty() {
            return Err(AppError::Validation("External ID cannot be empty".to_string()));
        }

        // Validate name is not empty
        if request.name.trim().is_empty() {
            return Err(AppError::Validation("Account name cannot be empty".to_string()));
        }

        // Validate currency code (basic validation)
        if request.currency.len() != 3 {
            return Err(AppError::Validation(
                "Currency must be a 3-letter ISO 4217 code".to_string(),
            ));
        }

        // Check if external_id already exists
        if self.account_repo.exists_by_external_id(&request.external_id).await? {
            return Err(AppError::Validation(format!(
                "Account with external_id '{}' already exists",
                request.external_id
            )));
        }

        // Create the account
        let mut account = Account::new(
            request.external_id,
            request.name,
            request.account_type,
            request.currency.clone(),
        );

        if let Some(metadata) = request.metadata {
            account = account.with_metadata(metadata);
        }

        let created_account = self.account_repo.create(&account).await?;

        // Create initial balance if specified
        let initial_balance = request.initial_balance.unwrap_or(Decimal::ZERO);
        let balance = AccountBalance::with_available_balance(
            created_account.id,
            request.currency,
            initial_balance,
        );
        self.balance_repo.create(&balance).await?;

        Ok(created_account)
    }

    /// Finds an account by its UUID.
    pub async fn find_by_id(&self, id: Uuid) -> Result<Account> {
        self.account_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account with id '{}' not found", id)))
    }

    /// Finds an account by its external reference.
    pub async fn find_by_external_id(&self, external_id: &str) -> Result<Account> {
        self.account_repo
            .find_by_external_id(external_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Account with external_id '{}' not found",
                    external_id
                ))
            })
    }

    /// Lists accounts with optional filters.
    pub async fn list_accounts(
        &self,
        account_type: Option<AccountType>,
        status: Option<AccountStatus>,
        currency: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Account>> {
        self.account_repo
            .list(account_type, status, currency, limit, offset)
            .await
    }

    /// Freezes an account, preventing new transactions.
    pub async fn freeze_account(&self, id: Uuid) -> Result<Account> {
        let account = self.find_by_id(id).await?;

        if account.status == AccountStatus::Closed {
            return Err(AppError::Validation("Cannot freeze a closed account".to_string()));
        }

        if account.status == AccountStatus::Frozen {
            return Err(AppError::Validation("Account is already frozen".to_string()));
        }

        self.account_repo
            .update_status(id, AccountStatus::Frozen)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account with id '{}' not found", id)))
    }

    /// Activates a frozen account.
    pub async fn activate_account(&self, id: Uuid) -> Result<Account> {
        let account = self.find_by_id(id).await?;

        if account.status == AccountStatus::Closed {
            return Err(AppError::Validation("Cannot activate a closed account".to_string()));
        }

        if account.status == AccountStatus::Active {
            return Err(AppError::Validation("Account is already active".to_string()));
        }

        self.account_repo
            .update_status(id, AccountStatus::Active)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account with id '{}' not found", id)))
    }

    /// Closes an account permanently.
    pub async fn close_account(&self, id: Uuid) -> Result<Account> {
        let account = self.find_by_id(id).await?;

        if account.status == AccountStatus::Closed {
            return Err(AppError::Validation("Account is already closed".to_string()));
        }

        // Check if account has non-zero balance
        let balances = self.balance_repo.find_by_account(id).await?;
        for balance in &balances {
            if balance.total_balance() != Decimal::ZERO {
                return Err(AppError::Validation(
                    "Cannot close account with non-zero balance".to_string(),
                ));
            }
        }

        self.account_repo
            .update_status(id, AccountStatus::Closed)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account with id '{}' not found", id)))
    }

    /// Updates account metadata.
    pub async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
    ) -> Result<Account> {
        // Verify account exists
        self.find_by_id(id).await?;

        self.account_repo
            .update_metadata(id, metadata)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Account with id '{}' not found", id)))
    }

    /// Gets all balances for an account.
    pub async fn get_balances(&self, account_id: Uuid) -> Result<Vec<AccountBalance>> {
        // Verify account exists
        self.find_by_id(account_id).await?;

        self.balance_repo.find_by_account(account_id).await
    }

    /// Gets balance for a specific currency.
    pub async fn get_balance(&self, account_id: Uuid, currency: &str) -> Result<AccountBalance> {
        // Verify account exists
        self.find_by_id(account_id).await?;

        self.balance_repo
            .find_by_account_and_currency(account_id, currency)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Balance for account '{}' in currency '{}' not found",
                    account_id, currency
                ))
            })
    }

    /// Validates that an account can participate in transactions.
    pub async fn validate_for_transaction(&self, account_id: Uuid) -> Result<Account> {
        let account = self.find_by_id(account_id).await?;

        if !account.status.is_operational() {
            return Err(AppError::Validation(format!(
                "Account '{}' is not operational (status: {:?})",
                account_id, account.status
            )));
        }

        Ok(account)
    }

    /// Counts accounts by type and status.
    pub async fn count_accounts(
        &self,
        account_type: Option<AccountType>,
        status: Option<AccountStatus>,
    ) -> Result<i64> {
        self.account_repo.count(account_type, status).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account_request() {
        let request = CreateAccountRequest {
            external_id: "EXT-001".to_string(),
            name: "Test Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(Decimal::from(1000)),
            metadata: None,
        };

        assert_eq!(request.external_id, "EXT-001");
        assert_eq!(request.account_type, AccountType::Asset);
    }
}
