//! Pipeline Patterns Module
//!
//! Re-exports core patterns from utils.
//! For advanced file reference handling (resolution, validation), use pipeline::file_reference.

pub use crate::utils::patterns::{
    extract_file_refs, extract_paths, count_file_line_refs, count_file_refs,
    count_generic_patterns, count_tier3_indicators, count_value_indicators, FileRef,
    ACTIONABLE_PATTERN, FILE_REFERENCE_PATTERN, GENERIC_COMMANDS, GENERIC_PATTERN,
    TIER3_INDICATORS, VALUE_INDICATORS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_references() {
        assert!(count_file_line_refs("See @src/main.rs:42") > 0);
        assert_eq!(count_file_refs("email@example.com"), 0);
        assert!(count_file_refs("Check @src/lib.rs") > 0);
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
    fn test_tier3_indicators() {
        assert_eq!(count_tier3_indicators("race condition detected"), 1);
        assert_eq!(count_tier3_indicators("normal code"), 0);
    }

    #[test]
    fn test_extract_paths() {
        let content = "See @src/main.rs:42 and @src/lib.rs for details";
        let refs = extract_paths(content);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"src/main.rs".to_string()));
        assert!(refs.contains(&"src/lib.rs".to_string()));
    }
}
