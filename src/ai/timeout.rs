//! Unified Timeout Configuration
//!
//! Provides a centralized timeout management system with:
//! - Operation-specific timeout defaults
//! - Helper function for wrapping async operations
//! - Consistent timeout error handling
//!
//! ## Usage
//!
//! ```ignore
//! use crate::ai::timeout::{TimeoutConfig, with_timeout};
//!
//! let config = TimeoutConfig::default();
//! let result = with_timeout(
//!     config.llm_request,
//!     async { /* LLM call */ },
//!     "LLM request"
//! ).await?;
//! ```

use std::future::Future;
use std::time::Duration;

use crate::constants::network as net_constants;
use crate::types::{Result, WeaveError};

/// Unified timeout configuration for all operations
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Timeout for LLM API requests (default: 5 minutes)
    pub llm_request: Duration,
    /// Timeout for file I/O operations (default: 30 seconds)
    pub file_io: Duration,
    /// Timeout for database operations (default: 30 seconds)
    pub database: Duration,
    /// Timeout for network connections (default: 30 seconds)
    pub connection: Duration,
    /// Timeout for analysis phase operations (default: 10 minutes)
    pub analysis_phase: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            llm_request: Duration::from_secs(net_constants::DEFAULT_TIMEOUT_SECS),
            file_io: Duration::from_secs(30),
            database: Duration::from_secs(30),
            connection: Duration::from_secs(net_constants::CONNECTION_TIMEOUT_SECS),
            analysis_phase: Duration::from_secs(600), // 10 minutes
        }
    }
}

impl TimeoutConfig {
    /// Create a config with shorter timeouts for fast operations
    pub fn fast() -> Self {
        Self {
            llm_request: Duration::from_secs(120),
            file_io: Duration::from_secs(10),
            database: Duration::from_secs(10),
            connection: Duration::from_secs(10),
            analysis_phase: Duration::from_secs(300),
        }
    }

    /// Create a config with longer timeouts for complex operations
    pub fn extended() -> Self {
        Self {
            llm_request: Duration::from_secs(600), // 10 minutes
            file_io: Duration::from_secs(60),
            database: Duration::from_secs(60),
            connection: Duration::from_secs(60),
            analysis_phase: Duration::from_secs(1800), // 30 minutes
        }
    }
}

/// Execute an async operation with a timeout
///
/// Returns a timeout error if the operation doesn't complete within the specified duration.
///
/// # Arguments
///
/// * `timeout` - Maximum duration to wait
/// * `future` - The async operation to execute
/// * `operation_name` - Description of the operation (for error messages)
///
/// # Example
///
/// ```ignore
/// let result = with_timeout(
///     Duration::from_secs(30),
///     async { expensive_operation().await },
///     "expensive operation"
/// ).await?;
/// ```
pub async fn with_timeout<T, F>(timeout: Duration, future: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(WeaveError::timeout(operation_name, timeout)),
    }
}

/// Execute an async operation with a timeout, mapping the inner result
///
/// This variant accepts futures that return non-Result types and wraps them.
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

    #[test]
    fn test_timeout_config_fast() {
        let config = TimeoutConfig::fast();
        assert!(config.llm_request < TimeoutConfig::default().llm_request);
        assert!(config.analysis_phase < TimeoutConfig::default().analysis_phase);
    }

    #[test]
    fn test_timeout_config_extended() {
        let config = TimeoutConfig::extended();
        assert!(config.llm_request > TimeoutConfig::default().llm_request);
        assert!(config.analysis_phase > TimeoutConfig::default().analysis_phase);
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
