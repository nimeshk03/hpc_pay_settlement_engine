use metrics::{counter, gauge, histogram, describe_counter, describe_gauge, describe_histogram, Unit};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use std::time::Instant;

static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Global metrics instance.
pub static METRICS: OnceLock<Metrics> = OnceLock::new();

/// Metrics collector for the settlement engine.
#[derive(Debug, Clone)]
pub struct Metrics {
    initialized: bool,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self { initialized: true }
    }

    pub fn record_transaction_created(&self, transaction_type: &str, currency: &str) {
        counter!("settlement_transactions_total", "type" => transaction_type.to_string(), "currency" => currency.to_string()).increment(1);
    }

    pub fn record_transaction_settled(&self, transaction_type: &str, currency: &str) {
        counter!("settlement_transactions_settled_total", "type" => transaction_type.to_string(), "currency" => currency.to_string()).increment(1);
    }

    pub fn record_transaction_failed(&self, transaction_type: &str, reason: &str) {
        counter!("settlement_transactions_failed_total", "type" => transaction_type.to_string(), "reason" => reason.to_string()).increment(1);
    }

    pub fn record_transaction_reversed(&self, transaction_type: &str) {
        counter!("settlement_transactions_reversed_total", "type" => transaction_type.to_string()).increment(1);
    }

    pub fn record_ledger_write_latency(&self, duration_ms: f64) {
        histogram!("settlement_ledger_write_duration_ms").record(duration_ms);
    }

    pub fn record_balance_query_latency(&self, duration_ms: f64, cache_hit: bool) {
        histogram!("settlement_balance_query_duration_ms", "cache_hit" => cache_hit.to_string()).record(duration_ms);
    }

    pub fn record_batch_created(&self, currency: &str) {
        counter!("settlement_batches_created_total", "currency" => currency.to_string()).increment(1);
    }

    pub fn record_batch_processed(&self, currency: &str, transaction_count: u64) {
        counter!("settlement_batches_processed_total", "currency" => currency.to_string()).increment(1);
        histogram!("settlement_batch_transaction_count").record(transaction_count as f64);
    }

    pub fn record_batch_failed(&self, currency: &str, reason: &str) {
        counter!("settlement_batches_failed_total", "currency" => currency.to_string(), "reason" => reason.to_string()).increment(1);
    }

    pub fn record_batch_processing_latency(&self, duration_ms: f64) {
        histogram!("settlement_batch_processing_duration_ms").record(duration_ms);
    }

    pub fn record_netting_calculation(&self, participant_count: u64, position_count: u64) {
        histogram!("settlement_netting_participant_count").record(participant_count as f64);
        histogram!("settlement_netting_position_count").record(position_count as f64);
    }

    pub fn record_netting_efficiency(&self, gross_amount: f64, net_amount: f64) {
        if gross_amount > 0.0 {
            let efficiency = 1.0 - (net_amount / gross_amount);
            histogram!("settlement_netting_efficiency_ratio").record(efficiency);
        }
    }

    pub fn record_netting_latency(&self, duration_ms: f64) {
        histogram!("settlement_netting_calculation_duration_ms").record(duration_ms);
    }

    pub fn set_active_batches(&self, count: i64) {
        gauge!("settlement_active_batches").set(count as f64);
    }

    pub fn set_pending_transactions(&self, count: i64) {
        gauge!("settlement_pending_transactions").set(count as f64);
    }

    pub fn record_http_request(&self, method: &str, path: &str, status: u16, duration_ms: f64) {
        counter!("http_requests_total", "method" => method.to_string(), "path" => path.to_string(), "status" => status.to_string()).increment(1);
        histogram!("http_request_duration_ms", "method" => method.to_string(), "path" => path.to_string()).record(duration_ms);
    }

    pub fn record_db_query(&self, query_type: &str, duration_ms: f64, success: bool) {
        counter!("db_queries_total", "type" => query_type.to_string(), "success" => success.to_string()).increment(1);
        histogram!("db_query_duration_ms", "type" => query_type.to_string()).record(duration_ms);
    }

    pub fn record_redis_operation(&self, operation: &str, duration_ms: f64, success: bool) {
        counter!("redis_operations_total", "operation" => operation.to_string(), "success" => success.to_string()).increment(1);
        histogram!("redis_operation_duration_ms", "operation" => operation.to_string()).record(duration_ms);
    }

    pub fn record_kafka_message(&self, topic: &str, success: bool) {
        counter!("kafka_messages_total", "topic" => topic.to_string(), "success" => success.to_string()).increment(1);
    }
}

/// Timer for measuring operation latency.
pub struct LatencyTimer {
    start: Instant,
}

impl LatencyTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

impl Default for LatencyTimer {
    fn default() -> Self {
        Self::new()
    }
}

/// Initializes the metrics system and returns the Prometheus handle.
pub fn init_metrics() -> PrometheusHandle {
    let handle = METRICS_HANDLE.get_or_init(|| {
        let builder = PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .expect("Failed to install Prometheus recorder");

        describe_metrics();
        handle
    });

    METRICS.get_or_init(Metrics::new);

    handle.clone()
}

/// Describes all metrics for Prometheus.
fn describe_metrics() {
    describe_counter!("settlement_transactions_total", Unit::Count, "Total number of transactions created");
    describe_counter!("settlement_transactions_settled_total", Unit::Count, "Total number of transactions settled");
    describe_counter!("settlement_transactions_failed_total", Unit::Count, "Total number of failed transactions");
    describe_counter!("settlement_transactions_reversed_total", Unit::Count, "Total number of reversed transactions");
    
    describe_histogram!("settlement_ledger_write_duration_ms", Unit::Milliseconds, "Ledger write latency in milliseconds");
    describe_histogram!("settlement_balance_query_duration_ms", Unit::Milliseconds, "Balance query latency in milliseconds");
    
    describe_counter!("settlement_batches_created_total", Unit::Count, "Total number of batches created");
    describe_counter!("settlement_batches_processed_total", Unit::Count, "Total number of batches processed");
    describe_counter!("settlement_batches_failed_total", Unit::Count, "Total number of failed batches");
    describe_histogram!("settlement_batch_processing_duration_ms", Unit::Milliseconds, "Batch processing latency in milliseconds");
    describe_histogram!("settlement_batch_transaction_count", Unit::Count, "Number of transactions per batch");
    
    describe_histogram!("settlement_netting_participant_count", Unit::Count, "Number of participants in netting calculation");
    describe_histogram!("settlement_netting_position_count", Unit::Count, "Number of positions in netting calculation");
    describe_histogram!("settlement_netting_efficiency_ratio", Unit::Count, "Netting efficiency ratio (1 - net/gross)");
    describe_histogram!("settlement_netting_calculation_duration_ms", Unit::Milliseconds, "Netting calculation latency in milliseconds");
    
    describe_gauge!("settlement_active_batches", Unit::Count, "Number of active batches");
    describe_gauge!("settlement_pending_transactions", Unit::Count, "Number of pending transactions");
    
    describe_counter!("http_requests_total", Unit::Count, "Total HTTP requests");
    describe_histogram!("http_request_duration_ms", Unit::Milliseconds, "HTTP request latency in milliseconds");
    
    describe_counter!("db_queries_total", Unit::Count, "Total database queries");
    describe_histogram!("db_query_duration_ms", Unit::Milliseconds, "Database query latency in milliseconds");
    
    describe_counter!("redis_operations_total", Unit::Count, "Total Redis operations");
    describe_histogram!("redis_operation_duration_ms", Unit::Milliseconds, "Redis operation latency in milliseconds");
    
    describe_counter!("kafka_messages_total", Unit::Count, "Total Kafka messages");
}

/// Returns the global metrics instance.
pub fn get_metrics() -> &'static Metrics {
    METRICS.get_or_init(Metrics::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_timer() {
        let timer = LatencyTimer::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();
        assert!(elapsed >= 10.0);
    }

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        assert!(metrics.initialized);
    }
}
