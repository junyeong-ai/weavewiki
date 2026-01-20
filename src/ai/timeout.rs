//! Unified Timeout Configuration
//!
//! Centralized timeout management with operation-specific defaults.

use std::future::Future;
use std::time::Duration;

use crate::config::NetworkConfig;
use crate::types::{ClaudegenError, Result};

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub llm_request: Duration,
    pub file_io: Duration,
    pub database: Duration,
    pub connection: Duration,
    pub analysis_phase: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        let network = NetworkConfig::default();
        Self {
            llm_request: Duration::from_millis(network.timeout_ms),
            file_io: Duration::from_secs(30), // Default 30s for file I/O
            database: Duration::from_secs(60), // Default 60s for database
            connection: Duration::from_millis(network.connect_timeout_ms),
            analysis_phase: Duration::from_secs(network.analysis_phase_timeout_secs),
        }
    }
}

impl TimeoutConfig {
    pub fn from_network_config(network: &NetworkConfig) -> Self {
        Self {
            llm_request: Duration::from_millis(network.timeout_ms),
            file_io: Duration::from_secs(30), // Default 30s for file I/O
            database: Duration::from_secs(60), // Default 60s for database
            connection: Duration::from_millis(network.connect_timeout_ms),
            analysis_phase: Duration::from_secs(network.analysis_phase_timeout_secs),
        }
    }
}

pub async fn with_timeout<T, F>(timeout: Duration, future: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(ClaudegenError::timeout(operation_name, timeout)),
    }
}

pub async fn with_timeout_map<T, F>(timeout: Duration, future: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => Ok(result),
        Err(_) => Err(ClaudegenError::timeout(operation_name, timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_config_defaults() {
        let config = TimeoutConfig::default();
        // NetworkConfig defaults: timeout_ms=300_000, connect_timeout_ms=30_000, analysis_phase_timeout_secs=600
        assert_eq!(config.llm_request.as_millis(), 300_000);
        assert_eq!(config.connection.as_millis(), 30_000);
        assert_eq!(config.analysis_phase.as_secs(), 600);
    }

    #[tokio::test]
    async fn test_with_timeout_success() {
        let result = with_timeout(
            Duration::from_secs(1),
            async { Ok::<_, ClaudegenError>(42) },
            "test operation",
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_timeout_expires() {
        let result = with_timeout(
            Duration::from_millis(10),
            async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok::<_, ClaudegenError>(42)
            },
            "slow operation",
        )
        .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ClaudegenError::Timeout { .. }
        ));
    }
}
