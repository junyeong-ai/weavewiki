//! Pure Pattern Matching Utilities
//!
//! Pattern-matching functions that don't depend on any pipeline context.
//! These can be safely used by both types and pipeline modules.

use std::sync::LazyLock;

use regex::Regex;

/// File reference pattern for @path/to/file:line format
/// Matches: @path/to/file.ext, @path/to/file.ext:42, @path/to/file.ext:42-50
/// Requires `@` to be preceded by whitespace/punctuation (not alphanumeric) to avoid email matching
pub static FILE_REFERENCE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^a-zA-Z0-9])@([a-zA-Z0-9_][a-zA-Z0-9_./\-]*)(?::(\d+)(?:-(\d+))?)?")
        .expect("Invalid regex")
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
    Regex::new(r"(?i)(best practice|industry standard|common pattern|typically|generally|usually)")
        .expect("Invalid regex")
});

/// Tier 3 high-value constraint indicators
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

/// Generic commands that indicate Tier 1 content
pub const GENERIC_COMMANDS: &[&str] = &[
    "cargo build",
    "cargo test",
    "npm install",
    "npm run",
    "pip install",
    "go build",
    "docker run",
    "git commit",
];

/// Parsed file reference
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub path: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
}

impl FileRef {
    pub fn has_line_info(&self) -> bool {
        self.line_start.is_some()
    }
}

/// Extract file references from content
pub fn extract_file_refs(content: &str) -> Vec<FileRef> {
    FILE_REFERENCE_PATTERN
        .captures_iter(content)
        .filter_map(|cap| {
            let path = cap.get(1)?.as_str().to_string();

            // Skip non-file references
            if path.is_empty()
                || path.starts_with("http")
                || path.starts_with("CLAUDE")
                || path.contains('@')
            {
                return None;
            }

            let line_start = cap.get(2).and_then(|m| m.as_str().parse().ok());
            let line_end = cap.get(3).and_then(|m| m.as_str().parse().ok());

            Some(FileRef {
                path,
                line_start,
                line_end,
            })
        })
        .collect()
}

/// Count file references with line numbers
pub fn count_file_line_refs(content: &str) -> usize {
    extract_file_refs(content)
        .into_iter()
        .filter(|r| r.has_line_info())
        .count()
}

/// Count all file references
pub fn count_file_refs(content: &str) -> usize {
    extract_file_refs(content).len()
}

/// Count Tier 3 indicators in content
pub fn count_tier3_indicators(content: &str) -> usize {
    let lower = content.to_lowercase();
    TIER3_INDICATORS
        .iter()
        .filter(|i| lower.contains(*i))
        .count()
}

/// Count value indicators in content
pub fn count_value_indicators(content: &str) -> usize {
    let lower = content.to_lowercase();
    VALUE_INDICATORS
        .iter()
        .filter(|i| lower.contains(*i))
        .count()
}

/// Count generic patterns (Tier 1 indicators) in content
pub fn count_generic_patterns(content: &str) -> usize {
    let lower = content.to_lowercase();
    GENERIC_COMMANDS
        .iter()
        .filter(|c| lower.contains(*c))
        .count()
}

/// Extract file paths only (no line numbers)
pub fn extract_paths(content: &str) -> Vec<String> {
    extract_file_refs(content)
        .into_iter()
        .map(|r| r.path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_line_refs() {
        assert!(count_file_line_refs("See @src/main.rs:42") > 0);
        assert_eq!(count_file_refs("email@example.com"), 0);
        assert!(count_file_refs("Check @src/lib.rs") > 0);
    }

    #[test]
    fn test_actionable_pattern() {
        assert!(ACTIONABLE_PATTERN.is_match("You must use Result"));
        assert!(ACTIONABLE_PATTERN.is_match("Avoid using println!"));
    }

    #[test]
    fn test_tier3_indicators() {
        assert!(count_tier3_indicators("Watch for race condition") > 0);
        assert!(count_tier3_indicators("This is normal") == 0);
    }

    #[test]
    fn test_generic_patterns() {
        assert!(count_generic_patterns("Run cargo build") > 0);
        assert!(count_generic_patterns("Custom build script") == 0);
    }
}
