//! Common Patterns Module
//!
//! Centralized regex patterns for structural validation.
//! Content-based classification is handled by HybridClassifier (LLM + structural rules).

use std::sync::LazyLock;

use regex::Regex;

/// File reference with line number: @src/main.rs:42
pub static FILE_LINE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@([a-zA-Z0-9_\-]+/[a-zA-Z0-9_./\-]+):(\d+)").expect("Invalid regex")
});

/// File reference without line number: @src/main.rs
pub static FILE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@([a-zA-Z0-9_\-]+/[a-zA-Z0-9_./\-]+)").expect("Invalid regex")
});

/// Actionable language patterns
pub static ACTIONABLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(",
        "must|shall|should|always|never|avoid|",
        "use|prefer|ensure|require|need|",
        "do not|don't|cannot|can't|forbidden|prohibited|",
        "check|verify|validate|test|run|execute|call|invoke|",
        "create|add|implement|build|define|configure|",
        "remove|delete|clean|clear|reset|",
        "read|write|update|modify|change|edit",
        r")\b"
    ))
    .expect("Invalid regex")
});

/// Generic language patterns
pub static GENERIC_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(best practice|industry standard|common pattern|typically|generally|usually)",
    )
    .expect("Invalid regex")
});

/// Code example block pattern
pub static CODE_EXAMPLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```\w*\n[\s\S]*?```").expect("Invalid regex"));

/// Tier 3 high-value constraint indicators (structural detection)
pub const TIER3_INDICATORS: &[&str] = &[
    "race condition",
    "deadlock",
    "memory leak",
    "data loss",
    "security vulnerability",
    "injection",
    "order matters",
    "must not",
    "never",
    "forbidden",
    "breaks",
    "fails when",
];

/// Value indicators for content scoring
pub const VALUE_INDICATORS: &[&str] = &[
    "example",
    "see @",
    "refer to",
    "instead use",
    "when",
    "because",
    "rationale",
];

pub fn count_file_refs(content: &str) -> usize {
    FILE_REF.captures_iter(content).count()
}

pub fn count_file_line_refs(content: &str) -> usize {
    FILE_LINE_REF.captures_iter(content).count()
}

pub fn count_code_examples(content: &str) -> usize {
    CODE_EXAMPLE_PATTERN.find_iter(content).count()
}

pub fn has_actionable_language(content: &str) -> bool {
    ACTIONABLE_PATTERN.is_match(content)
}

pub fn has_generic_language(content: &str) -> bool {
    GENERIC_PATTERN.is_match(content)
}

pub fn count_tier3_indicators(content: &str) -> usize {
    let lower = content.to_lowercase();
    TIER3_INDICATORS.iter().filter(|i| lower.contains(*i)).count()
}

pub fn count_value_indicators(content: &str) -> usize {
    let lower = content.to_lowercase();
    VALUE_INDICATORS.iter().filter(|i| lower.contains(*i)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_line_ref() {
        assert!(FILE_LINE_REF.is_match("See @src/main.rs:42"));
        assert!(!FILE_LINE_REF.is_match("email@example.com"));
    }

    #[test]
    fn test_file_ref() {
        assert!(FILE_REF.is_match("Check @src/lib.rs"));
        assert!(FILE_REF.is_match("@src/main.rs:42"));
    }

    #[test]
    fn test_actionable_pattern() {
        assert!(ACTIONABLE_PATTERN.is_match("You must use Result"));
        assert!(ACTIONABLE_PATTERN.is_match("Avoid using println!"));
        assert!(!ACTIONABLE_PATTERN.is_match("This is a file"));
    }

    #[test]
    fn test_generic_pattern() {
        assert!(GENERIC_PATTERN.is_match("Following best practices"));
        assert!(!GENERIC_PATTERN.is_match("Use Arc::clone()"));
    }

    #[test]
    fn test_count_functions() {
        let content = "See @src/main.rs:10 and @src/lib.rs";
        assert_eq!(count_file_refs(content), 2);
        assert_eq!(count_file_line_refs(content), 1);
    }

    #[test]
    fn test_tier3_indicators() {
        assert_eq!(count_tier3_indicators("race condition detected"), 1);
        assert_eq!(count_tier3_indicators("normal code"), 0);
    }
}
