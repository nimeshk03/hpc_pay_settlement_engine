// Configuration module placeholder
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub kafka: KafkaSettings,
    pub application: ApplicationSettings,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
    pub pool_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct RedisSettings {
    pub url: String,
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
