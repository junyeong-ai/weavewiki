//! Language Parser Module
//!
//! Tree-sitter based parsers for multiple programming languages.
//!
//! ## Parser Factory
//!
//! Use `create_parser` to create a parser for a given language:
//!
//! ```rust,ignore
//! use weavewiki::analyzer::parser::{Language, create_parser};
//!
//! let parser = create_parser(Language::Rust)?;
//! let result = parser.parse("main.rs", content)?;
//! ```

pub mod bash;
pub mod c;
pub mod cpp;
pub mod go;
pub mod java;
pub mod kotlin;
pub mod language;
pub mod python;
pub mod ruby;
pub mod rust_lang;
pub mod traits;
pub mod typescript;

pub use bash::BashParser;
pub use c::CLangParser;
pub use cpp::CppLangParser;
pub use go::GoParser;
pub use java::JavaParser;
pub use kotlin::KotlinParser;
pub use language::{Language, detect_language, detect_language_or_text};
pub use python::PythonParser;
pub use ruby::RubyParser;
pub use rust_lang::RustParser;
pub use traits::{
    ParseResult, Parser, QueryMatch, create_code_edge, create_code_node, create_dependency_edge,
    create_file_node, create_ts_parser, evidence_from_node, execute_query, get_node_position,
    get_node_text, query_captures,
};
pub use typescript::TypeScriptParser;

use crate::types::{Result, WeaveError};
use std::sync::Arc;

/// Shared parser for thread-safe access
pub type SharedParser = Arc<dyn Parser>;

/// Create a parser for the given language.
///
/// Returns a boxed parser trait object for the specified language.
/// Returns an error if the language is not supported for parsing.
///
/// # Supported Languages
///
/// - Rust, Go, C, C++
/// - Python, Ruby
/// - TypeScript, JavaScript, TSX, JSX
/// - Java, Kotlin
/// - Bash
///
/// # Example
///
/// ```rust,ignore
/// let parser = create_parser(Language::Python)?;
/// let result = parser.parse("app.py", source_code)?;
/// ```
pub fn create_parser(language: Language) -> Result<Box<dyn Parser>> {
    match language {
        Language::Rust => Ok(Box::new(RustParser::new()?)),
        Language::Go => Ok(Box::new(GoParser::new()?)),
        Language::C => Ok(Box::new(CLangParser::new()?)),
        Language::Cpp => Ok(Box::new(CppLangParser::new()?)),
        Language::Python => Ok(Box::new(PythonParser::new()?)),
        Language::Ruby => Ok(Box::new(RubyParser::new()?)),
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            Ok(Box::new(TypeScriptParser::new()?))
        }
        Language::Java => Ok(Box::new(JavaParser::new()?)),
        Language::Kotlin => Ok(Box::new(KotlinParser::new()?)),
        Language::Bash => Ok(Box::new(BashParser::new()?)),
        _ => Err(WeaveError::Config(format!(
            "No parser support for language: {}",
            language
        ))),
    }
}

/// Create a shared parser for concurrent access.
///
/// Wraps the parser in an Arc for thread-safe sharing.
pub fn create_shared_parser(language: Language) -> Result<SharedParser> {
    let parser = create_parser(language)?;
    Ok(Arc::from(parser))
}

/// Try to create a parser for a file path.
///
/// Detects the language from the file extension and creates the appropriate parser.
/// Returns None if the language is not detected or not supported.
pub fn create_parser_for_path(path: &str) -> Option<Box<dyn Parser>> {
    let language = Language::from_path(path);
    if language.has_parser_support() {
        create_parser(language).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_parser_rust() {
        let parser = create_parser(Language::Rust);
        assert!(parser.is_ok());
        assert_eq!(parser.unwrap().language(), Language::Rust);
    }

    #[test]
    fn test_create_parser_python() {
        let parser = create_parser(Language::Python);
        assert!(parser.is_ok());
        assert_eq!(parser.unwrap().language(), Language::Python);
    }

    #[test]
    fn test_create_parser_unsupported() {
        let parser = create_parser(Language::Elixir);
        assert!(parser.is_err());
    }

    #[test]
    fn test_create_parser_for_path() {
        let parser = create_parser_for_path("src/main.rs");
        assert!(parser.is_some());

        let parser = create_parser_for_path("unknown.xyz");
        assert!(parser.is_none());
    }

    #[test]
    fn test_create_shared_parser() {
        let parser = create_shared_parser(Language::Go);
        assert!(parser.is_ok());
    }
}
