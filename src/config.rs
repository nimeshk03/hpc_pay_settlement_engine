use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub kafka: KafkaSettings,
    pub application: ApplicationSettings,
    #[serde(default)]
    pub cache: CacheSettings,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
    pub pool_size: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_acquire_timeout")]
    pub acquire_timeout_secs: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime_secs: u64,
}

fn default_min_connections() -> u32 { 5 }
fn default_acquire_timeout() -> u64 { 5 }
fn default_idle_timeout() -> u64 { 300 }
fn default_max_lifetime() -> u64 { 1800 }

#[derive(Debug, Deserialize)]
pub struct RedisSettings {
    pub url: String,
    #[serde(default = "default_redis_pool_size")]
    pub pool_size: u32,
}

fn default_redis_pool_size() -> u32 { 10 }

#[derive(Debug, Deserialize)]
pub struct CacheSettings {
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    #[serde(default = "default_balance_ttl")]
    pub balance_ttl_secs: u64,
    #[serde(default = "default_cache_prefix")]
    pub key_prefix: String,
}

fn default_cache_enabled() -> bool { true }
fn default_balance_ttl() -> u64 { 60 }
fn default_cache_prefix() -> String { "settlement".to_string() }

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_cache_enabled(),
            balance_ttl_secs: default_balance_ttl(),
            key_prefix: default_cache_prefix(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct KafkaSettings {
    pub brokers: String,
    pub topic_prefix: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationSettings {
    pub port: u16,
    pub log_level: String,
}

impl Settings {
    pub fn new() -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .add_source(config::File::with_name("config/default"))
            .add_source(config::File::with_name("config/local").required(false))
            .add_source(config::Environment::with_prefix("APP").separator("__"));

        builder.build()?.try_deserialize()
    }
}
