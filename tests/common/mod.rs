use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

pub async fn setup_test_db() -> PgPool {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/settlement_engine".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

pub async fn cleanup_test_data(pool: &PgPool) {
    sqlx::query("DELETE FROM ledger_entries")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM netting_positions")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM transactions")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM account_balances")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM settlement_batches")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM accounts")
        .execute(pool)
        .await
        .ok();
}
