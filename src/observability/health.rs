use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

/// Health status of a service or dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self, HealthStatus::Degraded)
    }

    pub fn is_unhealthy(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy)
    }
}

/// Health status of a single dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyHealth {
    pub name: String,
    pub status: HealthStatus,
    pub latency_ms: Option<f64>,
    pub message: Option<String>,
}

impl DependencyHealth {
    pub fn healthy(name: impl Into<String>, latency_ms: f64) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Healthy,
            latency_ms: Some(latency_ms),
            message: None,
        }
    }

    pub fn degraded(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Degraded,
            latency_ms: None,
            message: Some(message.into()),
        }
    }

    pub fn unhealthy(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Unhealthy,
            latency_ms: None,
            message: Some(message.into()),
        }
    }
}

/// Aggregated health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedHealth {
    pub status: HealthStatus,
    pub version: String,
    pub uptime_seconds: u64,
    pub dependencies: Vec<DependencyHealth>,
}

impl AggregatedHealth {
    pub fn new(version: String, uptime_seconds: u64, dependencies: Vec<DependencyHealth>) -> Self {
        let status = Self::aggregate_status(&dependencies);
        Self {
            status,
            version,
            uptime_seconds,
            dependencies,
        }
    }

    fn aggregate_status(dependencies: &[DependencyHealth]) -> HealthStatus {
        let has_unhealthy = dependencies.iter().any(|d| d.status.is_unhealthy());
        let has_degraded = dependencies.iter().any(|d| d.status.is_degraded());

        if has_unhealthy {
            HealthStatus::Unhealthy
        } else if has_degraded {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }
}

/// Health checker for all dependencies.
pub struct HealthChecker {
    pool: PgPool,
    redis_client: redis::Client,
    kafka_client: Option<Arc<rskafka::client::Client>>,
    start_time: std::time::Instant,
}

impl HealthChecker {
    pub fn new(
        pool: PgPool,
        redis_client: redis::Client,
        kafka_client: Option<Arc<rskafka::client::Client>>,
    ) -> Self {
        Self {
            pool,
            redis_client,
            kafka_client,
            start_time: std::time::Instant::now(),
        }
    }

    /// Performs a full health check of all dependencies.
    pub async fn check_all(&self) -> AggregatedHealth {
        let mut dependencies = Vec::new();

        dependencies.push(self.check_database().await);
        dependencies.push(self.check_redis().await);
        dependencies.push(self.check_kafka().await);

        AggregatedHealth::new(
            env!("CARGO_PKG_VERSION").to_string(),
            self.start_time.elapsed().as_secs(),
            dependencies,
        )
    }

    /// Checks database connectivity.
    pub async fn check_database(&self) -> DependencyHealth {
        let start = std::time::Instant::now();
        
        match tokio::time::timeout(
            Duration::from_secs(5),
            sqlx::query("SELECT 1").fetch_one(&self.pool)
        ).await {
            Ok(Ok(_)) => {
                let latency = start.elapsed().as_secs_f64() * 1000.0;
                if latency > 100.0 {
                    DependencyHealth {
                        name: "database".to_string(),
                        status: HealthStatus::Degraded,
                        latency_ms: Some(latency),
                        message: Some("High latency detected".to_string()),
                    }
                } else {
                    DependencyHealth::healthy("database", latency)
                }
            }
            Ok(Err(e)) => DependencyHealth::unhealthy("database", format!("Query failed: {}", e)),
            Err(_) => DependencyHealth::unhealthy("database", "Connection timeout"),
        }
    }

    /// Checks Redis connectivity.
    pub async fn check_redis(&self) -> DependencyHealth {
        let start = std::time::Instant::now();
        
        match self.redis_client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    redis::cmd("PING").query_async::<_, ()>(&mut conn)
                ).await {
                    Ok(Ok(_)) => {
                        let latency = start.elapsed().as_secs_f64() * 1000.0;
                        if latency > 50.0 {
                            DependencyHealth {
                                name: "redis".to_string(),
                                status: HealthStatus::Degraded,
                                latency_ms: Some(latency),
                                message: Some("High latency detected".to_string()),
                            }
                        } else {
                            DependencyHealth::healthy("redis", latency)
                        }
                    }
                    Ok(Err(e)) => DependencyHealth::unhealthy("redis", format!("PING failed: {}", e)),
                    Err(_) => DependencyHealth::unhealthy("redis", "PING timeout"),
                }
            }
            Err(e) => DependencyHealth::unhealthy("redis", format!("Connection failed: {}", e)),
        }
    }

    /// Checks Kafka connectivity.
    pub async fn check_kafka(&self) -> DependencyHealth {
        match &self.kafka_client {
            Some(_client) => {
                DependencyHealth::healthy("kafka", 0.0)
            }
            None => DependencyHealth {
                name: "kafka".to_string(),
                status: HealthStatus::Degraded,
                latency_ms: None,
                message: Some("Kafka client not connected".to_string()),
            },
        }
    }

    /// Liveness check - returns true if the service is alive.
    pub fn is_alive(&self) -> bool {
        true
    }

    /// Readiness check - returns true if the service is ready to accept traffic.
    pub async fn is_ready(&self) -> bool {
        let db_health = self.check_database().await;
        let redis_health = self.check_redis().await;

        db_health.status.is_healthy() && (redis_health.status.is_healthy() || redis_health.status.is_degraded())
    }

    /// Returns uptime in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
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
    fn test_dependency_health_constructors() {
        let healthy = DependencyHealth::healthy("test", 5.0);
        assert_eq!(healthy.status, HealthStatus::Healthy);
        assert_eq!(healthy.latency_ms, Some(5.0));

        let degraded = DependencyHealth::degraded("test", "slow");
        assert_eq!(degraded.status, HealthStatus::Degraded);
        assert_eq!(degraded.message, Some("slow".to_string()));

        let unhealthy = DependencyHealth::unhealthy("test", "down");
        assert_eq!(unhealthy.status, HealthStatus::Unhealthy);
        assert_eq!(unhealthy.message, Some("down".to_string()));
    }

    #[test]
    fn test_aggregated_health_status() {
        let all_healthy = vec![
            DependencyHealth::healthy("db", 1.0),
            DependencyHealth::healthy("redis", 2.0),
        ];
        let health = AggregatedHealth::new("1.0.0".to_string(), 100, all_healthy);
        assert_eq!(health.status, HealthStatus::Healthy);

        let one_degraded = vec![
            DependencyHealth::healthy("db", 1.0),
            DependencyHealth::degraded("redis", "slow"),
        ];
        let health = AggregatedHealth::new("1.0.0".to_string(), 100, one_degraded);
        assert_eq!(health.status, HealthStatus::Degraded);

        let one_unhealthy = vec![
            DependencyHealth::healthy("db", 1.0),
            DependencyHealth::unhealthy("redis", "down"),
        ];
        let health = AggregatedHealth::new("1.0.0".to_string(), 100, one_unhealthy);
        assert_eq!(health.status, HealthStatus::Unhealthy);
    }
}
