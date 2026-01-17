mod common;

use rust_decimal_macros::dec;
use settlement_engine::models::{AccountStatus, AccountType, TransactionType};
use settlement_engine::services::{
    AccountService, BalanceService, DoubleEntryEngine,
    account_service::CreateAccountRequest,
    double_entry_engine::TransactionRequest,
};
use uuid::Uuid;

#[tokio::test]
async fn test_account_service_create_and_find() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let service = AccountService::new(pool.clone());

    // Create account
    let request = CreateAccountRequest {
        external_id: format!("EXT-{}", Uuid::new_v4()),
        name: "Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: Some(dec!(1000)),
        metadata: Some(serde_json::json!({"owner": "Test User"})),
    };

    let account = service.create_account(request.clone()).await.expect("Failed to create account");
    assert_eq!(account.name, "Test Account");
    assert_eq!(account.account_type, AccountType::Asset);
    assert_eq!(account.status, AccountStatus::Active);

    // Find by ID
    let found = service.find_by_id(account.id).await.expect("Failed to find by ID");
    assert_eq!(found.id, account.id);

    // Find by external ID
    let found_ext = service
        .find_by_external_id(&account.external_id)
        .await
        .expect("Failed to find by external ID");
    assert_eq!(found_ext.id, account.id);

    // Get balance
    let balance = service.get_balance(account.id, "USD").await.expect("Failed to get balance");
    assert_eq!(balance.available_balance, dec!(1000));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_account_service_status_management() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let service = AccountService::new(pool.clone());

    let request = CreateAccountRequest {
        external_id: format!("EXT-{}", Uuid::new_v4()),
        name: "Status Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: None,
        metadata: None,
    };

    let account = service.create_account(request).await.expect("Failed to create account");

    // Freeze account
    let frozen = service.freeze_account(account.id).await.expect("Failed to freeze");
    assert_eq!(frozen.status, AccountStatus::Frozen);

    // Validate for transaction should fail
    let validation = service.validate_for_transaction(account.id).await;
    assert!(validation.is_err());

    // Activate account
    let activated = service.activate_account(account.id).await.expect("Failed to activate");
    assert_eq!(activated.status, AccountStatus::Active);

    // Close account (balance is zero)
    let closed = service.close_account(account.id).await.expect("Failed to close");
    assert_eq!(closed.status, AccountStatus::Closed);

    // Cannot freeze closed account
    let freeze_closed = service.freeze_account(account.id).await;
    assert!(freeze_closed.is_err());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_account_service_validation() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let service = AccountService::new(pool.clone());

    // Empty external ID should fail
    let request = CreateAccountRequest {
        external_id: "".to_string(),
        name: "Test".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: None,
        metadata: None,
    };
    assert!(service.create_account(request).await.is_err());

    // Empty name should fail
    let request = CreateAccountRequest {
        external_id: "EXT-001".to_string(),
        name: "".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: None,
        metadata: None,
    };
    assert!(service.create_account(request).await.is_err());

    // Invalid currency should fail
    let request = CreateAccountRequest {
        external_id: "EXT-001".to_string(),
        name: "Test".to_string(),
        account_type: AccountType::Asset,
        currency: "US".to_string(), // Invalid - not 3 chars
        initial_balance: None,
        metadata: None,
    };
    assert!(service.create_account(request).await.is_err());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_balance_service_operations() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let balance_service = BalanceService::new(pool.clone());

    // Create account with initial balance
    let request = CreateAccountRequest {
        external_id: format!("EXT-{}", Uuid::new_v4()),
        name: "Balance Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: Some(dec!(1000)),
        metadata: None,
    };

    let account = account_service.create_account(request).await.expect("Failed to create account");

    // Credit
    let credited = balance_service
        .credit(account.id, "USD", dec!(500))
        .await
        .expect("Failed to credit");
    assert_eq!(credited.available_balance, dec!(1500));

    // Debit
    let debited = balance_service
        .debit(account.id, "USD", dec!(200))
        .await
        .expect("Failed to debit");
    assert_eq!(debited.available_balance, dec!(1300));

    // Reserve
    let reserved = balance_service
        .reserve(account.id, "USD", dec!(100))
        .await
        .expect("Failed to reserve");
    assert_eq!(reserved.available_balance, dec!(1200));
    assert_eq!(reserved.reserved_balance, dec!(100));

    // Check usable balance
    let usable = balance_service
        .get_usable_balance(account.id, "USD")
        .await
        .expect("Failed to get usable");
    assert_eq!(usable, dec!(1100)); // 1200 - 100 reserved

    // Release reservation
    let released = balance_service
        .release_reservation(account.id, "USD", dec!(50))
        .await
        .expect("Failed to release");
    assert_eq!(released.available_balance, dec!(1250));
    assert_eq!(released.reserved_balance, dec!(50));

    // Create snapshot
    let snapshot = balance_service
        .create_snapshot(account.id, "USD")
        .await
        .expect("Failed to create snapshot");
    assert_eq!(snapshot.available_balance, dec!(1250));
    assert_eq!(snapshot.usable_balance, dec!(1200));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_balance_service_insufficient_funds() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let balance_service = BalanceService::new(pool.clone());

    let request = CreateAccountRequest {
        external_id: format!("EXT-{}", Uuid::new_v4()),
        name: "Low Balance Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: Some(dec!(100)),
        metadata: None,
    };

    let account = account_service.create_account(request).await.expect("Failed to create account");

    // Debit more than available should fail
    let result = balance_service.debit(account.id, "USD", dec!(200)).await;
    assert!(result.is_err());

    // Validate sufficient funds
    let validation = balance_service
        .validate_sufficient_funds(account.id, "USD", dec!(200))
        .await;
    assert!(validation.is_err());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_double_entry_engine_basic_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let engine = DoubleEntryEngine::new(pool.clone());

    // Create source and destination accounts
    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(500)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Execute transaction (no fee for simple balance verification)
    let request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: source.id,
        destination_account_id: dest.id,
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };

    let result = engine.execute_transaction(request).await.expect("Failed to execute transaction");

    // Verify transaction
    assert_eq!(result.transaction.amount, dec!(100));
    assert_eq!(result.transaction.fee_amount, dec!(0));
    assert_eq!(result.transaction.net_amount, dec!(100));

    // Verify balances
    assert_eq!(result.source_balance.available_balance, dec!(900)); // 1000 - 100
    assert_eq!(result.destination_balance.available_balance, dec!(600)); // 500 + 100

    // Verify ledger entries are balanced (debits = credits when no fee)
    let balanced = engine
        .verify_transaction_balance(result.transaction.id)
        .await
        .expect("Failed to verify balance");
    assert!(balanced);

    // Get entries
    let entries = engine
        .get_transaction_entries(result.transaction.id)
        .await
        .expect("Failed to get entries");
    assert_eq!(entries.len(), 2);

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_double_entry_engine_idempotency() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let engine = DoubleEntryEngine::new(pool.clone());

    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
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
            currency: "USD".to_string(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    let idempotency_key = format!("IDEM-{}", Uuid::new_v4());

    // First transaction
    let request1 = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: source.id,
        destination_account_id: dest.id,
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: idempotency_key.clone(),
        effective_date: None,
        metadata: None,
    };

    let result1 = engine.execute_transaction(request1).await.expect("Failed first transaction");

    // Second transaction with same idempotency key
    let request2 = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: source.id,
        destination_account_id: dest.id,
        amount: dec!(200), // Different amount
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: idempotency_key.clone(),
        effective_date: None,
        metadata: None,
    };

    let result2 = engine.execute_transaction(request2).await.expect("Failed second transaction");

    // Should return the same transaction
    assert_eq!(result1.transaction.id, result2.transaction.id);
    assert_eq!(result2.transaction.amount, dec!(100)); // Original amount

    // Balance should only be debited once
    let source_balance = account_service
        .get_balance(source.id, "USD")
        .await
        .expect("Failed to get balance");
    assert_eq!(source_balance.available_balance, dec!(900)); // 1000 - 100, not 1000 - 300

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_double_entry_engine_insufficient_funds() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let engine = DoubleEntryEngine::new(pool.clone());

    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Low Balance Source".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(50)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    let request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: source.id,
        destination_account_id: dest.id,
        amount: dec!(100), // More than available
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };

    let result = engine.execute_transaction(request).await;
    assert!(result.is_err());

    // Balance should be unchanged
    let source_balance = account_service
        .get_balance(source.id, "USD")
        .await
        .expect("Failed to get balance");
    assert_eq!(source_balance.available_balance, dec!(50));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_double_entry_engine_reversal() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let engine = DoubleEntryEngine::new(pool.clone());

    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
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
            currency: "USD".to_string(),
            initial_balance: Some(dec!(500)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    // Execute original transaction
    let original_request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: source.id,
        destination_account_id: dest.id,
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };

    let original = engine
        .execute_transaction(original_request)
        .await
        .expect("Failed to execute original");

    // Verify balances after original
    assert_eq!(original.source_balance.available_balance, dec!(900));
    assert_eq!(original.destination_balance.available_balance, dec!(600));

    // Reverse the transaction
    use settlement_engine::services::double_entry_engine::ReversalRequest;
    let reversal_request = ReversalRequest {
        original_transaction_id: original.transaction.id,
        external_id: format!("REV-{}", Uuid::new_v4()),
        idempotency_key: format!("IDEM-REV-{}", Uuid::new_v4()),
        reason: Some("Customer requested refund".to_string()),
    };

    let reversal = engine
        .reverse_transaction(reversal_request)
        .await
        .expect("Failed to reverse");

    // Verify reversal transaction
    assert_eq!(reversal.transaction.transaction_type, TransactionType::Refund);
    assert_eq!(reversal.transaction.amount, dec!(100));

    // Verify balances are restored
    // Source gets money back: 900 + 100 = 1000
    // Dest loses money: 600 - 100 = 500
    let source_balance = account_service
        .get_balance(source.id, "USD")
        .await
        .expect("Failed to get source balance");
    let dest_balance = account_service
        .get_balance(dest.id, "USD")
        .await
        .expect("Failed to get dest balance");

    assert_eq!(source_balance.available_balance, dec!(1000));
    assert_eq!(dest_balance.available_balance, dec!(500));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_double_entry_engine_validation() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let engine = DoubleEntryEngine::new(pool.clone());

    let account = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("ACC-{}", Uuid::new_v4()),
            name: "Test Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create account");

    // Same source and destination should fail
    let request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: account.id,
        destination_account_id: account.id, // Same as source
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };
    assert!(engine.execute_transaction(request).await.is_err());

    // Zero amount should fail
    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Dest".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: None,
            metadata: None,
        })
        .await
        .expect("Failed to create dest");

    let request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: account.id,
        destination_account_id: dest.id,
        amount: dec!(0), // Zero amount
        currency: "USD".to_string(),
        fee_amount: dec!(0),
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };
    assert!(engine.execute_transaction(request).await.is_err());

    // Fee greater than amount should fail
    let request = TransactionRequest {
        external_id: format!("TX-{}", Uuid::new_v4()),
        transaction_type: TransactionType::Payment,
        source_account_id: account.id,
        destination_account_id: dest.id,
        amount: dec!(100),
        currency: "USD".to_string(),
        fee_amount: dec!(150), // Fee > amount
        idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
        effective_date: None,
        metadata: None,
    };
    assert!(engine.execute_transaction(request).await.is_err());

    common::cleanup_test_data(&pool).await;
}
