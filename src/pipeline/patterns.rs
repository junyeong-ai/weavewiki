//! Pipeline Patterns Module
//!
//! Re-exports file reference patterns from utils.
//! For advanced file reference handling (resolution, validation), use pipeline::file_reference.

pub use crate::utils::patterns::{
    FILE_REFERENCE_PATTERN, FileRef, count_file_line_refs, count_file_refs, extract_file_refs,
    extract_paths,
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
    fn test_extract_paths() {
        let content = "See @src/main.rs:42 and @src/lib.rs for details";
        let refs = extract_paths(content);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"src/main.rs".to_string()));
        assert!(refs.contains(&"src/lib.rs".to_string()));
    }
}
