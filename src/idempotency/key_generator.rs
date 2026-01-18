use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Configuration for idempotency key generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyGeneratorConfig {
    /// Time window in seconds for key uniqueness (default: 24 hours)
    pub time_window_seconds: i64,
    /// Whether to include timestamp in key generation
    pub include_timestamp: bool,
    /// Prefix for generated keys
    pub key_prefix: String,
}

impl Default for KeyGeneratorConfig {
    fn default() -> Self {
        Self {
            time_window_seconds: 86400, // 24 hours
            include_timestamp: true,
            key_prefix: "idem".to_string(),
        }
    }
}

/// Attributes used to generate an idempotency key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyAttributes {
    pub client_id: String,
    pub operation_type: String,
    pub source_account: Option<Uuid>,
    pub destination_account: Option<Uuid>,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub reference: Option<String>,
}

impl IdempotencyAttributes {
    pub fn new(client_id: impl Into<String>, operation_type: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            operation_type: operation_type.into(),
            source_account: None,
            destination_account: None,
            amount: None,
            currency: None,
            reference: None,
        }
    }

    pub fn with_source_account(mut self, account_id: Uuid) -> Self {
        self.source_account = Some(account_id);
        self
    }

    pub fn with_destination_account(mut self, account_id: Uuid) -> Self {
        self.destination_account = Some(account_id);
        self
    }

    pub fn with_amount(mut self, amount: impl Into<String>) -> Self {
        self.amount = Some(amount.into());
        self
    }

    pub fn with_currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }
}

/// Generator for idempotency keys using SHA-256 hashing.
#[derive(Debug, Clone)]
pub struct IdempotencyKeyGenerator {
    config: KeyGeneratorConfig,
}

impl IdempotencyKeyGenerator {
    pub fn new(config: KeyGeneratorConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(KeyGeneratorConfig::default())
    }

    /// Generates an idempotency key from the given attributes.
    pub fn generate(&self, attributes: &IdempotencyAttributes) -> String {
        self.generate_at(attributes, Utc::now())
    }

    /// Generates an idempotency key at a specific timestamp.
    pub fn generate_at(&self, attributes: &IdempotencyAttributes, timestamp: DateTime<Utc>) -> String {
        let mut hasher = Sha256::new();

        // Add client ID and operation type
        hasher.update(attributes.client_id.as_bytes());
        hasher.update(b"|");
        hasher.update(attributes.operation_type.as_bytes());

        // Add optional fields
        if let Some(ref source) = attributes.source_account {
            hasher.update(b"|src:");
            hasher.update(source.to_string().as_bytes());
        }

        if let Some(ref dest) = attributes.destination_account {
            hasher.update(b"|dst:");
            hasher.update(dest.to_string().as_bytes());
        }

        if let Some(ref amount) = attributes.amount {
            hasher.update(b"|amt:");
            hasher.update(amount.as_bytes());
        }

        if let Some(ref currency) = attributes.currency {
            hasher.update(b"|cur:");
            hasher.update(currency.as_bytes());
        }

        if let Some(ref reference) = attributes.reference {
            hasher.update(b"|ref:");
            hasher.update(reference.as_bytes());
        }

        // Add timestamp window if configured
        if self.config.include_timestamp {
            let window = self.get_time_window(timestamp);
            hasher.update(b"|tw:");
            hasher.update(window.to_string().as_bytes());
        }

        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);

        format!("{}_{}", self.config.key_prefix, hash_hex)
    }

    /// Generates a key from a client-provided idempotency key.
    /// This normalizes the key format while preserving uniqueness.
    pub fn from_client_key(&self, client_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(client_key.as_bytes());
        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);

        format!("{}_{}", self.config.key_prefix, hash_hex)
    }

    /// Gets the time window bucket for a given timestamp.
    fn get_time_window(&self, timestamp: DateTime<Utc>) -> i64 {
        timestamp.timestamp() / self.config.time_window_seconds
    }

    /// Checks if a key is within the current time window.
    pub fn is_within_window(&self, key_timestamp: DateTime<Utc>) -> bool {
        let now = Utc::now();
        let window_duration = Duration::seconds(self.config.time_window_seconds);
        now.signed_duration_since(key_timestamp) < window_duration
    }

    /// Gets the expiration time for a key created now.
    pub fn get_expiration(&self) -> DateTime<Utc> {
        Utc::now() + Duration::seconds(self.config.time_window_seconds)
    }

    /// Gets the TTL in seconds for Redis storage.
    pub fn get_ttl_seconds(&self) -> i64 {
        self.config.time_window_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_key_generation_consistency() {
        let generator = IdempotencyKeyGenerator::with_default_config();
        let attrs = IdempotencyAttributes::new("client-123", "payment")
            .with_source_account(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap())
            .with_destination_account(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap())
            .with_amount("100.00")
            .with_currency("USD");

        let timestamp = DateTime::parse_from_rfc3339("2026-01-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let key1 = generator.generate_at(&attrs, timestamp);
        let key2 = generator.generate_at(&attrs, timestamp);

        assert_eq!(key1, key2);
        assert!(key1.starts_with("idem_"));
    }

    #[test]
    fn test_different_attributes_different_keys() {
        let generator = IdempotencyKeyGenerator::with_default_config();
        let timestamp = Utc::now();

        let attrs1 = IdempotencyAttributes::new("client-123", "payment")
            .with_amount("100.00");

        let attrs2 = IdempotencyAttributes::new("client-123", "payment")
            .with_amount("200.00");

        let key1 = generator.generate_at(&attrs1, timestamp);
        let key2 = generator.generate_at(&attrs2, timestamp);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_time_window_affects_key() {
        let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
            time_window_seconds: 3600, // 1 hour
            include_timestamp: true,
            key_prefix: "test".to_string(),
        });

        let attrs = IdempotencyAttributes::new("client-123", "payment");

        let time1 = DateTime::parse_from_rfc3339("2026-01-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let time2 = DateTime::parse_from_rfc3339("2026-01-18T13:30:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let key1 = generator.generate_at(&attrs, time1);
        let key2 = generator.generate_at(&attrs, time2);

        // Different time windows should produce different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_same_time_window_same_key() {
        let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
            time_window_seconds: 3600, // 1 hour
            include_timestamp: true,
            key_prefix: "test".to_string(),
        });

        let attrs = IdempotencyAttributes::new("client-123", "payment");

        let time1 = DateTime::parse_from_rfc3339("2026-01-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let time2 = DateTime::parse_from_rfc3339("2026-01-18T12:30:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let key1 = generator.generate_at(&attrs, time1);
        let key2 = generator.generate_at(&attrs, time2);

        // Same time window should produce same key
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_from_client_key() {
        let generator = IdempotencyKeyGenerator::with_default_config();

        let key1 = generator.from_client_key("my-unique-request-123");
        let key2 = generator.from_client_key("my-unique-request-123");
        let key3 = generator.from_client_key("different-request");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert!(key1.starts_with("idem_"));
    }

    #[test]
    fn test_key_length() {
        let generator = IdempotencyKeyGenerator::with_default_config();
        let attrs = IdempotencyAttributes::new("client", "op");
        let key = generator.generate(&attrs);

        // SHA-256 produces 64 hex chars + prefix + underscore
        assert!(key.len() > 64);
    }

    #[test]
    fn test_is_within_window() {
        let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
            time_window_seconds: 3600,
            include_timestamp: true,
            key_prefix: "test".to_string(),
        });

        let recent = Utc::now() - Duration::minutes(30);
        let old = Utc::now() - Duration::hours(2);

        assert!(generator.is_within_window(recent));
        assert!(!generator.is_within_window(old));
    }

    #[test]
    fn test_without_timestamp() {
        let generator = IdempotencyKeyGenerator::new(KeyGeneratorConfig {
            time_window_seconds: 3600,
            include_timestamp: false,
            key_prefix: "notimed".to_string(),
        });

        let attrs = IdempotencyAttributes::new("client-123", "payment");

        let time1 = DateTime::parse_from_rfc3339("2026-01-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let time2 = DateTime::parse_from_rfc3339("2026-01-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let key1 = generator.generate_at(&attrs, time1);
        let key2 = generator.generate_at(&attrs, time2);

        // Without timestamp, keys should be the same regardless of time
        assert_eq!(key1, key2);
    }
}
