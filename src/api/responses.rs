use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{
    Account, AccountBalance, AccountStatus, AccountType, BatchStatus, LedgerEntry,
    SettlementBatch, TransactionRecord, TransactionStatus, TransactionType,
};

/// Standard API response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ErrorResponse>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(error: ErrorResponse) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(error),
        }
    }
}

/// Error response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<Vec<ValidationErrorDetail>>,
}

impl ErrorResponse {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Vec<ValidationErrorDetail>) -> Self {
        self.details = Some(details);
        self
    }
}

/// Validation error detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrorDetail {
    pub field: String,
    pub message: String,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub timestamp: DateTime<Utc>,
    pub services: ServiceHealth,
}

/// Service health status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealth {
    pub database: bool,
    pub redis: bool,
    pub kafka: bool,
}

/// Account response DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountResponse {
    pub id: Uuid,
    pub external_id: String,
    pub name: String,
    pub account_type: AccountType,
    pub status: AccountStatus,
    pub currency: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Account> for AccountResponse {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            external_id: account.external_id,
            name: account.name,
            account_type: account.account_type,
            status: account.status,
            currency: account.currency,
            metadata: account.metadata,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

/// Balance response DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub account_id: Uuid,
    pub currency: String,
    pub available_balance: Decimal,
    pub pending_balance: Decimal,
    pub reserved_balance: Decimal,
    pub total_balance: Decimal,
    pub last_updated: DateTime<Utc>,
}

impl From<AccountBalance> for BalanceResponse {
    fn from(balance: AccountBalance) -> Self {
        Self {
            account_id: balance.account_id,
            currency: balance.currency,
            available_balance: balance.available_balance,
            pending_balance: balance.pending_balance,
            reserved_balance: balance.reserved_balance,
            total_balance: balance.available_balance + balance.pending_balance,
            last_updated: balance.last_updated,
        }
    }
}

/// Transaction response DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub id: Uuid,
    pub external_id: String,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub source_account_id: Uuid,
    pub destination_account_id: Uuid,
    pub amount: Decimal,
    pub currency: String,
    pub fee_amount: Decimal,
    pub net_amount: Decimal,
    pub settlement_batch_id: Option<Uuid>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl From<TransactionRecord> for TransactionResponse {
    fn from(tx: TransactionRecord) -> Self {
        Self {
            id: tx.id,
            external_id: tx.external_id,
            transaction_type: tx.transaction_type,
            status: tx.status,
            source_account_id: tx.source_account_id,
            destination_account_id: tx.destination_account_id,
            amount: tx.amount,
            currency: tx.currency,
            fee_amount: tx.fee_amount,
            net_amount: tx.net_amount,
            settlement_batch_id: tx.settlement_batch_id,
            metadata: tx.metadata,
            created_at: tx.created_at,
            settled_at: tx.settled_at,
        }
    }
}

/// Batch response DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    pub id: Uuid,
    pub status: BatchStatus,
    pub currency: String,
    pub settlement_date: chrono::NaiveDate,
    pub total_transactions: i32,
    pub gross_amount: Decimal,
    pub net_amount: Decimal,
    pub fee_amount: Decimal,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<SettlementBatch> for BatchResponse {
    fn from(batch: SettlementBatch) -> Self {
        Self {
            id: batch.id,
            status: batch.status,
            currency: batch.currency,
            settlement_date: batch.settlement_date,
            total_transactions: batch.total_transactions,
            gross_amount: batch.gross_amount,
            net_amount: batch.net_amount,
            fee_amount: batch.fee_amount,
            created_at: batch.created_at,
            completed_at: batch.completed_at,
        }
    }
}

/// Ledger entry response DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntryResponse {
    pub id: Uuid,
    pub account_id: Uuid,
    pub transaction_id: Uuid,
    pub entry_type: String,
    pub amount: Decimal,
    pub balance_after: Decimal,
    pub currency: String,
    pub effective_date: chrono::NaiveDate,
    pub created_at: DateTime<Utc>,
}

impl From<LedgerEntry> for LedgerEntryResponse {
    fn from(entry: LedgerEntry) -> Self {
        Self {
            id: entry.id,
            account_id: entry.account_id,
            transaction_id: entry.transaction_id,
            entry_type: format!("{:?}", entry.entry_type),
            amount: entry.amount,
            balance_after: entry.balance_after,
            currency: entry.currency,
            effective_date: entry.effective_date,
            created_at: entry.created_at,
        }
    }
}

/// Paginated list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl<T> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        Self {
            items,
            total,
            limit,
            offset,
        }
    }
}
