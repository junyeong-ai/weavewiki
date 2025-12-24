pub mod claim;
pub mod convention;
pub mod domain;
pub mod edge;
pub mod error;
pub mod node;
pub mod project;
pub mod utils;

pub use claim::*;
pub use convention::*;
pub use domain::DomainTerm;
pub use edge::*;
pub use error::{
    ErrorCategory, ErrorClassifier, LlmError, Result, ResultExt, ValidationError,
    ValidationErrorKind, WeaveError,
};
pub use node::*;
pub use project::*;
pub use utils::{
    ParseWithDefault, TokenEstimator, enum_to_str, estimate_code_tokens, estimate_tokens,
    json_bool, json_f64, json_i64, json_string, json_string_array, json_string_or,
    log_filter_error, log_filter_warn, truncate_to_token_limit,
};

// =============================================================================
// Domain Newtypes
// =============================================================================

use std::fmt;

/// Type-safe wrapper for session IDs
///
/// Prevents accidental mixing of session IDs with other string types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Type-safe wrapper for token counts
///
/// Provides compile-time type safety for token budget operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TokenCount(u64);

impl TokenCount {
    pub const ZERO: Self = Self(0);

    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Check if count exceeds a threshold percentage of a budget
    pub fn exceeds_threshold(self, budget: Self, threshold: f64) -> bool {
        if budget.0 == 0 {
            return false;
        }
        (self.0 as f64 / budget.0 as f64) >= threshold
    }

    /// Calculate utilization as a percentage (0.0 - 1.0)
    pub fn utilization(self, budget: Self) -> f64 {
        if budget.0 == 0 {
            0.0
        } else {
            self.0 as f64 / budget.0 as f64
        }
    }
}

impl fmt::Display for TokenCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for TokenCount {
    fn from(count: u64) -> Self {
        Self(count)
    }
}

impl From<u32> for TokenCount {
    fn from(count: u32) -> Self {
        Self(count as u64)
    }
}

impl From<usize> for TokenCount {
    fn from(count: usize) -> Self {
        Self(count as u64)
    }
}

impl std::ops::Add for TokenCount {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for TokenCount {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::Sub for TokenCount {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

/// Type-safe wrapper for file paths
///
/// Ensures file paths are not accidentally mixed with other string types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FilePath(String);

impl FilePath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    /// Extract the file name from the path
    pub fn file_name(&self) -> Option<&str> {
        std::path::Path::new(&self.0)
            .file_name()
            .and_then(|n| n.to_str())
    }

    /// Extract the extension from the path
    pub fn extension(&self) -> Option<&str> {
        std::path::Path::new(&self.0)
            .extension()
            .and_then(|e| e.to_str())
    }

    /// Get the parent directory path
    pub fn parent(&self) -> Option<&str> {
        std::path::Path::new(&self.0)
            .parent()
            .and_then(|p| p.to_str())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for FilePath {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for FilePath {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for FilePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::path::Path> for FilePath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

/// Type-safe wrapper for graph node IDs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn file(path: &str) -> Self {
        Self(format!("file:{}", path))
    }

    pub fn module(name: &str) -> Self {
        Self(format!("module:{}", name))
    }

    pub fn class(path: &str, name: &str) -> Self {
        Self(format!("class:{}:{}", path, name))
    }

    pub fn function(path: &str, name: &str) -> Self {
        Self(format!("function:{}:{}", path, name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Type-safe wrapper for graph edge IDs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EdgeId(String);

impl EdgeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn dependency(path: &str, target: &str) -> Self {
        Self(format!("dep:{}:{}", path, target))
    }

    pub fn owns(path: &str, element: &str) -> Self {
        Self(format!("owns:{}:{}", path, element))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for EdgeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for EdgeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for EdgeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod newtype_tests {
    use super::*;

    #[test]
    fn test_token_count_arithmetic() {
        let a = TokenCount::new(100);
        let b = TokenCount::new(50);

        assert_eq!((a + b).get(), 150);
        assert_eq!((a - b).get(), 50);
        assert_eq!(a.saturating_sub(TokenCount::new(200)).get(), 0);
    }

    #[test]
    fn test_token_count_threshold() {
        let consumed = TokenCount::new(750);
        let budget = TokenCount::new(1000);

        assert!(consumed.exceeds_threshold(budget, 0.7));
        assert!(!consumed.exceeds_threshold(budget, 0.8));
    }

    #[test]
    fn test_token_count_utilization() {
        let consumed = TokenCount::new(250);
        let budget = TokenCount::new(1000);

        assert!((consumed.utilization(budget) - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_file_path_methods() {
        let path = FilePath::new("src/main.rs");

        assert_eq!(path.file_name(), Some("main.rs"));
        assert_eq!(path.extension(), Some("rs"));
        assert_eq!(path.parent(), Some("src"));
    }

    #[test]
    fn test_node_id_constructors() {
        assert_eq!(NodeId::file("main.rs").as_str(), "file:main.rs");
        assert_eq!(
            NodeId::class("main.rs", "Foo").as_str(),
            "class:main.rs:Foo"
        );
        assert_eq!(
            NodeId::function("main.rs", "run").as_str(),
            "function:main.rs:run"
        );
    }

    #[test]
    fn test_edge_id_constructors() {
        assert_eq!(
            EdgeId::dependency("main.rs", "lib.rs").as_str(),
            "dep:main.rs:lib.rs"
        );
        assert_eq!(EdgeId::owns("main.rs", "Foo").as_str(), "owns:main.rs:Foo");
    }

    #[test]
    fn test_session_id() {
        let id = SessionId::new("sess-123");
        assert_eq!(id.as_str(), "sess-123");
        assert_eq!(format!("{}", id), "sess-123");
    }
}
