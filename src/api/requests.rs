use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{AccountType, TransactionType};

/// Request to create a new account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountRequest {
    pub external_id: String,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub initial_balance: Option<Decimal>,
    pub metadata: Option<serde_json::Value>,
}

impl CreateAccountRequest {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        if self.external_id.trim().is_empty() {
            errors.push(ValidationError { field: "external_id".to_string(), message: "external_id cannot be empty".to_string() });
        }
        if self.name.trim().is_empty() {
            errors.push(ValidationError { field: "name".to_string(), message: "name cannot be empty".to_string() });
        }
        if self.currency.len() != 3 {
            errors.push(ValidationError { field: "currency".to_string(), message: "currency must be a 3-letter ISO 4217 code".to_string() });
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

/// Request to create a new transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTransactionRequest {
    pub external_id: String,
    pub transaction_type: TransactionType,
    pub source_account_id: Uuid,
    pub destination_account_id: Uuid,
    pub amount: Decimal,
    pub currency: String,
    pub fee_amount: Option<Decimal>,
    pub idempotency_key: String,
    pub metadata: Option<serde_json::Value>,
}

impl CreateTransactionRequest {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        if self.external_id.trim().is_empty() {
            errors.push(ValidationError { field: "external_id".to_string(), message: "external_id cannot be empty".to_string() });
        }
        if self.currency.len() != 3 {
            errors.push(ValidationError { field: "currency".to_string(), message: "currency must be a 3-letter ISO 4217 code".to_string() });
        }
        if self.amount <= Decimal::ZERO {
            errors.push(ValidationError { field: "amount".to_string(), message: "amount must be positive".to_string() });
        }
        if self.idempotency_key.trim().is_empty() {
            errors.push(ValidationError { field: "idempotency_key".to_string(), message: "idempotency_key cannot be empty".to_string() });
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Request to reverse a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseTransactionRequest {
    pub reason: String,
    pub idempotency_key: String,
}

impl ReverseTransactionRequest {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        if self.reason.trim().is_empty() {
            errors.push(ValidationError { field: "reason".to_string(), message: "reason cannot be empty".to_string() });
        }
        if self.idempotency_key.trim().is_empty() {
            errors.push(ValidationError { field: "idempotency_key".to_string(), message: "idempotency_key cannot be empty".to_string() });
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Query parameters for listing transactions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListTransactionsQuery {
    pub account_id: Option<Uuid>,
    pub status: Option<String>,
    pub currency: Option<String>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Query parameters for listing batches.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListBatchesQuery {
    pub status: Option<String>,
    pub currency: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Query parameters for listing ledger entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListLedgerEntriesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Request to process a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessBatchRequest {
    pub force: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_create_account_request_validation() {
        let valid_request = CreateAccountRequest {
            external_id: "ACC001".to_string(),
            name: "Test Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(100.00)),
            metadata: None,
        };
        assert!(valid_request.validate().is_ok());

        let invalid_request = CreateAccountRequest {
            external_id: "".to_string(),
            name: "Test Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: None,
            metadata: None,
        };
        assert!(invalid_request.validate().is_err());
    }

    #[test]
    fn test_create_transaction_request_validation() {
        let valid_request = CreateTransactionRequest {
            external_id: "TXN001".to_string(),
            transaction_type: TransactionType::Payment,
            source_account_id: Uuid::new_v4(),
            destination_account_id: Uuid::new_v4(),
            amount: dec!(100.00),
            currency: "USD".to_string(),
            fee_amount: Some(dec!(1.00)),
            idempotency_key: "key123".to_string(),
            metadata: None,
        };
        assert!(valid_request.validate().is_ok());

        let invalid_currency = CreateTransactionRequest {
            external_id: "TXN001".to_string(),
            transaction_type: TransactionType::Payment,
            source_account_id: Uuid::new_v4(),
            destination_account_id: Uuid::new_v4(),
            amount: dec!(100.00),
            currency: "US".to_string(),
            fee_amount: None,
            idempotency_key: "key123".to_string(),
            metadata: None,
        };
        assert!(invalid_currency.validate().is_err());
    }
}
