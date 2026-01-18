pub mod account_repository;
pub mod balance_repository;
pub mod batch_repository;
pub mod ledger_repository;
pub mod netting_repository;
pub mod transaction_repository;

pub use account_repository::AccountRepository;
pub use balance_repository::BalanceRepository;
pub use batch_repository::BatchRepository;
pub use ledger_repository::LedgerRepository;
pub use netting_repository::{BatchNettingSummary, NettingRepository};
pub use transaction_repository::TransactionRepository;

use sqlx::PgPool;

/// Database connection pool type alias.
pub type DbPool = PgPool;
