mod common;

use chrono::{Duration, NaiveDate, Utc};
use rust_decimal_macros::dec;
use settlement_engine::models::{
    Account, AccountBalance, AccountStatus, AccountType, BatchStatus, EntryType, LedgerEntry,
    NettingPosition, SettlementBatch, TransactionRecord, TransactionStatus, TransactionType,
};
use settlement_engine::repositories::{
    AccountRepository, BalanceRepository, BatchRepository, LedgerRepository, NettingRepository,
    TransactionRepository,
};
use uuid::Uuid;

#[tokio::test]
async fn test_account_repository_crud() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let repo = AccountRepository::new(pool.clone());

    // Create
    let account = Account::new(
        format!("EXT-{}", Uuid::new_v4()),
        "Test Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );

    let created = repo.create(&account).await.expect("Failed to create account");
    assert_eq!(created.external_id, account.external_id);
    assert_eq!(created.account_type, AccountType::Asset);
    assert_eq!(created.status, AccountStatus::Active);

    // Find by ID
    let found = repo
        .find_by_id(created.id)
        .await
        .expect("Failed to find account")
        .expect("Account not found");
    assert_eq!(found.id, created.id);

    // Find by external ID
    let found_ext = repo
        .find_by_external_id(&created.external_id)
        .await
        .expect("Failed to find by external ID")
        .expect("Account not found");
    assert_eq!(found_ext.id, created.id);

    // Update status
    let updated = repo
        .update_status(created.id, AccountStatus::Frozen)
        .await
        .expect("Failed to update status")
        .expect("Account not found");
    assert_eq!(updated.status, AccountStatus::Frozen);

    // List
    let accounts = repo
        .list(Some(AccountType::Asset), None, None, 10, 0)
        .await
        .expect("Failed to list accounts");
    assert!(!accounts.is_empty());

    // Count
    let count = repo
        .count(Some(AccountType::Asset), None)
        .await
        .expect("Failed to count");
    assert!(count >= 1);

    // Delete (soft)
    let deleted = repo.delete(created.id).await.expect("Failed to delete");
    assert!(deleted);

    let after_delete = repo
        .find_by_id(created.id)
        .await
        .expect("Failed to find")
        .expect("Account not found");
    assert_eq!(after_delete.status, AccountStatus::Closed);

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_balance_repository_operations() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let balance_repo = BalanceRepository::new(pool.clone());

    // Create account first
    let account = Account::new(
        format!("EXT-{}", Uuid::new_v4()),
        "Balance Test Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let account = account_repo.create(&account).await.expect("Failed to create account");

    // Create balance
    let balance = AccountBalance::with_available_balance(account.id, "USD".to_string(), dec!(1000));
    let created = balance_repo.create(&balance).await.expect("Failed to create balance");
    assert_eq!(created.available_balance, dec!(1000));
    assert_eq!(created.version, 1);

    // Find by account and currency
    let found = balance_repo
        .find_by_account_and_currency(account.id, "USD")
        .await
        .expect("Failed to find balance")
        .expect("Balance not found");
    assert_eq!(found.available_balance, dec!(1000));

    // Credit
    let credited = balance_repo
        .credit(account.id, "USD", dec!(500))
        .await
        .expect("Failed to credit");
    assert_eq!(credited.available_balance, dec!(1500));
    assert_eq!(credited.version, 2);

    // Debit
    let debited = balance_repo
        .debit(account.id, "USD", dec!(200))
        .await
        .expect("Failed to debit");
    assert_eq!(debited.available_balance, dec!(1300));

    // Reserve
    let reserved = balance_repo
        .reserve(account.id, "USD", dec!(100))
        .await
        .expect("Failed to reserve");
    assert_eq!(reserved.available_balance, dec!(1200));
    assert_eq!(reserved.reserved_balance, dec!(100));

    // Release reservation
    let released = balance_repo
        .release_reservation(account.id, "USD", dec!(50))
        .await
        .expect("Failed to release");
    assert_eq!(released.available_balance, dec!(1250));
    assert_eq!(released.reserved_balance, dec!(50));

    // Debit insufficient funds should fail
    let insufficient = balance_repo.debit(account.id, "USD", dec!(10000)).await;
    assert!(insufficient.is_err());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_balance_optimistic_locking() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let balance_repo = BalanceRepository::new(pool.clone());

    let account = Account::new(
        format!("EXT-{}", Uuid::new_v4()),
        "Locking Test Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let account = account_repo.create(&account).await.expect("Failed to create account");

    let balance = AccountBalance::with_available_balance(account.id, "USD".to_string(), dec!(1000));
    let created = balance_repo.create(&balance).await.expect("Failed to create balance");

    // Update with correct version
    let mut to_update = created.clone();
    to_update.available_balance = dec!(900);
    let updated = balance_repo
        .update_with_version(&to_update)
        .await
        .expect("Failed to update");
    assert!(updated.is_some());

    // Update with stale version should return None
    let stale = balance_repo
        .update_with_version(&to_update)
        .await
        .expect("Failed to update");
    assert!(stale.is_none());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_transaction_repository_crud() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let tx_repo = TransactionRepository::new(pool.clone());

    // Create accounts
    let source = Account::new(
        format!("SRC-{}", Uuid::new_v4()),
        "Source Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let dest = Account::new(
        format!("DST-{}", Uuid::new_v4()),
        "Destination Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let source = account_repo.create(&source).await.expect("Failed to create source");
    let dest = account_repo.create(&dest).await.expect("Failed to create dest");

    // Create transaction
    let idempotency_key = format!("IDEM-{}", Uuid::new_v4());
    let tx = TransactionRecord::payment(
        format!("EXT-TX-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD".to_string(),
        dec!(2.50),
        idempotency_key.clone(),
    );

    let created = tx_repo.create(&tx).await.expect("Failed to create transaction");
    assert_eq!(created.amount, dec!(100));
    assert_eq!(created.fee_amount, dec!(2.50));
    assert_eq!(created.net_amount, dec!(97.50));
    assert_eq!(created.status, TransactionStatus::Pending);

    // Find by ID
    let found = tx_repo
        .find_by_id(created.id)
        .await
        .expect("Failed to find")
        .expect("Transaction not found");
    assert_eq!(found.id, created.id);

    // Find by idempotency key
    let found_idem = tx_repo
        .find_by_idempotency_key(&idempotency_key)
        .await
        .expect("Failed to find by idempotency key")
        .expect("Transaction not found");
    assert_eq!(found_idem.id, created.id);

    // Update status
    let settled = tx_repo
        .update_status(created.id, TransactionStatus::Settled)
        .await
        .expect("Failed to update status")
        .expect("Transaction not found");
    assert_eq!(settled.status, TransactionStatus::Settled);
    assert!(settled.settled_at.is_some());

    // List
    let transactions = tx_repo
        .list(Some(TransactionType::Payment), None, None, None, 10, 0)
        .await
        .expect("Failed to list");
    assert!(!transactions.is_empty());

    // Find by account
    let by_account = tx_repo
        .find_by_account(source.id, 10, 0)
        .await
        .expect("Failed to find by account");
    assert!(!by_account.is_empty());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_repository_operations() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let tx_repo = TransactionRepository::new(pool.clone());
    let ledger_repo = LedgerRepository::new(pool.clone());

    // Create accounts
    let source = Account::new(
        format!("SRC-{}", Uuid::new_v4()),
        "Source Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let dest = Account::new(
        format!("DST-{}", Uuid::new_v4()),
        "Destination Account".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let source = account_repo.create(&source).await.expect("Failed to create source");
    let dest = account_repo.create(&dest).await.expect("Failed to create dest");

    // Create transaction
    let tx = TransactionRecord::payment(
        format!("EXT-TX-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD".to_string(),
        dec!(0),
        format!("IDEM-{}", Uuid::new_v4()),
    );
    let tx = tx_repo.create(&tx).await.expect("Failed to create transaction");

    // Create ledger entries (debit source, credit dest)
    let effective_date = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
    let debit_entry = LedgerEntry::debit(
        tx.id,
        source.id,
        dec!(100),
        "USD".to_string(),
        dec!(900), // balance after
        effective_date,
    );
    let credit_entry = LedgerEntry::credit(
        tx.id,
        dest.id,
        dec!(100),
        "USD".to_string(),
        dec!(1100), // balance after
        effective_date,
    );

    let entries = ledger_repo
        .create_batch(&[debit_entry, credit_entry])
        .await
        .expect("Failed to create entries");
    assert_eq!(entries.len(), 2);

    // Find by transaction
    let tx_entries = ledger_repo
        .find_by_transaction(tx.id)
        .await
        .expect("Failed to find by transaction");
    assert_eq!(tx_entries.len(), 2);

    // Verify balance
    let balanced = ledger_repo
        .verify_transaction_balance(tx.id)
        .await
        .expect("Failed to verify balance");
    assert!(balanced);

    // Find by account
    let account_entries = ledger_repo
        .find_by_account(source.id, 10, 0)
        .await
        .expect("Failed to find by account");
    assert_eq!(account_entries.len(), 1);
    assert_eq!(account_entries[0].entry_type, EntryType::Debit);

    // Sum by type
    let debit_sum = ledger_repo
        .sum_by_account_and_type(source.id, "USD", EntryType::Debit)
        .await
        .expect("Failed to sum");
    assert_eq!(debit_sum, dec!(100));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_batch_repository_lifecycle() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let batch_repo = BatchRepository::new(pool.clone());

    // Create batch
    let settlement_date = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
    let cut_off = Utc::now() + Duration::hours(2);
    let batch = SettlementBatch::new(settlement_date, cut_off, "USD".to_string());

    let created = batch_repo.create(&batch).await.expect("Failed to create batch");
    assert_eq!(created.status, BatchStatus::Pending);
    assert_eq!(created.total_transactions, 0);

    // Find by ID
    let found = batch_repo
        .find_by_id(created.id)
        .await
        .expect("Failed to find")
        .expect("Batch not found");
    assert_eq!(found.id, created.id);

    // Increment totals
    let incremented = batch_repo
        .increment_totals(created.id, dec!(100), dec!(2.50))
        .await
        .expect("Failed to increment")
        .expect("Batch not found");
    assert_eq!(incremented.total_transactions, 1);
    assert_eq!(incremented.gross_amount, dec!(100));
    assert_eq!(incremented.fee_amount, dec!(2.50));

    // Update status to processing
    let processing = batch_repo
        .update_status(created.id, BatchStatus::Processing)
        .await
        .expect("Failed to update status")
        .expect("Batch not found");
    assert_eq!(processing.status, BatchStatus::Processing);

    // Update status to completed
    let completed = batch_repo
        .update_status(created.id, BatchStatus::Completed)
        .await
        .expect("Failed to update status")
        .expect("Batch not found");
    assert_eq!(completed.status, BatchStatus::Completed);
    assert!(completed.completed_at.is_some());

    // Find by status
    let completed_batches = batch_repo
        .find_by_status(BatchStatus::Completed)
        .await
        .expect("Failed to find by status");
    assert!(!completed_batches.is_empty());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_netting_repository_operations() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let batch_repo = BatchRepository::new(pool.clone());
    let netting_repo = NettingRepository::new(pool.clone());

    // Create accounts (participants)
    let participant_a = Account::new(
        format!("PART-A-{}", Uuid::new_v4()),
        "Participant A".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let participant_b = Account::new(
        format!("PART-B-{}", Uuid::new_v4()),
        "Participant B".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let participant_a = account_repo.create(&participant_a).await.expect("Failed to create");
    let participant_b = account_repo.create(&participant_b).await.expect("Failed to create");

    // Create batch
    let settlement_date = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
    let cut_off = Utc::now() + Duration::hours(2);
    let batch = SettlementBatch::new(settlement_date, cut_off, "USD".to_string());
    let batch = batch_repo.create(&batch).await.expect("Failed to create batch");

    // Create netting positions
    let mut pos_a = NettingPosition::new(batch.id, participant_a.id, "USD".to_string());
    pos_a.add_receivable(dec!(100));
    pos_a.add_payable(dec!(75));

    let mut pos_b = NettingPosition::new(batch.id, participant_b.id, "USD".to_string());
    pos_b.add_receivable(dec!(75));
    pos_b.add_payable(dec!(100));

    let positions = netting_repo
        .create_batch(&[pos_a, pos_b])
        .await
        .expect("Failed to create positions");
    assert_eq!(positions.len(), 2);

    // Find by batch
    let batch_positions = netting_repo
        .find_by_batch(batch.id)
        .await
        .expect("Failed to find by batch");
    assert_eq!(batch_positions.len(), 2);

    // Find net receivers
    let receivers = netting_repo
        .find_net_receivers(batch.id)
        .await
        .expect("Failed to find receivers");
    assert_eq!(receivers.len(), 1);
    assert_eq!(receivers[0].participant_id, participant_a.id);

    // Find net payers
    let payers = netting_repo
        .find_net_payers(batch.id)
        .await
        .expect("Failed to find payers");
    assert_eq!(payers.len(), 1);
    assert_eq!(payers[0].participant_id, participant_b.id);

    // Get batch summary
    let summary = netting_repo
        .get_batch_summary(batch.id)
        .await
        .expect("Failed to get summary");
    assert_eq!(summary.participant_count, 2);
    assert_eq!(summary.net_receivers, 1);
    assert_eq!(summary.net_payers, 1);

    // Upsert (update existing)
    let mut updated_pos = NettingPosition::new(batch.id, participant_a.id, "USD".to_string());
    updated_pos.add_receivable(dec!(150));
    updated_pos.add_payable(dec!(75));

    let upserted = netting_repo
        .upsert(&updated_pos)
        .await
        .expect("Failed to upsert");
    assert_eq!(upserted.gross_receivable, dec!(150));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_transaction_idempotency() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_repo = AccountRepository::new(pool.clone());
    let tx_repo = TransactionRepository::new(pool.clone());

    let source = Account::new(
        format!("SRC-{}", Uuid::new_v4()),
        "Source".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let dest = Account::new(
        format!("DST-{}", Uuid::new_v4()),
        "Dest".to_string(),
        AccountType::Asset,
        "USD".to_string(),
    );
    let source = account_repo.create(&source).await.expect("Failed to create");
    let dest = account_repo.create(&dest).await.expect("Failed to create");

    let idempotency_key = format!("IDEM-{}", Uuid::new_v4());

    // First transaction
    let tx1 = TransactionRecord::payment(
        format!("EXT-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD".to_string(),
        dec!(0),
        idempotency_key.clone(),
    );
    let created = tx_repo.create(&tx1).await.expect("Failed to create");

    // Check idempotency key exists
    let exists = tx_repo
        .exists_by_idempotency_key(&idempotency_key)
        .await
        .expect("Failed to check");
    assert!(exists);

    // Trying to create with same idempotency key should fail (unique constraint)
    let tx2 = TransactionRecord::payment(
        format!("EXT-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(200),
        "USD".to_string(),
        dec!(0),
        idempotency_key.clone(),
    );
    let duplicate = tx_repo.create(&tx2).await;
    assert!(duplicate.is_err());

    // Find by idempotency key returns original
    let found = tx_repo
        .find_by_idempotency_key(&idempotency_key)
        .await
        .expect("Failed to find")
        .expect("Not found");
    assert_eq!(found.id, created.id);
    assert_eq!(found.amount, dec!(100));

    common::cleanup_test_data(&pool).await;
}
