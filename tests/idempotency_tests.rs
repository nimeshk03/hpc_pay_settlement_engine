mod common;

use settlement_engine::idempotency::{
    IdempotencyAttributes, IdempotencyKeyGenerator, IdempotencyRecord, IdempotencyStatus,
    KeyGeneratorConfig, PostgresIdempotencyStore,
};
use uuid::Uuid;

#[tokio::test]
async fn test_postgres_idempotency_store_acquire() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    let record = IdempotencyRecord::new(
        format!("idem_{}", Uuid::new_v4()),
        "client-123".to_string(),
        "payment".to_string(),
        "hash123".to_string(),
        86400,
    );

    // First acquire should succeed (new request)
    let result = store.try_acquire(&record).await.expect("Failed to acquire");
    assert!(result.is_none(), "Expected None for new request");

    // Second acquire with same key should return existing record
    let record2 = IdempotencyRecord::new(
        record.idempotency_key.clone(),
        "client-123".to_string(),
        "payment".to_string(),
        "hash123".to_string(),
        86400,
    );

    let result2 = store.try_acquire(&record2).await.expect("Failed to acquire");
    assert!(result2.is_some(), "Expected Some for duplicate request");
    assert_eq!(result2.unwrap().idempotency_key, record.idempotency_key);

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_postgres_idempotency_store_find_by_key() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    let key = format!("idem_{}", Uuid::new_v4());
    let record = IdempotencyRecord::new(
        key.clone(),
        "client-456".to_string(),
        "transfer".to_string(),
        "hash456".to_string(),
        86400,
    );

    // Insert record
    store.try_acquire(&record).await.expect("Failed to acquire");

    // Find by key
    let found = store.find_by_key(&key).await.expect("Failed to find");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.client_id, "client-456");
    assert_eq!(found.operation_type, "transfer");
    assert_eq!(found.status, IdempotencyStatus::Processing);

    // Find non-existent key
    let not_found = store
        .find_by_key("non-existent-key")
        .await
        .expect("Failed to find");
    assert!(not_found.is_none());

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_postgres_idempotency_store_mark_completed() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    let key = format!("idem_{}", Uuid::new_v4());
    let record = IdempotencyRecord::new(
        key.clone(),
        "client-789".to_string(),
        "payment".to_string(),
        "hash789".to_string(),
        86400,
    );

    // Insert record
    store.try_acquire(&record).await.expect("Failed to acquire");

    // Mark as completed
    let response_data = serde_json::json!({
        "transaction_id": "tx-123",
        "status": "success"
    });

    let completed = store
        .mark_completed(&key, response_data.clone())
        .await
        .expect("Failed to mark completed");

    assert!(completed.is_some());
    let completed = completed.unwrap();
    assert_eq!(completed.status, IdempotencyStatus::Completed);
    assert!(completed.response_data.is_some());
    assert!(completed.completed_at.is_some());

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_postgres_idempotency_store_mark_failed() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    let key = format!("idem_{}", Uuid::new_v4());
    let record = IdempotencyRecord::new(
        key.clone(),
        "client-fail".to_string(),
        "payment".to_string(),
        "hashfail".to_string(),
        86400,
    );

    // Insert record
    store.try_acquire(&record).await.expect("Failed to acquire");

    // Mark as failed
    let failed = store
        .mark_failed(&key, "Insufficient funds")
        .await
        .expect("Failed to mark failed");

    assert!(failed.is_some());
    let failed = failed.unwrap();
    assert_eq!(failed.status, IdempotencyStatus::Failed);
    assert_eq!(failed.error_message, Some("Insufficient funds".to_string()));
    assert!(failed.completed_at.is_some());

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_postgres_idempotency_store_cleanup_expired() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    // Insert a record with very short TTL (already expired)
    let key = format!("idem_expired_{}", Uuid::new_v4());
    let mut record = IdempotencyRecord::new(
        key.clone(),
        "client-expired".to_string(),
        "payment".to_string(),
        "hashexpired".to_string(),
        -1, // Negative TTL means already expired
    );
    record.expires_at = chrono::Utc::now() - chrono::Duration::hours(1);

    // Insert directly via SQL to set expired timestamp
    sqlx::query(
        r#"
        INSERT INTO idempotency_keys (id, idempotency_key, client_id, operation_type, status, request_hash, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(record.id)
    .bind(&record.idempotency_key)
    .bind(&record.client_id)
    .bind(&record.operation_type)
    .bind(&record.status)
    .bind(&record.request_hash)
    .bind(record.created_at)
    .bind(record.expires_at)
    .execute(&pool)
    .await
    .expect("Failed to insert expired record");

    // Verify record exists
    let found = store.find_by_key(&key).await.expect("Failed to find");
    assert!(found.is_some());

    // Run cleanup
    let deleted = store.cleanup_expired().await.expect("Failed to cleanup");
    assert!(deleted >= 1);

    // Verify record is deleted
    let found_after = store.find_by_key(&key).await.expect("Failed to find");
    assert!(found_after.is_none());

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_postgres_idempotency_store_count_by_status() {
    let pool = common::setup_test_db().await;
    cleanup_idempotency_data(&pool).await;

    let store = PostgresIdempotencyStore::new(pool.clone());

    // Insert multiple records with different statuses
    for i in 0..3 {
        let record = IdempotencyRecord::new(
            format!("idem_count_{}_{}", i, Uuid::new_v4()),
            "client-count".to_string(),
            "payment".to_string(),
            format!("hash{}", i),
            86400,
        );
        store.try_acquire(&record).await.expect("Failed to acquire");
    }

    // Count processing
    let processing_count = store
        .count_by_status(IdempotencyStatus::Processing)
        .await
        .expect("Failed to count");
    assert!(processing_count >= 3);

    // Mark one as completed
    let key = store
        .find_by_key(&format!("idem_count_0_{}", ""))
        .await
        .ok()
        .flatten();
    
    // Count completed (should be 0 initially)
    let completed_count = store
        .count_by_status(IdempotencyStatus::Completed)
        .await
        .expect("Failed to count");
    assert_eq!(completed_count, 0);

    cleanup_idempotency_data(&pool).await;
}

#[tokio::test]
async fn test_key_generator_consistency() {
    let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
        time_window_seconds: 86400,
        include_timestamp: false,
        key_prefix: "test".to_string(),
    });

    let attrs = IdempotencyAttributes::new("client-123", "payment")
        .with_source_account(Uuid::new_v4())
        .with_amount("100.00")
        .with_currency("USD");

    let key1 = generator.generate(&attrs);
    let key2 = generator.generate(&attrs);

    assert_eq!(key1, key2, "Same attributes should produce same key");
    assert!(key1.starts_with("test_"));
}

#[tokio::test]
async fn test_key_generator_different_attributes() {
    let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
        time_window_seconds: 86400,
        include_timestamp: false,
        key_prefix: "test".to_string(),
    });

    let attrs1 = IdempotencyAttributes::new("client-123", "payment")
        .with_amount("100.00");

    let attrs2 = IdempotencyAttributes::new("client-123", "payment")
        .with_amount("200.00");

    let key1 = generator.generate(&attrs1);
    let key2 = generator.generate(&attrs2);

    assert_ne!(key1, key2, "Different amounts should produce different keys");
}

#[tokio::test]
async fn test_idempotency_record_expiration() {
    let record = IdempotencyRecord::new(
        "test-key".to_string(),
        "client".to_string(),
        "op".to_string(),
        "hash".to_string(),
        86400,
    );

    assert!(!record.is_expired());
    assert!(!record.is_completed());
    assert!(!record.is_failed());
    assert_eq!(record.status, IdempotencyStatus::Processing);
}

async fn cleanup_idempotency_data(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM idempotency_keys")
        .execute(pool)
        .await
        .ok();
}
