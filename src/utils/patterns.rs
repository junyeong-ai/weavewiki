//! File Reference Pattern Utilities
//!
//! Deterministic pattern matching for file references.
//! Tier classification is handled by LLM, not by static patterns.

use std::sync::LazyLock;

use regex::Regex;

/// File reference pattern for @path/to/file:line format
/// Matches: @path/to/file.ext, @path/to/file.ext:42, @path/to/file.ext:42-50
///
/// Supports:
/// - Unicode characters in path names (non-ASCII filenames)
/// - Paths with spaces when quoted: @"path with spaces/file.rs"
/// - Standard path separators (/, \)
/// - Common path characters (., _, -)
///
/// Known Limitations:
/// - Requires delimiter before `@` (space, `(`, `[`, `{`, `,`, `;`, or line start)
/// - Won't match `@file:line` after markdown bullet (`-`) or colon (`:`)
/// - Designed for Claude Code context output format; other formats may not match
/// - Backtick-wrapped refs (`` `@path` ``) won't match the inner path
///
/// These limitations are acceptable because:
/// 1. Claude Code outputs references with consistent formatting
/// 2. False negatives are preferable to false positives (email matching)
/// 3. Programmatic extraction supplements, not replaces, LLM understanding
pub static FILE_REFERENCE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern explanation:
    // - (?:^|[\s\(\[\{,;]) - Start of string or common delimiters (not alphanumeric to avoid emails)
    // - @"([^"]+)" - Quoted path (supports spaces and special chars)
    // - OR @([^\s@:,;()\[\]{}]+) - Unquoted path (Unicode-safe, excludes problematic chars)
    // - (?::(\d+)(?:-(\d+))?)? - Optional :line or :line-line range
    Regex::new(r#"(?:^|[\s\(\[\{,;])@(?:"([^"]+)"|([^\s@:,;()\[\]{}"][^\s@:,;()\[\]{}]*))(?::(\d+)(?:-(\d+))?)?"#)
        .expect("Invalid regex")
});

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
            // Group 1 = quoted path, Group 2 = unquoted path
            let path = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str().to_string())?;

            // Filter out non-file patterns
            if path.is_empty()
                || path.starts_with("http")
                || path.starts_with("https")
                || path.starts_with("CLAUDE")
                || path.contains('@') // Likely an email
                || path.chars().all(|c| c.is_ascii_digit())
            // Just numbers
            {
                return None;
            }

            // Line numbers are in groups 3 and 4 now
            let line_start = cap.get(3).and_then(|m| m.as_str().parse().ok());
            let line_end = cap.get(4).and_then(|m| m.as_str().parse().ok());

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
    fn test_file_ref_with_range() {
        let refs = extract_file_refs("See @src/main.rs:10-20 for details");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].line_start, Some(10));
        assert_eq!(refs[0].line_end, Some(20));
    }

    #[test]
    fn test_quoted_path_with_spaces() {
        let refs = extract_file_refs(r#"See @"src/my module/file.rs":42 for details"#);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/my module/file.rs");
        assert_eq!(refs[0].line_start, Some(42));
    }

    #[test]
    fn test_unicode_path() {
        let refs = extract_file_refs("Check @src/コンポーネント/main.rs for Japanese component");
        assert_eq!(refs.len(), 1);
        assert!(refs[0].path.contains("コンポーネント"));
    }

    #[test]
    fn test_multiple_refs() {
        let refs = extract_file_refs("See @src/a.rs:1, @src/b.rs:2 and @src/c.rs");
        assert_eq!(refs.len(), 3);
    }
}
