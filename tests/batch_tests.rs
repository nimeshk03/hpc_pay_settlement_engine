mod common;

use chrono::{Duration, Utc};
use rust_decimal_macros::dec;
use settlement_engine::models::{AccountType, BatchStatus};
use settlement_engine::services::{
    AccountService, BatchService, BatchStateMachine, CreateBatchRequest, LedgerService,
    LedgerTransactionRequest, SettlementWindowConfig, SettlementWindowType,
    account_service::CreateAccountRequest,
};
use uuid::Uuid;

fn unique_currency() -> String {
    format!("X{}", &Uuid::new_v4().to_string().replace("-", "")[..2]).to_uppercase()
}

#[tokio::test]
async fn test_batch_service_create_batch() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let batch_service = BatchService::new(pool.clone());

    let request = CreateBatchRequest::for_today(&currency, 24);
    let batch = batch_service
        .create_batch(request)
        .await
        .expect("Failed to create batch");

    assert_eq!(batch.status, BatchStatus::Pending);
    assert_eq!(batch.currency, currency);
    assert_eq!(batch.total_transactions, 0);
    assert_eq!(batch.gross_amount, dec!(0));
    assert!(batch.cut_off_time > Utc::now());
}

#[tokio::test]
async fn test_batch_service_duplicate_batch_prevention() {
    let pool = common::setup_test_db().await;
    let currency1 = unique_currency();
    let currency2 = unique_currency();

    let batch_service = BatchService::new(pool.clone());

    // Create first batch
    let request1 = CreateBatchRequest::for_today(&currency1, 24);
    batch_service
        .create_batch(request1)
        .await
        .expect("Failed to create first batch");

    // Try to create duplicate batch
    let request2 = CreateBatchRequest::for_today(&currency1, 24);
    let result = batch_service.create_batch(request2).await;
    assert!(result.is_err());

    // Different currency should work
    let request3 = CreateBatchRequest::for_today(&currency2, 24);
    let batch3 = batch_service
        .create_batch(request3)
        .await
        .expect("Failed to create second currency batch");
    assert_eq!(batch3.currency, currency2);
}

#[tokio::test]
async fn test_batch_service_get_or_create_current_batch() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let batch_service = BatchService::new(pool.clone());

    // First call creates a new batch
    let batch1 = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to get/create batch");

    // Second call returns the same batch
    let batch2 = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to get/create batch");

    assert_eq!(batch1.id, batch2.id);
}

#[tokio::test]
async fn test_batch_service_assign_transaction() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create accounts
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Create a transaction
    let tx_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        &currency,
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let tx_result = ledger_service
        .process_payment(tx_request)
        .await
        .expect("Failed to process payment");

    // Create a batch
    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    // Assign transaction to batch
    let assigned = batch_service
        .assign_transaction_to_batch(tx_result.transaction.id, batch.id)
        .await
        .expect("Failed to assign transaction");

    assert_eq!(assigned.settlement_batch_id, Some(batch.id));

    // Verify batch totals updated
    let updated_batch = batch_service
        .get_batch(batch.id)
        .await
        .expect("Failed to get batch")
        .expect("Batch not found");

    assert_eq!(updated_batch.total_transactions, 1);
    assert_eq!(updated_batch.gross_amount, dec!(100));
}

#[tokio::test]
async fn test_batch_service_recalculate_totals() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create accounts
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(5000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Create batch
    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    // Create and assign multiple transactions
    for i in 0..3 {
        let tx_request = LedgerTransactionRequest::payment(
            format!("PAY-{}-{}", i, Uuid::new_v4()),
            source.id,
            dest.id,
            dec!(100),
            &currency,
            format!("IDEM-{}-{}", i, Uuid::new_v4()),
        )
        .with_fee(dec!(5));

        let tx_result = ledger_service
            .process_payment(tx_request)
            .await
            .expect("Failed to process payment");

        batch_service
            .assign_transaction_to_batch(tx_result.transaction.id, batch.id)
            .await
            .expect("Failed to assign transaction");
    }

    // Recalculate totals
    let updated_batch = batch_service
        .recalculate_batch_totals(batch.id)
        .await
        .expect("Failed to recalculate totals");

    assert_eq!(updated_batch.total_transactions, 3);
    assert_eq!(updated_batch.gross_amount, dec!(300)); // 3 * 100
    assert_eq!(updated_batch.fee_amount, dec!(15)); // 3 * 5
    assert_eq!(updated_batch.net_amount, dec!(285)); // 300 - 15
}

#[tokio::test]
async fn test_batch_service_close_and_process() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create accounts
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Create batch and add transaction
    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    let tx_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        &currency,
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let tx_result = ledger_service
        .process_payment(tx_request)
        .await
        .expect("Failed to process payment");

    batch_service
        .assign_transaction_to_batch(tx_result.transaction.id, batch.id)
        .await
        .expect("Failed to assign transaction");

    // Trigger batch processing
    let result = batch_service
        .trigger_batch_processing(batch.id)
        .await
        .expect("Failed to process batch");

    assert_eq!(result.status, BatchStatus::Completed);
    assert_eq!(result.total_transactions, 1);
    assert_eq!(result.successful_transactions, 1);
    assert_eq!(result.failed_transactions, 0);
    assert!(result.errors.is_empty());

    // Verify batch status updated
    let final_batch = batch_service
        .get_batch(batch.id)
        .await
        .expect("Failed to get batch")
        .expect("Batch not found");

    assert_eq!(final_batch.status, BatchStatus::Completed);
    assert!(final_batch.completed_at.is_some());
}

#[tokio::test]
async fn test_batch_service_notifications() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create accounts and transaction
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    let tx_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        &currency,
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let tx_result = ledger_service
        .process_payment(tx_request)
        .await
        .expect("Failed to process payment");

    batch_service
        .assign_transaction_to_batch(tx_result.transaction.id, batch.id)
        .await
        .expect("Failed to assign transaction");

    // Clear any existing notifications
    batch_service.clear_notifications().await;

    // Process batch
    batch_service
        .trigger_batch_processing(batch.id)
        .await
        .expect("Failed to process batch");

    // Check notifications
    let notifications = batch_service.get_notifications().await;
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].batch_id, batch.id);
    assert_eq!(notifications[0].status, BatchStatus::Completed);
}

#[tokio::test]
async fn test_batch_service_fail_and_retry() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let batch_service = BatchService::new(pool.clone());

    // Create batch directly with explicit request
    let request = CreateBatchRequest::for_today(&currency, 24);
    let batch = batch_service
        .create_batch(request)
        .await
        .expect("Failed to create batch");

    // Close batch (move to processing)
    let closed = batch_service
        .close_batch(batch.id)
        .await
        .expect("Failed to close batch");
    assert_eq!(closed.status, BatchStatus::Processing);

    // Fail the batch
    let failed = batch_service
        .fail_batch(batch.id, "Test failure reason")
        .await
        .expect("Failed to fail batch");
    assert_eq!(failed.status, BatchStatus::Failed);

    // Retry the batch
    let retried = batch_service
        .retry_batch(batch.id)
        .await
        .expect("Failed to retry batch");
    assert_eq!(retried.status, BatchStatus::Pending);
}

#[tokio::test]
async fn test_batch_service_list_batches() {
    let pool = common::setup_test_db().await;
    let currency1 = unique_currency();
    let currency2 = unique_currency();
    let currency3 = unique_currency();

    let batch_service = BatchService::new(pool.clone());

    // Create batches in different currencies
    batch_service
        .create_batch(CreateBatchRequest::for_today(&currency1, 24))
        .await
        .expect("Failed to create first batch");

    batch_service
        .create_batch(CreateBatchRequest::for_today(&currency2, 24))
        .await
        .expect("Failed to create second batch");

    batch_service
        .create_batch(CreateBatchRequest::for_today(&currency3, 24))
        .await
        .expect("Failed to create third batch");

    // List all batches
    let all_batches = batch_service
        .list_batches(None, None, 100, 0)
        .await
        .expect("Failed to list batches");
    assert!(all_batches.len() >= 3);

    // List first currency batches only
    let filtered_batches = batch_service
        .list_batches(None, Some(&currency1), 10, 0)
        .await
        .expect("Failed to list filtered batches");
    assert!(filtered_batches.iter().all(|b| b.currency == currency1));

    // List pending batches
    let pending_batches = batch_service
        .list_batches(Some(BatchStatus::Pending), None, 100, 0)
        .await
        .expect("Failed to list pending batches");
    assert!(pending_batches.iter().all(|b| b.status == BatchStatus::Pending));
}

#[tokio::test]
async fn test_batch_state_machine() {
    // Valid transitions
    assert!(BatchStateMachine::can_transition(
        BatchStatus::Pending,
        BatchStatus::Processing
    ));
    assert!(BatchStateMachine::can_transition(
        BatchStatus::Processing,
        BatchStatus::Completed
    ));
    assert!(BatchStateMachine::can_transition(
        BatchStatus::Processing,
        BatchStatus::Failed
    ));
    assert!(BatchStateMachine::can_transition(
        BatchStatus::Failed,
        BatchStatus::Pending
    ));

    // Invalid transitions
    assert!(!BatchStateMachine::can_transition(
        BatchStatus::Completed,
        BatchStatus::Pending
    ));
    assert!(!BatchStateMachine::can_transition(
        BatchStatus::Pending,
        BatchStatus::Completed
    ));

    // Transition function
    let result = BatchStateMachine::transition(BatchStatus::Pending, BatchStatus::Processing);
    assert!(result.is_ok());

    let result = BatchStateMachine::transition(BatchStatus::Completed, BatchStatus::Pending);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_settlement_window_config() {
    let config = SettlementWindowConfig::default();
    assert_eq!(config.window_type, SettlementWindowType::Daily);
    assert!(config.auto_close);

    // Test window durations
    assert_eq!(SettlementWindowType::RealTime.duration(), Duration::seconds(0));
    assert_eq!(SettlementWindowType::MicroBatch.duration(), Duration::minutes(5));
    assert_eq!(SettlementWindowType::Hourly.duration(), Duration::hours(1));
    assert_eq!(SettlementWindowType::Daily.duration(), Duration::hours(24));
}

#[tokio::test]
async fn test_batch_service_with_custom_config() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let config = SettlementWindowConfig {
        window_type: SettlementWindowType::Hourly,
        cut_off_time: None,
        timezone: "UTC".to_string(),
        auto_close: true,
    };

    let batch_service = BatchService::new(pool.clone()).with_config(config);

    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    assert_eq!(batch.status, BatchStatus::Pending);
    assert_eq!(batch.currency, currency);
}

#[tokio::test]
async fn test_batch_service_get_batch_transactions() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create accounts
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(5000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Create batch
    let batch = batch_service
        .get_or_create_current_batch(&currency)
        .await
        .expect("Failed to create batch");

    // Create and assign transactions
    for i in 0..5 {
        let tx_request = LedgerTransactionRequest::payment(
            format!("PAY-{}-{}", i, Uuid::new_v4()),
            source.id,
            dest.id,
            dec!(50),
            &currency,
            format!("IDEM-{}-{}", i, Uuid::new_v4()),
        );

        let tx_result = ledger_service
            .process_payment(tx_request)
            .await
            .expect("Failed to process payment");

        batch_service
            .assign_transaction_to_batch(tx_result.transaction.id, batch.id)
            .await
            .expect("Failed to assign transaction");
    }

    // Get batch transactions
    let transactions = batch_service
        .get_batch_transactions(batch.id)
        .await
        .expect("Failed to get transactions");

    assert_eq!(transactions.len(), 5);
    assert!(transactions.iter().all(|t| t.settlement_batch_id == Some(batch.id)));
}
