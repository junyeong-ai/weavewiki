//! LLM Response Parsing Types
//!
//! Core types for the unified response parsing system that handles
//! schema-first validation and partial parsing with graceful degradation.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Result of parsing an LLM response with field-level granularity.
///
/// Unlike traditional parsing that fails entirely on any error,
/// this captures partial success and allows recovery.
#[derive(Debug, Clone)]
pub struct ParseResult<T> {
    /// Parsed data (may be partial with defaults for failed fields)
    pub data: T,
    /// Overall parsing status
    pub status: ParseStatus,
    /// Fields that failed to parse
    pub failed_fields: Vec<FieldError>,
    /// Number of fields successfully recovered
    pub recovered_count: usize,
}

impl<T> ParseResult<T> {
    /// Create a complete parse result (all fields parsed successfully)
    pub fn complete(data: T) -> Self {
        Self {
            data,
            status: ParseStatus::Complete,
            failed_fields: Vec::new(),
            recovered_count: 0,
        }
    }

    /// Create a partial parse result with some fields using defaults
    pub fn partial(data: T, failed_fields: Vec<FieldError>, recovered_count: usize) -> Self {
        let success_ratio = if failed_fields.is_empty() && recovered_count == 0 {
            1.0
        } else {
            recovered_count as f32 / (recovered_count + failed_fields.len()).max(1) as f32
        };

        Self {
            data,
            status: ParseStatus::Partial { success_ratio },
            failed_fields,
            recovered_count,
        }
    }

    /// Create a failed parse result
    pub fn failed(data: T, reason: impl Into<String>) -> Self {
        Self {
            data,
            status: ParseStatus::Failed {
                reason: reason.into(),
            },
            failed_fields: Vec::new(),
            recovered_count: 0,
        }
    }

    /// Check if parsing was complete (no errors)
    pub fn is_complete(&self) -> bool {
        matches!(self.status, ParseStatus::Complete)
    }

    /// Check if parsing succeeded (complete or partial with usable data)
    pub fn is_usable(&self) -> bool {
        match &self.status {
            ParseStatus::Complete => true,
            ParseStatus::Partial { success_ratio } => *success_ratio > 0.3,
            ParseStatus::Failed { .. } => false,
        }
    }

    /// Get the success ratio (1.0 for complete, 0.0 for failed)
    pub fn success_ratio(&self) -> f32 {
        self.status.success_ratio()
    }

    /// Log parsing status for diagnostics
    pub fn log_status(&self, context: &str) {
        match &self.status {
            ParseStatus::Complete => {
                tracing::debug!(context = %context, "Parse complete");
            }
            ParseStatus::Partial { success_ratio } => {
                tracing::warn!(
                    context = %context,
                    success_ratio = %success_ratio,
                    failed_fields = ?self.failed_fields.iter().map(|f| &f.field).collect::<Vec<_>>(),
                    recovered = %self.recovered_count,
                    "Partial parse with recovery"
                );
            }
            ParseStatus::Failed { reason } => {
                tracing::error!(
                    context = %context,
                    reason = %reason,
                    "Parse failed"
                );
            }
        }
    }
}

/// Status of the parsing operation
#[derive(Debug, Clone, PartialEq)]
pub enum ParseStatus {
    /// All fields parsed successfully
    Complete,
    /// Some fields used defaults due to parse errors
    Partial { success_ratio: f32 },
    /// Critical failure - data may not be usable
    Failed { reason: String },
}

impl ParseStatus {
    /// Get the success ratio for this status
    pub fn success_ratio(&self) -> f32 {
        match self {
            Self::Complete => 1.0,
            Self::Partial { success_ratio } => *success_ratio,
            Self::Failed { .. } => 0.0,
        }
    }
}

impl fmt::Display for ParseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete => write!(f, "complete"),
            Self::Partial { success_ratio } => {
                write!(f, "partial ({:.0}%)", success_ratio * 100.0)
            }
            Self::Failed { reason } => write!(f, "failed: {}", reason),
        }
    }
}

/// Error information for a single field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    /// Field name that failed to parse
    pub field: String,
    /// Expected type for the field
    pub expected_type: String,
    /// Actual value received (truncated for logging)
    pub actual_value: Option<String>,
    /// Action taken to recover
    pub recovery_action: RecoveryAction,
}

impl FieldError {
    pub fn new(
        field: impl Into<String>,
        expected_type: impl Into<String>,
        actual_value: Option<String>,
        recovery_action: RecoveryAction,
    ) -> Self {
        Self {
            field: field.into(),
            expected_type: expected_type.into(),
            actual_value: actual_value.map(|v| truncate_value(&v, 100)),
            recovery_action,
        }
    }
}

/// Action taken to recover from a field parse error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Used the type's default value
    UsedDefault,
    /// Skipped the field entirely
    Skipped,
    /// Converted from a different type
    Converted,
}

impl fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsedDefault => write!(f, "used_default"),
            Self::Skipped => write!(f, "skipped"),
            Self::Converted => write!(f, "converted"),
        }
    }
}

/// Truncate a value string for logging
fn truncate_value(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_result_complete() {
        let result = ParseResult::complete("test data");
        assert!(result.is_complete());
        assert!(result.is_usable());
        assert_eq!(result.success_ratio(), 1.0);
    }

    #[test]
    fn test_parse_result_partial() {
        let result = ParseResult::partial(
            "partial data",
            vec![FieldError::new(
                "field1",
                "String",
                None,
                RecoveryAction::UsedDefault,
            )],
            3,
        );
        assert!(!result.is_complete());
        assert!(result.is_usable());
        assert!(result.success_ratio() > 0.0 && result.success_ratio() < 1.0);
    }

    #[test]
    fn test_parse_result_failed() {
        let result = ParseResult::failed("failed data", "JSON syntax error");
        assert!(!result.is_complete());
        assert!(!result.is_usable());
        assert_eq!(result.success_ratio(), 0.0);
    }

    #[test]
    fn test_truncate_value() {
        assert_eq!(truncate_value("short", 100), "short");
        assert_eq!(
            truncate_value("a".repeat(200).as_str(), 10),
            "aaaaaaaaaa..."
        );
    }
}
