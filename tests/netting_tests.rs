mod common;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use settlement_engine::models::{AccountType, NettingPosition, NettingSummary};
use settlement_engine::services::{
    AccountService, BatchService, CreateBatchRequest, LedgerService, LedgerTransactionRequest,
    NettingService, account_service::CreateAccountRequest,
};
use uuid::Uuid;

fn unique_currency() -> String {
    format!("N{}", &Uuid::new_v4().to_string().replace("-", "")[..2]).to_uppercase()
}

#[tokio::test]
async fn test_netting_service_bilateral_calculation() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create two banks
    let bank_a = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-A-{}", Uuid::new_v4()),
            name: "Bank A".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(100000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank A");

    let bank_b = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-B-{}", Uuid::new_v4()),
            name: "Bank B".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(100000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank B");

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // Bank A pays Bank B: 100,000
    let tx1 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_a.id,
            bank_b.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 1");

    batch_service
        .assign_transaction_to_batch(tx1.transaction.id, batch.id)
        .await
        .expect("Failed to assign tx1");

    // Bank B pays Bank A: 75,000
    let tx2 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_b.id,
            bank_a.id,
            dec!(75000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 2");

    batch_service
        .assign_transaction_to_batch(tx2.transaction.id, batch.id)
        .await
        .expect("Failed to assign tx2");

    // Get batch transactions
    let transactions = batch_service
        .get_batch_transactions(batch.id)
        .await
        .expect("Failed to get transactions");

    // Calculate bilateral netting
    let result = netting_service.calculate_bilateral_netting(batch.id, &currency, &transactions);

    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.total_gross_volume, dec!(175000));
    assert_eq!(result.total_net_volume, dec!(25000));
    // Efficiency should be ~85.7%
    assert!(result.netting_efficiency > dec!(85));
    assert_eq!(result.instructions.len(), 1);
    assert_eq!(result.instructions[0].amount, dec!(25000));
}

#[tokio::test]
async fn test_netting_service_multilateral_calculation() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create three banks
    let bank_a = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-A-{}", Uuid::new_v4()),
            name: "Bank A".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank A");

    let bank_b = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-B-{}", Uuid::new_v4()),
            name: "Bank B".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank B");

    let bank_c = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-C-{}", Uuid::new_v4()),
            name: "Bank C".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank C");

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // A -> B: 100,000
    let tx1 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_a.id,
            bank_b.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 1");
    batch_service.assign_transaction_to_batch(tx1.transaction.id, batch.id).await.unwrap();

    // B -> C: 80,000
    let tx2 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_b.id,
            bank_c.id,
            dec!(80000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 2");
    batch_service.assign_transaction_to_batch(tx2.transaction.id, batch.id).await.unwrap();

    // C -> A: 60,000
    let tx3 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_c.id,
            bank_a.id,
            dec!(60000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 3");
    batch_service.assign_transaction_to_batch(tx3.transaction.id, batch.id).await.unwrap();

    // Get batch transactions
    let transactions = batch_service
        .get_batch_transactions(batch.id)
        .await
        .expect("Failed to get transactions");

    // Calculate multilateral netting
    let result = netting_service.calculate_multilateral_netting(batch.id, &currency, &transactions);

    assert_eq!(result.positions.len(), 3);
    assert_eq!(result.participant_count, 3);

    // Verify positions sum to zero (conservation of money)
    let total_net: Decimal = result.positions.iter().map(|p| p.net_position).sum();
    assert_eq!(total_net, Decimal::ZERO);

    // Net positions:
    // A: receives 60k, pays 100k = -40k (net payer)
    // B: receives 100k, pays 80k = +20k (net receiver)
    // C: receives 80k, pays 60k = +20k (net receiver)
    let pos_a = result.positions.iter().find(|p| p.participant_id == bank_a.id).unwrap();
    let pos_b = result.positions.iter().find(|p| p.participant_id == bank_b.id).unwrap();
    let pos_c = result.positions.iter().find(|p| p.participant_id == bank_c.id).unwrap();

    assert_eq!(pos_a.net_position, dec!(-40000));
    assert_eq!(pos_b.net_position, dec!(20000));
    assert_eq!(pos_c.net_position, dec!(20000));
}

#[tokio::test]
async fn test_netting_service_circular_dependency() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create three banks
    let bank_a = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-A-{}", Uuid::new_v4()),
            name: "Bank A".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank A");

    let bank_b = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-B-{}", Uuid::new_v4()),
            name: "Bank B".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank B");

    let bank_c = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-C-{}", Uuid::new_v4()),
            name: "Bank C".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(500000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank C");

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // Circular: A -> B -> C -> A (each 100,000)
    let tx1 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_a.id,
            bank_b.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 1");
    batch_service.assign_transaction_to_batch(tx1.transaction.id, batch.id).await.unwrap();

    let tx2 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_b.id,
            bank_c.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 2");
    batch_service.assign_transaction_to_batch(tx2.transaction.id, batch.id).await.unwrap();

    let tx3 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_c.id,
            bank_a.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 3");
    batch_service.assign_transaction_to_batch(tx3.transaction.id, batch.id).await.unwrap();

    // Get batch transactions
    let transactions = batch_service
        .get_batch_transactions(batch.id)
        .await
        .expect("Failed to get transactions");

    // Calculate multilateral netting
    let result = netting_service.calculate_multilateral_netting(batch.id, &currency, &transactions);

    // All positions should be balanced (circular cancels out)
    assert!(result.positions.iter().all(|p| p.is_balanced()));
    assert_eq!(result.total_net_volume, Decimal::ZERO);
    assert_eq!(result.netting_efficiency, dec!(100));
    // No settlement instructions needed
    assert!(result.instructions.is_empty());
}

#[tokio::test]
async fn test_netting_service_persist_positions() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create two banks
    let bank_a = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-A-{}", Uuid::new_v4()),
            name: "Bank A".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(100000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank A");

    let bank_b = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-B-{}", Uuid::new_v4()),
            name: "Bank B".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(100000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank B");

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // Create transaction
    let tx = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_a.id,
            bank_b.id,
            dec!(50000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment");
    batch_service.assign_transaction_to_batch(tx.transaction.id, batch.id).await.unwrap();

    // Get transactions and calculate netting
    let transactions = batch_service.get_batch_transactions(batch.id).await.unwrap();
    let result = netting_service.calculate_multilateral_netting(batch.id, &currency, &transactions);

    // Persist positions
    let persisted = netting_service
        .persist_positions(&result.positions)
        .await
        .expect("Failed to persist positions");

    assert_eq!(persisted.len(), 2);

    // Retrieve positions
    let retrieved = netting_service
        .get_batch_positions(batch.id)
        .await
        .expect("Failed to get positions");

    assert_eq!(retrieved.len(), 2);

    // Get summary
    let summary = netting_service
        .get_batch_summary(batch.id)
        .await
        .expect("Failed to get summary");

    assert_eq!(summary.participant_count, 2);
    assert_eq!(summary.net_receivers, 1);
    assert_eq!(summary.net_payers, 1);
}

#[tokio::test]
async fn test_netting_service_generate_report() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create banks
    let bank_a = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-A-{}", Uuid::new_v4()),
            name: "Bank A".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(200000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank A");

    let bank_b = account_service
        .create_account(CreateAccountRequest {
            external_id: format!("BANK-B-{}", Uuid::new_v4()),
            name: "Bank B".to_string(),
            account_type: AccountType::Asset,
            currency: currency.clone(),
            initial_balance: Some(dec!(200000)),
            metadata: None,
        })
        .await
        .expect("Failed to create Bank B");

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // Create transactions
    let tx1 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_a.id,
            bank_b.id,
            dec!(100000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 1");
    batch_service.assign_transaction_to_batch(tx1.transaction.id, batch.id).await.unwrap();

    let tx2 = ledger_service
        .process_payment(LedgerTransactionRequest::payment(
            format!("PAY-{}", Uuid::new_v4()),
            bank_b.id,
            bank_a.id,
            dec!(75000),
            &currency,
            format!("IDEM-{}", Uuid::new_v4()),
        ))
        .await
        .expect("Failed to process payment 2");
    batch_service.assign_transaction_to_batch(tx2.transaction.id, batch.id).await.unwrap();

    // Get transactions
    let transactions = batch_service.get_batch_transactions(batch.id).await.unwrap();

    // Generate report
    let report = netting_service.generate_report(batch.id, &currency, &transactions);

    assert_eq!(report.batch_id, batch.id);
    assert_eq!(report.total_transactions, 2);
    assert_eq!(report.gross_volume, dec!(350000)); // 175k * 2 (each participant's gross)
    assert!(report.bilateral_result.is_some());
    assert!(report.multilateral_result.is_some());
    assert!(report.reduction_percentage > dec!(85));
}

#[tokio::test]
async fn test_netting_service_high_efficiency_scenario() {
    let pool = common::setup_test_db().await;
    let currency = unique_currency();

    let account_service = AccountService::new(pool.clone());
    let ledger_service = LedgerService::new(pool.clone());
    let netting_service = NettingService::new(pool.clone());
    let batch_service = BatchService::new(pool.clone());

    // Create 4 banks for a more complex scenario
    let mut banks = Vec::new();
    for i in 0..4 {
        let bank = account_service
            .create_account(CreateAccountRequest {
                external_id: format!("BANK-{}-{}", i, Uuid::new_v4()),
                name: format!("Bank {}", i),
                account_type: AccountType::Asset,
                currency: currency.clone(),
                initial_balance: Some(dec!(1000000)),
                metadata: None,
            })
            .await
            .expect("Failed to create bank");
        banks.push(bank);
    }

    // Create batch
    let batch = batch_service
        .create_batch(CreateBatchRequest::for_today(&currency, 24))
        .await
        .expect("Failed to create batch");

    // Create multiple transactions between banks
    // Bank 0 -> Bank 1: 50,000
    // Bank 1 -> Bank 2: 40,000
    // Bank 2 -> Bank 3: 30,000
    // Bank 3 -> Bank 0: 45,000
    // Bank 0 -> Bank 2: 25,000
    // Bank 1 -> Bank 3: 35,000
    let tx_pairs = vec![
        (0, 1, dec!(50000)),
        (1, 2, dec!(40000)),
        (2, 3, dec!(30000)),
        (3, 0, dec!(45000)),
        (0, 2, dec!(25000)),
        (1, 3, dec!(35000)),
    ];

    for (from, to, amount) in tx_pairs {
        let tx = ledger_service
            .process_payment(LedgerTransactionRequest::payment(
                format!("PAY-{}", Uuid::new_v4()),
                banks[from].id,
                banks[to].id,
                amount,
                &currency,
                format!("IDEM-{}", Uuid::new_v4()),
            ))
            .await
            .expect("Failed to process payment");
        batch_service.assign_transaction_to_batch(tx.transaction.id, batch.id).await.unwrap();
    }

    // Get transactions
    let transactions = batch_service.get_batch_transactions(batch.id).await.unwrap();
    assert_eq!(transactions.len(), 6);

    // Calculate multilateral netting
    let result = netting_service.calculate_multilateral_netting(batch.id, &currency, &transactions);

    // Verify conservation of money
    let total_net: Decimal = result.positions.iter().map(|p| p.net_position).sum();
    assert_eq!(total_net, Decimal::ZERO);

    // Gross volume = 225,000 (sum of all transactions)
    // Net volume should be much smaller due to netting
    assert!(result.total_net_volume < result.total_gross_volume);

    // Generate report
    let report = netting_service.generate_report(batch.id, &currency, &transactions);
    assert_eq!(report.total_transactions, 6);
    assert!(report.reduction_percentage > dec!(0)); // Some reduction expected
}

#[tokio::test]
async fn test_netting_position_model() {
    let batch_id = Uuid::new_v4();
    let participant_id = Uuid::new_v4();

    let mut position = NettingPosition::new(batch_id, participant_id, "USD".to_string());

    // Add receivables and payables
    position.add_receivable(dec!(100000));
    position.add_payable(dec!(75000));

    assert_eq!(position.gross_receivable, dec!(100000));
    assert_eq!(position.gross_payable, dec!(75000));
    assert_eq!(position.net_position, dec!(25000));
    assert!(position.is_net_receiver());
    assert_eq!(position.gross_volume(), dec!(175000));
    assert_eq!(position.netting_benefit(), dec!(150000));
}

#[tokio::test]
async fn test_netting_summary_from_positions() {
    let batch_id = Uuid::new_v4();

    let mut pos_a = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
    pos_a.add_receivable(dec!(100000));
    pos_a.add_payable(dec!(75000));

    let mut pos_b = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
    pos_b.add_receivable(dec!(75000));
    pos_b.add_payable(dec!(100000));

    let positions = vec![pos_a, pos_b];
    let summary = NettingSummary::from_positions(batch_id, "USD".to_string(), &positions);

    assert_eq!(summary.participant_count, 2);
    assert_eq!(summary.net_receivers, 1);
    assert_eq!(summary.net_payers, 1);
    assert_eq!(summary.balanced_participants, 0);
    // Efficiency should be ~85.7%
    assert!(summary.netting_efficiency() > dec!(85));
}
