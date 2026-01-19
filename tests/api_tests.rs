mod common;

use settlement_engine::api::requests::{CreateAccountRequest, CreateTransactionRequest};
use settlement_engine::api::responses::{ApiResponse, AccountResponse, TransactionResponse, BatchResponse, PaginatedResponse};
use settlement_engine::models::{AccountType, TransactionType};
use settlement_engine::services::{AccountService, LedgerService, BatchService, LedgerTransactionRequest};
use rust_decimal_macros::dec;
use uuid::Uuid;

fn unique_currency() -> String {
    let id = Uuid::new_v4().to_string();
    format!("T{}", &id[0..2].to_uppercase())
}

#[tokio::test]
async fn test_api_response_success_serialization() {
    let response: ApiResponse<String> = ApiResponse::success("test data".to_string());
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"data\":\"test data\""));
}

#[tokio::test]
async fn test_api_response_error_serialization() {
    use settlement_engine::api::responses::ErrorResponse;
    let error = ErrorResponse::new("TEST_ERROR", "Test error message");
    let response: ApiResponse<()> = ApiResponse::<()>::error(error);
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":false"));
    assert!(json.contains("\"code\":\"TEST_ERROR\""));
}

#[tokio::test]
async fn test_account_response_from_account() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let account_service = AccountService::new(pool);

    let request = settlement_engine::services::account_service::CreateAccountRequest {
        external_id: format!("API-TEST-{}", Uuid::new_v4()),
        name: "API Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: currency.clone(),
        initial_balance: Some(dec!(500.00)),
        metadata: None,
    };

    let account = account_service.create_account(request).await.unwrap();
    let response = AccountResponse::from(account.clone());

    assert_eq!(response.id, account.id);
    assert_eq!(response.name, "API Test Account");
    assert_eq!(response.currency, currency);
}

#[tokio::test]
async fn test_transaction_response_from_transaction() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool);

    let source = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000.00)),
            metadata: None,
        })
        .await
        .unwrap();

    let dest = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0.00)),
            metadata: None,
        })
        .await
        .unwrap();

    let tx_request = LedgerTransactionRequest::payment(
        format!("TX-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(100.00),
        &currency,
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_transaction(tx_request).await.unwrap();
    let response = TransactionResponse::from(result.transaction.clone());

    assert_eq!(response.id, result.transaction.id);
    assert_eq!(response.amount, dec!(100.00));
    assert_eq!(response.currency, currency);
}

#[tokio::test]
async fn test_batch_response_from_batch() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let batch_service = BatchService::new(pool);

    let batch = batch_service.get_or_create_current_batch(&currency).await.unwrap();
    let response = BatchResponse::from(batch.clone());

    assert_eq!(response.id, batch.id);
    assert_eq!(response.currency, currency);
}

#[tokio::test]
async fn test_paginated_response() {
    let items = vec!["item1".to_string(), "item2".to_string(), "item3".to_string()];
    let response = PaginatedResponse::new(items.clone(), 100, 50, 0);

    assert_eq!(response.items.len(), 3);
    assert_eq!(response.total, 100);
    assert_eq!(response.limit, 50);
    assert_eq!(response.offset, 0);
}

#[tokio::test]
async fn test_create_account_request_validation_success() {
    let request = CreateAccountRequest {
        external_id: "ACC001".to_string(),
        name: "Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: Some(dec!(100.00)),
        metadata: None,
    };
    assert!(request.validate().is_ok());
}

#[tokio::test]
async fn test_create_account_request_validation_empty_external_id() {
    let request = CreateAccountRequest {
        external_id: "".to_string(),
        name: "Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "USD".to_string(),
        initial_balance: None,
        metadata: None,
    };
    let result = request.validate();
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.field == "external_id"));
}

#[tokio::test]
async fn test_create_account_request_validation_invalid_currency() {
    let request = CreateAccountRequest {
        external_id: "ACC001".to_string(),
        name: "Test Account".to_string(),
        account_type: AccountType::Asset,
        currency: "US".to_string(),
        initial_balance: None,
        metadata: None,
    };
    let result = request.validate();
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.field == "currency"));
}

#[tokio::test]
async fn test_create_transaction_request_validation_success() {
    let request = CreateTransactionRequest {
        external_id: "TX001".to_string(),
        transaction_type: TransactionType::Payment,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(100.00),
        currency: "USD".to_string(),
        fee_amount: None,
        idempotency_key: "IDEM001".to_string(),
        metadata: None,
    };
    assert!(request.validate().is_ok());
}

#[tokio::test]
async fn test_create_transaction_request_validation_zero_amount() {
    let request = CreateTransactionRequest {
        external_id: "TX001".to_string(),
        transaction_type: TransactionType::Payment,
        source_account_id: Uuid::new_v4(),
        destination_account_id: Uuid::new_v4(),
        amount: dec!(0.00),
        currency: "USD".to_string(),
        fee_amount: None,
        idempotency_key: "IDEM001".to_string(),
        metadata: None,
    };
    let result = request.validate();
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.field == "amount"));
}

#[tokio::test]
async fn test_ledger_service_get_transaction() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool);

    let source = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000.00)),
            metadata: None,
        })
        .await
        .unwrap();

    let dest = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0.00)),
            metadata: None,
        })
        .await
        .unwrap();

    let tx_request = LedgerTransactionRequest::payment(
        format!("TX-{}", Uuid::new_v4()),
        source.id,
        dest.id,
        dec!(50.00),
        &currency,
        format!("IDEM-{}", Uuid::new_v4()),
    );

    let result = ledger_service.process_transaction(tx_request).await.unwrap();

    let fetched = ledger_service.get_transaction(result.transaction.id).await.unwrap();
    assert_eq!(fetched.id, result.transaction.id);
    assert_eq!(fetched.amount, dec!(50.00));
}

#[tokio::test]
async fn test_ledger_service_list_transactions() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool);

    let source = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("SRC-{}", Uuid::new_v4()),
            name: "Source".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(1000.00)),
            metadata: None,
        })
        .await
        .unwrap();

    let dest = account_service
        .create_account(settlement_engine::services::account_service::CreateAccountRequest {
            external_id: format!("DST-{}", Uuid::new_v4()),
            name: "Destination".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(0.00)),
            metadata: None,
        })
        .await
        .unwrap();

    for i in 0..3 {
        let tx_request = LedgerTransactionRequest::payment(
            format!("TX-{}-{}", i, Uuid::new_v4()),
            source.id,
            dest.id,
            dec!(10.00),
            &currency,
            format!("IDEM-{}-{}", i, Uuid::new_v4()),
        );
        ledger_service.process_transaction(tx_request).await.unwrap();
    }

    let transactions = ledger_service
        .list_transactions(Some(source.id), None, Some(&currency), 10, 0)
        .await
        .unwrap();

    assert!(transactions.len() >= 3);
}

#[tokio::test]
async fn test_batch_service_get_batch() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let batch_service = BatchService::new(pool);

    let batch = batch_service.get_or_create_current_batch(&currency).await.unwrap();
    let fetched = batch_service.get_batch(batch.id).await.unwrap();

    assert_eq!(fetched.id, batch.id);
    assert_eq!(fetched.currency, currency);
}

#[tokio::test]
async fn test_batch_service_list_batches() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();
    let batch_service = BatchService::new(pool);

    batch_service.get_or_create_current_batch(&currency).await.unwrap();

    let batches = batch_service
        .list_batches(None, Some(&currency), 10, 0)
        .await
        .unwrap();

    assert!(!batches.is_empty());
}
