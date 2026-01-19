use settlement_engine::api::{create_router, AppState};
use settlement_engine::config::Settings;
use settlement_engine::observability::{
    init_logging, init_metrics, LogConfig, LogFormat, HealthChecker,
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration first (needed for log level)
    let settings = Settings::new()?;

    // Initialize structured logging
    let log_config = LogConfig {
        level: settings.application.log_level.clone(),
        format: LogFormat::from(
            std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string()).as_str()
        ),
        include_target: true,
        include_file: false,
        include_line: false,
    };
    init_logging(&log_config);

    // Initialize Prometheus metrics
    let metrics_handle = init_metrics();
    info!("Configuration loaded, metrics initialized");

    // Connect to PostgreSQL
    info!("Connecting to database at {}...", settings.database.url);
    let pool = PgPoolOptions::new()
        .max_connections(settings.database.pool_size)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&settings.database.url)
        .await?;

    info!("Database connection established");

    // Run migrations
    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Migrations applied successfully");

    // Connect to Redis
    info!("Connecting to Redis at {}...", settings.redis.url);
    let redis_client = redis::Client::open(settings.redis.url.clone())?;
    let mut con = redis_client.get_multiplexed_async_connection().await?;
    let _: () = redis::cmd("PING").query_async(&mut con).await?;
    info!("Redis connection established");

    // Connect to Kafka (with timeout, preserve client)
    info!("Checking Kafka connection...");
    use rskafka::client::ClientBuilder;

    let connection = vec![settings.kafka.brokers.clone()];
    let kafka_client = match tokio::time::timeout(
        Duration::from_secs(3),
        ClientBuilder::new(connection).build()
    )
    .await
    {
        Ok(Ok(client)) => {
            info!("Kafka client created successfully");
            Some(Arc::new(client))
        }
        Ok(Err(e)) => {
            tracing::warn!("Kafka connection failed: {}. Continuing without Kafka.", e);
            None
        }
        Err(_) => {
            tracing::warn!("Kafka connection timed out. Continuing without Kafka.");
            None
        }
    };

    if kafka_client.is_none() {
        info!("Kafka not available, continuing without event streaming");
    }

    info!("System startup verification complete.");

    // Create health checker
    let health_checker = Arc::new(HealthChecker::new(
        pool.clone(),
        redis_client.clone(),
        kafka_client.clone(),
    ));

    // Create application state with metrics handle and health checker
    let state = AppState::new(pool, redis_client, kafka_client)
        .with_metrics(metrics_handle)
        .with_health_checker(health_checker);

    // Create API router
    let app = create_router(state);

    // Start HTTP server
    let addr = format!("0.0.0.0:{}", settings.application.port);
    info!("Starting HTTP server on {}", addr);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
