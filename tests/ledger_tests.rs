mod common;

use rust_decimal_macros::dec;
use settlement_engine::models::{AccountType, TransactionStatus, TransactionType};
use settlement_engine::services::{
    AccountService, LedgerService, LedgerTransactionRequest, TransactionStateMachine,
    ValidationResult, account_service::CreateAccountRequest,
};
use uuid::Uuid;

#[tokio::test]
async fn test_ledger_service_payment_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

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

    // Process payment
    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service
        .process_payment(request)
        .await
        .expect("Failed to process payment");

    // Verify transaction
    assert_eq!(result.transaction.transaction_type, TransactionType::Payment);
    assert_eq!(result.transaction.status, TransactionStatus::Settled);
    assert_eq!(result.transaction.amount, dec!(100));

    // Verify balances
    assert_eq!(result.source_balance.available_balance, dec!(900));
    assert_eq!(result.destination_balance.available_balance, dec!(600));

    // Verify ledger entries
    assert_eq!(result.entries.len(), 2);

    // Verify balance is correct
    let balanced = ledger_service
        .verify_transaction_balance(result.transaction.id)
        .await
        .expect("Failed to verify balance");
    assert!(balanced);

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_transfer_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Transfer Source".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(2000)),
            metadata: None,
        })
        .await
        .expect("Failed to create source");

    let dest = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Transfer Destination".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create destination");

    let request = LedgerTransactionRequest::transfer(
        format!("TRF-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(500),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service
        .process_transfer(request)
        .await
        .expect("Failed to process transfer");

    assert_eq!(result.transaction.transaction_type, TransactionType::Transfer);
    assert_eq!(result.source_balance.available_balance, dec!(1500));
    assert_eq!(result.destination_balance.available_balance, dec!(500));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_fee_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let customer = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("CUST-{}", Uuid::new_v4()),
            name: "Customer Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create customer");

    let fee_account = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("FEE-{}", Uuid::new_v4()),
            name: "Fee Collection Account".to_string(),
            account_type: AccountType::Revenue,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(0)),
            metadata: None,
        })
        .await
        .expect("Failed to create fee account");

    let request = LedgerTransactionRequest::fee(
        format!("FEE-{}", Uuid::new_v4()),
        customer.id,
        fee_account.id,
        dec!(25),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service
        .process_fee(request)
        .await
        .expect("Failed to process fee");

    assert_eq!(result.transaction.transaction_type, TransactionType::Fee);
    assert_eq!(result.source_balance.available_balance, dec!(975));
    assert_eq!(result.destination_balance.available_balance, dec!(25));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_refund_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let merchant = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("MERCH-{}", Uuid::new_v4()),
            name: "Merchant Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(5000)),
            metadata: None,
        })
        .await
        .expect("Failed to create merchant");

    let customer = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("CUST-{}", Uuid::new_v4()),
            name: "Customer Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create customer");

    // First, create original payment (customer pays merchant)
    let payment_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        customer.id,
        merchant.id,
        dec!(200),
        "USD",
        format!("IDEM-PAY-{}", Uuid::new_v4()),
    );

    let payment_result = ledger_service
        .process_payment(payment_request)
        .await
        .expect("Failed to process payment");

    // Verify payment
    assert_eq!(payment_result.source_balance.available_balance, dec!(800)); // Customer: 1000 - 200
    assert_eq!(payment_result.destination_balance.available_balance, dec!(5200)); // Merchant: 5000 + 200

    // Now process refund (merchant refunds customer)
    let refund_request = LedgerTransactionRequest::refund(
        format!("REF-{}", Uuid::new_v4()),
        payment_result.transaction.id,
        merchant.id, // Merchant is now the source (paying back)
        customer.id, // Customer is the destination (receiving refund)
        dec!(200),
        "USD",
        format!("IDEM-REF-{}", Uuid::new_v4()),
    );

    let refund_result = ledger_service
        .process_refund(refund_request)
        .await
        .expect("Failed to process refund");

    assert_eq!(refund_result.transaction.transaction_type, TransactionType::Refund);
    assert_eq!(refund_result.source_balance.available_balance, dec!(5000)); // Merchant back to original
    assert_eq!(refund_result.destination_balance.available_balance, dec!(1000)); // Customer back to original

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_chargeback_transaction() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let merchant = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("MERCH-{}", Uuid::new_v4()),
            name: "Merchant Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(5000)),
            metadata: None,
        })
        .await
        .expect("Failed to create merchant");

    let customer = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("CUST-{}", Uuid::new_v4()),
            name: "Customer Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create customer");

    // Original payment
    let payment_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        customer.id,
        merchant.id,
        dec!(300),
        "USD",
        format!("IDEM-PAY-{}", Uuid::new_v4()),
    );

    let payment_result = ledger_service
        .process_payment(payment_request)
        .await
        .expect("Failed to process payment");

    // Process chargeback
    let chargeback_request = LedgerTransactionRequest::chargeback(
        format!("CB-{}", Uuid::new_v4()),
        payment_result.transaction.id,
        merchant.id,
        customer.id,
        dec!(300),
        "USD",
        format!("IDEM-CB-{}", Uuid::new_v4()),
    );

    let chargeback_result = ledger_service
        .process_chargeback(chargeback_request)
        .await
        .expect("Failed to process chargeback");

    assert_eq!(chargeback_result.transaction.transaction_type, TransactionType::Chargeback);
    assert_eq!(chargeback_result.source_balance.available_balance, dec!(5000)); // Merchant back to original
    assert_eq!(chargeback_result.destination_balance.available_balance, dec!(1000)); // Customer back to original

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_payment_with_fee() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

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

    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    )
    .with_fee(dec!(5));

    let result = ledger_service
        .process_payment(request)
        .await
        .expect("Failed to process payment");

    // Source debited full amount
    assert_eq!(result.source_balance.available_balance, dec!(900)); // 1000 - 100
    // Destination credited net amount (amount - fee)
    assert_eq!(result.destination_balance.available_balance, dec!(95)); // 0 + (100 - 5)
    assert_eq!(result.transaction.net_amount, dec!(95));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_validation_errors() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

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
    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        account.id,
        account.id,
        dec!(100),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_payment(request).await;
    assert!(result.is_err());

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

    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        account.id,
        dest.id,
        dec!(0),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_payment(request).await;
    assert!(result.is_err());

    // Fee exceeds amount should fail
    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        account.id,
        dest.id,
        dec!(100),
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    )
    .with_fee(dec!(150));

    let result = ledger_service.process_payment(request).await;
    assert!(result.is_err());

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_insufficient_funds() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let source = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Low Balance".to_string(),
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

    let request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100), // More than available
        "USD",
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_payment(request).await;
    assert!(result.is_err());

    // Verify balance unchanged
    let balance = account_service
        .get_balance(source.id, "USD")
        .await
        .expect("Failed to get balance");
    assert_eq!(balance.available_balance, dec!(50));

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_idempotency() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

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

    // First request
    let request1 = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100),
        "USD",
        idempotency_key.clone(),
    );

    let result1 = ledger_service
        .process_payment(request1)
        .await
        .expect("Failed first payment");

    // Second request with same idempotency key
    let request2 = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(200), // Different amount
        "USD",
        idempotency_key.clone(),
    );

    let result2 = ledger_service
        .process_payment(request2)
        .await
        .expect("Failed second payment");

    // Should return same transaction
    assert_eq!(result1.transaction.id, result2.transaction.id);
    assert_eq!(result2.transaction.amount, dec!(100)); // Original amount

    // Balance should only be debited once
    let balance = account_service
        .get_balance(source.id, "USD")
        .await
        .expect("Failed to get balance");
    assert_eq!(balance.available_balance, dec!(900)); // 1000 - 100, not 1000 - 300

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_ledger_service_account_history() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let account = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("ACC-{}", Uuid::new_v4()),
            name: "History Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create account");

    let other = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("OTHER-{}", Uuid::new_v4()),
            name: "Other Account".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create other");

    // Create multiple transactions
    for i in 0..3 {
        let request = LedgerTransactionRequest::payment(
            format!("PAY-{}-{}", i, Uuid::new_v4()),
            account.id,
            other.id,
            dec!(50),
            "USD",
            format!("IDEM-{}-{}", i, Uuid::new_v4()),
        );

        ledger_service
            .process_payment(request)
            .await
            .expect("Failed to process payment");
    }

    // Get account history
    let history = ledger_service
        .get_account_history(account.id, 10)
        .await
        .expect("Failed to get history");

    assert_eq!(history.len(), 3);

    common::cleanup_test_data(&pool).await;
}

#[tokio::test]
async fn test_transaction_state_machine() {
    // Test valid transitions
    assert!(TransactionStateMachine::can_transition(
        TransactionStatus::Pending,
        TransactionStatus::Settled
    ));
    assert!(TransactionStateMachine::can_transition(
        TransactionStatus::Pending,
        TransactionStatus::Failed
    ));
    assert!(TransactionStateMachine::can_transition(
        TransactionStatus::Settled,
        TransactionStatus::Reversed
    ));

    // Test invalid transitions
    assert!(!TransactionStateMachine::can_transition(
        TransactionStatus::Failed,
        TransactionStatus::Settled
    ));
    assert!(!TransactionStateMachine::can_transition(
        TransactionStatus::Reversed,
        TransactionStatus::Pending
    ));
    assert!(!TransactionStateMachine::can_transition(
        TransactionStatus::Pending,
        TransactionStatus::Reversed
    ));

    // Test transition function
    let result = TransactionStateMachine::transition(
        TransactionStatus::Pending,
        TransactionStatus::Settled,
    );
    assert!(result.is_ok());

    let result = TransactionStateMachine::transition(
        TransactionStatus::Failed,
        TransactionStatus::Settled,
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn test_ledger_service_refund_validation() {
    let pool = common::setup_test_db().await;
    common::cleanup_test_data(&pool).await;

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());

    let merchant = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("MERCH-{}", Uuid::new_v4()),
            name: "Merchant".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(5000)),
            metadata: None,
        })
        .await
        .expect("Failed to create merchant");

    let customer = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("CUST-{}", Uuid::new_v4()),
            name: "Customer".to_string(),
            account_type: AccountType::Asset,
            currency: "USD".to_string(),
            initial_balance: Some(dec!(1000)),
            metadata: None,
        })
        .await
        .expect("Failed to create customer");

    // Create original payment
    let payment_request = LedgerTransactionRequest::payment(
        format!("PAY-{}", Uuid::new_v4()),
        customer.id,
        merchant.id,
        dec!(100),
        "USD",
        format!("IDEM-PAY-{}", Uuid::new_v4()),
    );

    let payment_result = ledger_service
        .process_payment(payment_request)
        .await
        .expect("Failed to process payment");

    // Try to refund more than original amount
    let refund_request = LedgerTransactionRequest::refund(
        format!("REF-{}", Uuid::new_v4()),
        payment_result.transaction.id,
        merchant.id,
        customer.id,
        dec!(200), // More than original 100
        "USD",
        format!("IDEM-REF-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_refund(refund_request).await;
    assert!(result.is_err());

    // Try to refund non-existent transaction
    let refund_request = LedgerTransactionRequest::refund(
        format!("REF-{}", Uuid::new_v4()),
        Uuid::new_v4(), // Non-existent transaction
        merchant.id,
        customer.id,
        dec!(50),
        "USD",
        format!("IDEM-REF-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_refund(refund_request).await;
    assert!(result.is_err());

    common::cleanup_test_data(&pool).await;
}
