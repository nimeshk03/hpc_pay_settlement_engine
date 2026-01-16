use settlement_engine::config::Settings;
use settlement_engine::error::AppError;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Load configuration
    let settings = Settings::new()?;
    info!("Configuration loaded");

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
    let client = redis::Client::open(settings.redis.url)?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let _: () = redis::cmd("PING").query_async(&mut con).await?;
    info!("Redis connection established");

    // Connect to Kafka (Basic check)
    info!("Checking Kafka connection...");
    use rskafka::client::ClientBuilder;

    let connection = vec![settings.kafka.brokers];
    let _client = ClientBuilder::new(connection)
        .build()
        .await
        .map_err(AppError::Kafka)?;

    info!("Kafka client created successfully");

    info!("System startup verification complete: All services healthy.");
    
    Ok(())
}
