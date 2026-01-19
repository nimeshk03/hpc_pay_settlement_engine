use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Configuration for logging.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: String,
    pub format: LogFormat,
    pub include_target: bool,
    pub include_file: bool,
    pub include_line: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Pretty,
            include_target: true,
            include_file: false,
            include_line: false,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
    Compact,
}

impl From<&str> for LogFormat {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            "compact" => LogFormat::Compact,
            _ => LogFormat::Pretty,
        }
    }
}

/// Request span for correlation ID tracking.
#[derive(Debug, Clone)]
pub struct RequestSpan {
    pub request_id: String,
    pub method: String,
    pub path: String,
}

impl RequestSpan {
    pub fn new(request_id: String, method: String, path: String) -> Self {
        Self {
            request_id,
            method,
            path,
        }
    }
}

/// Initializes the logging system with the given configuration.
pub fn init_logging(config: &LogConfig) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    match config.format {
        LogFormat::Json => {
            let fmt_layer = fmt::layer()
                .json()
                .with_target(config.include_target)
                .with_file(config.include_file)
                .with_line_number(config.include_line)
                .with_span_events(FmtSpan::CLOSE);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
        LogFormat::Compact => {
            let fmt_layer = fmt::layer()
                .compact()
                .with_target(config.include_target)
                .with_file(config.include_file)
                .with_line_number(config.include_line);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
        LogFormat::Pretty => {
            let fmt_layer = fmt::layer()
                .pretty()
                .with_target(config.include_target)
                .with_file(config.include_file)
                .with_line_number(config.include_line);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
    }

    tracing::info!("Logging initialized with level: {}", config.level);
}

/// Masks sensitive data in strings (e.g., account numbers, keys).
pub fn mask_sensitive(value: &str, visible_chars: usize) -> String {
    if value.len() <= visible_chars * 2 {
        return "*".repeat(value.len());
    }
    
    let prefix = &value[..visible_chars];
    let suffix = &value[value.len() - visible_chars..];
    let masked_len = value.len() - (visible_chars * 2);
    
    format!("{}{}{}",prefix, "*".repeat(masked_len), suffix)
}

/// Masks a UUID, showing only first and last 4 characters.
pub fn mask_uuid(uuid: &uuid::Uuid) -> String {
    let s = uuid.to_string();
    mask_sensitive(&s, 4)
}

/// Masks currency amounts for logging (shows magnitude only).
pub fn mask_amount(amount: &rust_decimal::Decimal) -> String {
    let abs = amount.abs();
    if abs >= rust_decimal::Decimal::from(1_000_000) {
        "***M+".to_string()
    } else if abs >= rust_decimal::Decimal::from(1_000) {
        "***K+".to_string()
    } else {
        "***".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_sensitive_short_string() {
        assert_eq!(mask_sensitive("abc", 2), "***");
    }

    #[test]
    fn test_mask_sensitive_long_string() {
        assert_eq!(mask_sensitive("1234567890", 2), "12******90");
    }

    #[test]
    fn test_mask_uuid() {
        let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let masked = mask_uuid(&uuid);
        assert!(masked.starts_with("550e"));
        assert!(masked.ends_with("0000"));
        assert!(masked.contains("*"));
    }

    #[test]
    fn test_mask_amount() {
        use rust_decimal::Decimal;
        assert_eq!(mask_amount(&Decimal::from(500)), "***");
        assert_eq!(mask_amount(&Decimal::from(5000)), "***K+");
        assert_eq!(mask_amount(&Decimal::from(5_000_000)), "***M+");
    }

    #[test]
    fn test_log_format_from_str() {
        assert_eq!(LogFormat::from("json"), LogFormat::Json);
        assert_eq!(LogFormat::from("JSON"), LogFormat::Json);
        assert_eq!(LogFormat::from("compact"), LogFormat::Compact);
        assert_eq!(LogFormat::from("pretty"), LogFormat::Pretty);
        assert_eq!(LogFormat::from("unknown"), LogFormat::Pretty);
    }
}
