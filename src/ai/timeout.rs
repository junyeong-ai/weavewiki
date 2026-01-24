//! Timeout Utilities
//!
//! Async timeout wrappers for operations.

use std::future::Future;
use std::time::Duration;

use crate::types::{ClaudegenError, Result};

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
