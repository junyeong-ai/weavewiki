//! Unified Timeout Configuration
//!
//! Centralized timeout management with operation-specific defaults.

use std::future::Future;
use std::time::Duration;

use crate::constants::network as net_constants;
use crate::types::{Result, WeaveError};

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
        Self {
            llm_request: Duration::from_secs(net_constants::DEFAULT_TIMEOUT_SECS),
            file_io: Duration::from_secs(30),
            database: Duration::from_secs(30),
            connection: Duration::from_secs(net_constants::CONNECTION_TIMEOUT_SECS),
            analysis_phase: Duration::from_secs(600),
        }
    }
}

pub async fn with_timeout<T, F>(timeout: Duration, future: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(WeaveError::timeout(operation_name, timeout)),
    }
}

pub async fn with_timeout_map<T, F>(timeout: Duration, future: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => Ok(result),
        Err(_) => Err(WeaveError::timeout(operation_name, timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_config_defaults() {
        let config = TimeoutConfig::default();
        assert_eq!(config.llm_request.as_secs(), 300);
        assert_eq!(config.connection.as_secs(), 30);
        assert_eq!(config.analysis_phase.as_secs(), 600);
    }

    #[tokio::test]
    async fn test_with_timeout_success() {
        let result = with_timeout(
            Duration::from_secs(1),
            async { Ok::<_, WeaveError>(42) },
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
                Ok::<_, WeaveError>(42)
            },
            "slow operation",
        )
        .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WeaveError::Timeout { .. }));
    }
}
