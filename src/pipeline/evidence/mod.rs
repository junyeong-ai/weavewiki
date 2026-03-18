//! Evidence Reference Formatting - Single Source of Truth
//!
//! Provides consistent formatting for file references throughout the codebase.
//! - Artifact references: @file:line format for generated documentation
//! - Internal references: file:line format for analysis data

/// Format a file reference for artifact output (@file:line format).
///
/// Used in generated rules, skills, agents, and CLAUDE.md.
#[inline]
pub fn artifact_ref(file: &str, line: u32) -> String {
    format!("@{}:{}", file, line)
}

/// Format a file reference for artifact output with optional line number.
///
/// Produces `@file:line` when line is present, or `@file` when absent.
#[inline]
pub fn artifact_ref_opt(file: &str, line: Option<u32>) -> String {
    match line {
        Some(l) => format!("@{}:{}", file, l),
        None => format!("@{}", file),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_ref() {
        assert_eq!(artifact_ref("src/main.rs", 42), "@src/main.rs:42");
    }

    #[test]
    fn test_artifact_ref_opt() {
        assert_eq!(artifact_ref_opt("src/main.rs", Some(42)), "@src/main.rs:42");
        assert_eq!(artifact_ref_opt("src/main.rs", None), "@src/main.rs");
    }
}
