//! Common Verification Utilities
//!
//! Shared utilities for code analysis and extraction.
//! Eliminates duplicate pattern matching across verifier components.

/// Code pattern extraction utilities
pub mod patterns {
    /// Function declaration patterns for various languages
    const FUNCTION_PATTERNS: &[(&str, &str)] = &[
        ("function ", "("),  // JavaScript
        ("fn ", "("),        // Rust
        ("def ", "("),       // Python
        ("func ", "("),      // Go
        ("public ", "("),    // Java/C#
        ("private ", "("),   // Java/C#
        ("protected ", "("), // Java/C#
        ("static ", "("),    // Static methods
        ("async ", "("),     // Async functions
    ];

    /// Class/struct declaration patterns
    const CLASS_PATTERNS: &[(&str, &str)] = &[
        ("class ", "{"),
        ("class ", "("),     // Python with inheritance
        ("class ", ":"),     // Python
        ("struct ", "{"),    // C/Rust/Go
        ("interface ", "{"), // TypeScript/Java
        ("trait ", "{"),     // Rust
    ];

    /// Type definition patterns
    const TYPE_PATTERNS: &[(&str, &str)] = &[
        ("type ", "="),    // TypeScript/Rust
        ("typedef ", " "), // C/C++
        ("enum ", "{"),    // Various languages
        ("type ", "{"),    // Go
    ];

    /// Extract function name from a declaration statement
    ///
    /// # Examples
    /// ```
    /// use weavewiki::verifier::common::patterns::extract_function_name;
    ///
    /// assert_eq!(extract_function_name("fn hello()"), Some("hello".to_string()));
    /// assert_eq!(extract_function_name("function greet(name)"), Some("greet".to_string()));
    /// assert_eq!(extract_function_name("def process(data):"), Some("process".to_string()));
    /// ```
    pub fn extract_function_name(statement: &str) -> Option<String> {
        extract_identifier(statement, FUNCTION_PATTERNS)
    }

    /// Extract class/struct name from a declaration statement
    ///
    /// # Examples
    /// ```
    /// use weavewiki::verifier::common::patterns::extract_class_name;
    ///
    /// assert_eq!(extract_class_name("class MyClass {"), Some("MyClass".to_string()));
    /// assert_eq!(extract_class_name("struct Point {"), Some("Point".to_string()));
    /// ```
    pub fn extract_class_name(statement: &str) -> Option<String> {
        extract_identifier(statement, CLASS_PATTERNS)
    }

    /// Extract type name from a definition statement
    pub fn extract_type_name(statement: &str) -> Option<String> {
        extract_identifier(statement, TYPE_PATTERNS)
    }

    /// Extract dependency target from import/use statement
    ///
    /// # Examples
    /// ```
    /// use weavewiki::verifier::common::patterns::extract_dependency_target;
    ///
    /// assert_eq!(extract_dependency_target("import foo from 'bar'"), Some("bar".to_string()));
    /// assert_eq!(extract_dependency_target("use crate::module;"), Some("crate::module".to_string()));
    /// ```
    pub fn extract_dependency_target(statement: &str) -> Option<String> {
        let trimmed = statement.trim();

        // ES6 import: import X from 'path'
        if let Some(from_idx) = trimmed.find("from ") {
            let after = &trimmed[from_idx + 5..];
            let cleaned = after
                .trim()
                .trim_matches(|c| c == '\'' || c == '"' || c == ';');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }

        // Rust use: use path;
        if let Some(rest) = trimmed.strip_prefix("use ") {
            let path = rest.trim_end_matches(';').trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }

        // CommonJS require: require('path')
        if let Some(req_idx) = trimmed.find("require(") {
            let after = &trimmed[req_idx + 8..];
            if let Some(end) = after.find(')') {
                let path = after[..end].trim_matches(|c| c == '\'' || c == '"');
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }

        None
    }

    /// Generic identifier extraction from patterns
    fn extract_identifier(text: &str, patterns: &[(&str, &str)]) -> Option<String> {
        let trimmed = text.trim();

        for (prefix, suffix) in patterns {
            if let Some(start_idx) = trimmed.find(prefix) {
                let after_prefix = &trimmed[start_idx + prefix.len()..];

                // Find identifier end (either at suffix or whitespace)
                let end_idx = after_prefix.find(suffix).unwrap_or_else(|| {
                    after_prefix
                        .find(char::is_whitespace)
                        .unwrap_or(after_prefix.len())
                });

                let identifier = after_prefix[..end_idx].trim();

                // Validate identifier (alphanumeric + underscore)
                if !identifier.is_empty()
                    && identifier.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    return Some(identifier.to_string());
                }
            }
        }

        None
    }

    /// Normalize a function signature for comparison
    ///
    /// Collapses whitespace and normalizes formatting for reliable comparison.
    pub fn normalize_signature(sig: &str) -> String {
        sig.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Check if content contains a function with the given name
    pub fn contains_function(content: &str, name: &str) -> bool {
        FUNCTION_PATTERNS
            .iter()
            .any(|(prefix, suffix)| content.contains(&format!("{}{}{}", prefix, name, suffix)))
    }

    /// Check if content contains a class/struct with the given name
    pub fn contains_class(content: &str, name: &str) -> bool {
        CLASS_PATTERNS
            .iter()
            .any(|(prefix, _suffix)| content.contains(&format!("{}{}", prefix, name)))
    }

    /// Check if content contains a type with the given name
    pub fn contains_type(content: &str, name: &str) -> bool {
        TYPE_PATTERNS
            .iter()
            .any(|(prefix, _)| content.contains(&format!("{}{}", prefix, name)))
    }
}

#[cfg(test)]
mod tests {
    use super::patterns::*;

    #[test]
    fn test_extract_function_name_rust() {
        assert_eq!(
            extract_function_name("fn hello()"),
            Some("hello".to_string())
        );
        assert_eq!(
            extract_function_name("pub fn process(data: &str)"),
            Some("process".to_string())
        );
        assert_eq!(
            extract_function_name("async fn fetch()"),
            Some("fetch".to_string())
        );
    }

    #[test]
    fn test_extract_function_name_js() {
        assert_eq!(
            extract_function_name("function greet(name)"),
            Some("greet".to_string())
        );
        assert_eq!(
            extract_function_name("async function load()"),
            Some("load".to_string())
        );
    }

    #[test]
    fn test_extract_function_name_python() {
        assert_eq!(
            extract_function_name("def process(data):"),
            Some("process".to_string())
        );
    }

    #[test]
    fn test_extract_class_name() {
        assert_eq!(
            extract_class_name("class MyClass {"),
            Some("MyClass".to_string())
        );
        assert_eq!(
            extract_class_name("struct Point {"),
            Some("Point".to_string())
        );
        assert_eq!(
            extract_class_name("interface IHandler {"),
            Some("IHandler".to_string())
        );
        assert_eq!(
            extract_class_name("trait Serialize {"),
            Some("Serialize".to_string())
        );
    }

    #[test]
    fn test_extract_dependency_target() {
        assert_eq!(
            extract_dependency_target("import foo from 'bar'"),
            Some("bar".to_string())
        );
        assert_eq!(
            extract_dependency_target("use crate::module;"),
            Some("crate::module".to_string())
        );
        assert_eq!(
            extract_dependency_target("const x = require('lodash')"),
            Some("lodash".to_string())
        );
    }

    #[test]
    fn test_normalize_signature() {
        let sig = "fn  hello(  x: i32,   y: i32  )  ->  i32";
        assert_eq!(
            normalize_signature(sig),
            "fn hello( x: i32, y: i32 ) -> i32"
        );
    }

    #[test]
    fn test_contains_function() {
        let content = "pub fn process(data: &str) -> Result<()>";
        assert!(contains_function(content, "process"));
        assert!(!contains_function(content, "nonexistent"));
    }
}
