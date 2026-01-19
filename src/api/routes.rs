use axum::{
    routing::{get, post},
    Router,
};
use rskafka::client::Client as KafkaClient;
use sqlx::PgPool;
use std::sync::Arc;

use super::handlers;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub redis_client: redis::Client,
    pub kafka_client: Option<Arc<KafkaClient>>,
}

impl AppState {
    pub fn new(pool: PgPool, redis_client: redis::Client, kafka_client: Option<Arc<KafkaClient>>) -> Self {
        Self {
            pool,
            redis_client,
            kafka_client,
        }
    }

    /// Returns true if Kafka is connected.
    pub fn kafka_connected(&self) -> bool {
        self.kafka_client.is_some()
    }
}

/// Creates the main API router with all routes.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health endpoints
        .route("/health", get(handlers::health_check))
        .route("/ready", get(handlers::readiness_check))
        .route("/live", get(handlers::liveness_check))
        // Account endpoints
        .route("/accounts", post(handlers::create_account))
        .route("/accounts/:id", get(handlers::get_account))
        .route("/accounts/:id/balance", get(handlers::get_account_balance))
        .route("/accounts/:id/ledger", get(handlers::get_account_ledger))
        // Transaction endpoints
        .route("/transactions", post(handlers::create_transaction))
        .route("/transactions", get(handlers::list_transactions))
        .route("/transactions/:id", get(handlers::get_transaction))
        .route("/transactions/:id/reverse", post(handlers::reverse_transaction))
        // Batch endpoints
        .route("/batches", get(handlers::list_batches))
        .route("/batches/:id", get(handlers::get_batch))
        .route("/batches/:id/process", post(handlers::process_batch))
        .route("/batches/:id/positions", get(handlers::get_batch_positions))
        .with_state(state)
}

