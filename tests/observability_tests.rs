use settlement_engine::observability::{
    LogConfig, LogFormat, mask_sensitive, mask_uuid, mask_amount,
    Metrics, LatencyTimer, HealthStatus, DependencyHealth, AggregatedHealth,
};
use rust_decimal::Decimal;
use uuid::Uuid;

#[test]
fn test_log_config_default() {
    let config = LogConfig::default();
    assert_eq!(config.level, "info");
    assert!(config.include_target);
    assert!(!config.include_file);
    assert!(!config.include_line);
}

#[test]
fn test_log_format_from_str() {
    assert_eq!(LogFormat::from("json"), LogFormat::Json);
    assert_eq!(LogFormat::from("JSON"), LogFormat::Json);
    assert_eq!(LogFormat::from("compact"), LogFormat::Compact);
    assert_eq!(LogFormat::from("COMPACT"), LogFormat::Compact);
    assert_eq!(LogFormat::from("pretty"), LogFormat::Pretty);
    assert_eq!(LogFormat::from("unknown"), LogFormat::Pretty);
}

#[test]
fn test_mask_sensitive_short_string() {
    let result = mask_sensitive("abc", 2);
    assert_eq!(result, "***");
}

#[test]
fn test_mask_sensitive_long_string() {
    let result = mask_sensitive("1234567890", 2);
    assert_eq!(result, "12******90");
}

#[test]
fn test_mask_sensitive_exact_boundary() {
    let result = mask_sensitive("1234", 2);
    assert_eq!(result, "****");
}

#[test]
fn test_mask_uuid() {
    let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    let masked = mask_uuid(&uuid);
    assert!(masked.starts_with("550e"));
    assert!(masked.ends_with("0000"));
    assert!(masked.contains("*"));
    assert_eq!(masked.len(), 36);
}

#[test]
fn test_mask_amount_small() {
    let amount = Decimal::from(500);
    assert_eq!(mask_amount(&amount), "***");
}

#[test]
fn test_mask_amount_thousands() {
    let amount = Decimal::from(5000);
    assert_eq!(mask_amount(&amount), "***K+");
}

#[test]
fn test_mask_amount_millions() {
    let amount = Decimal::from(5_000_000);
    assert_eq!(mask_amount(&amount), "***M+");
}

#[test]
fn test_mask_amount_negative() {
    let amount = Decimal::from(-5_000_000);
    assert_eq!(mask_amount(&amount), "***M+");
}

#[test]
fn test_metrics_creation() {
    let metrics = Metrics::new();
    metrics.record_transaction_created("PAYMENT", "USD");
    metrics.record_transaction_settled("PAYMENT", "USD");
    metrics.record_transaction_failed("PAYMENT", "insufficient_funds");
    metrics.record_transaction_reversed("PAYMENT");
}

#[test]
fn test_metrics_latency_recording() {
    let metrics = Metrics::new();
    metrics.record_ledger_write_latency(5.5);
    metrics.record_balance_query_latency(1.2, true);
    metrics.record_balance_query_latency(10.5, false);
}

#[test]
fn test_metrics_batch_recording() {
    let metrics = Metrics::new();
    metrics.record_batch_created("USD");
    metrics.record_batch_processed("USD", 100);
    metrics.record_batch_failed("USD", "timeout");
    metrics.record_batch_processing_latency(500.0);
}

#[test]
fn test_metrics_netting_recording() {
    let metrics = Metrics::new();
    metrics.record_netting_calculation(10, 45);
    metrics.record_netting_efficiency(1000.0, 300.0);
    metrics.record_netting_latency(25.0);
}

#[test]
fn test_metrics_gauges() {
    let metrics = Metrics::new();
    metrics.set_active_batches(5);
    metrics.set_pending_transactions(100);
}

#[test]
fn test_metrics_http_request() {
    let metrics = Metrics::new();
    metrics.record_http_request("GET", "/health", 200, 5.0);
    metrics.record_http_request("POST", "/transactions", 201, 50.0);
    metrics.record_http_request("GET", "/accounts/123", 404, 2.0);
}

#[test]
fn test_metrics_db_query() {
    let metrics = Metrics::new();
    metrics.record_db_query("select", 5.0, true);
    metrics.record_db_query("insert", 10.0, true);
    metrics.record_db_query("update", 15.0, false);
}

#[test]
fn test_metrics_redis_operation() {
    let metrics = Metrics::new();
    metrics.record_redis_operation("get", 1.0, true);
    metrics.record_redis_operation("set", 2.0, true);
    metrics.record_redis_operation("del", 1.5, false);
}

#[test]
fn test_metrics_kafka_message() {
    let metrics = Metrics::new();
    metrics.record_kafka_message("transactions", true);
    metrics.record_kafka_message("batches", false);
}

#[test]
fn test_latency_timer() {
    let timer = LatencyTimer::new();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let elapsed = timer.elapsed_ms();
    assert!(elapsed >= 10.0);
    assert!(elapsed < 100.0);
}

#[test]
fn test_health_status_checks() {
    assert!(HealthStatus::Healthy.is_healthy());
    assert!(!HealthStatus::Healthy.is_degraded());
    assert!(!HealthStatus::Healthy.is_unhealthy());

    assert!(!HealthStatus::Degraded.is_healthy());
    assert!(HealthStatus::Degraded.is_degraded());
    assert!(!HealthStatus::Degraded.is_unhealthy());

    assert!(!HealthStatus::Unhealthy.is_healthy());
    assert!(!HealthStatus::Unhealthy.is_degraded());
    assert!(HealthStatus::Unhealthy.is_unhealthy());
}

#[test]
fn test_dependency_health_healthy() {
    let health = DependencyHealth::healthy("database", 5.0);
    assert_eq!(health.name, "database");
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.latency_ms, Some(5.0));
    assert!(health.message.is_none());
}

#[test]
fn test_dependency_health_degraded() {
    let health = DependencyHealth::degraded("redis", "High latency");
    assert_eq!(health.name, "redis");
    assert_eq!(health.status, HealthStatus::Degraded);
    assert!(health.latency_ms.is_none());
    assert_eq!(health.message, Some("High latency".to_string()));
}

#[test]
fn test_dependency_health_unhealthy() {
    let health = DependencyHealth::unhealthy("kafka", "Connection refused");
    assert_eq!(health.name, "kafka");
    assert_eq!(health.status, HealthStatus::Unhealthy);
    assert!(health.latency_ms.is_none());
    assert_eq!(health.message, Some("Connection refused".to_string()));
}

#[test]
fn test_aggregated_health_all_healthy() {
    let dependencies = vec![
        DependencyHealth::healthy("database", 5.0),
        DependencyHealth::healthy("redis", 2.0),
        DependencyHealth::healthy("kafka", 1.0),
    ];
    let health = AggregatedHealth::new("1.0.0".to_string(), 3600, dependencies);
    
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.version, "1.0.0");
    assert_eq!(health.uptime_seconds, 3600);
    assert_eq!(health.dependencies.len(), 3);
}

#[test]
fn test_aggregated_health_one_degraded() {
    let dependencies = vec![
        DependencyHealth::healthy("database", 5.0),
        DependencyHealth::degraded("redis", "Slow"),
        DependencyHealth::healthy("kafka", 1.0),
    ];
    let health = AggregatedHealth::new("1.0.0".to_string(), 3600, dependencies);
    
    assert_eq!(health.status, HealthStatus::Degraded);
}

#[test]
fn test_aggregated_health_one_unhealthy() {
    let dependencies = vec![
        DependencyHealth::healthy("database", 5.0),
        DependencyHealth::degraded("redis", "Slow"),
        DependencyHealth::unhealthy("kafka", "Down"),
    ];
    let health = AggregatedHealth::new("1.0.0".to_string(), 3600, dependencies);
    
    assert_eq!(health.status, HealthStatus::Unhealthy);
}

#[test]
fn test_aggregated_health_empty_dependencies() {
    let health = AggregatedHealth::new("1.0.0".to_string(), 0, vec![]);
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.dependencies.is_empty());
}

#[test]
fn test_health_status_serialization() {
    let healthy = serde_json::to_string(&HealthStatus::Healthy).unwrap();
    assert_eq!(healthy, "\"healthy\"");
    
    let degraded = serde_json::to_string(&HealthStatus::Degraded).unwrap();
    assert_eq!(degraded, "\"degraded\"");
    
    let unhealthy = serde_json::to_string(&HealthStatus::Unhealthy).unwrap();
    assert_eq!(unhealthy, "\"unhealthy\"");
}

#[test]
fn test_dependency_health_serialization() {
    let health = DependencyHealth::healthy("database", 5.5);
    let json = serde_json::to_string(&health).unwrap();
    
    assert!(json.contains("\"name\":\"database\""));
    assert!(json.contains("\"status\":\"healthy\""));
    assert!(json.contains("\"latency_ms\":5.5"));
}

#[test]
fn test_aggregated_health_serialization() {
    let dependencies = vec![
        DependencyHealth::healthy("database", 5.0),
    ];
    let health = AggregatedHealth::new("1.0.0".to_string(), 100, dependencies);
    let json = serde_json::to_string(&health).unwrap();
    
    assert!(json.contains("\"status\":\"healthy\""));
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"uptime_seconds\":100"));
    assert!(json.contains("\"dependencies\""));
}
