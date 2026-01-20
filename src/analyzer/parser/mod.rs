//! Language Parser Module
//!
//! Tree-sitter based parsers for essential programming languages.

pub mod bash;
pub mod go;
pub mod language;
pub mod python;
pub mod rust_lang;
pub mod traits;
pub mod typescript;

pub use bash::BashParser;
pub use go::GoParser;
pub use language::{Language, detect_language, detect_language_or_text};
pub use python::PythonParser;
pub use rust_lang::RustParser;
pub use traits::{
    ParseResult, Parser, QueryMatch, create_code_edge, create_code_node, create_dependency_edge,
    create_file_node, create_ts_parser, evidence_from_node, execute_query, get_node_position,
    get_node_text, query_captures,
};
pub use typescript::TypeScriptParser;

use crate::types::{ClaudegenError, Result};
use std::sync::Arc;

pub type SharedParser = Arc<dyn Parser>;

pub fn create_parser(language: Language) -> Result<Box<dyn Parser>> {
    match language {
        Language::Rust => Ok(Box::new(RustParser::new()?)),
        Language::Go => Ok(Box::new(GoParser::new()?)),
        Language::Python => Ok(Box::new(PythonParser::new()?)),
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            Ok(Box::new(TypeScriptParser::new()?))
        }
        Language::Bash => Ok(Box::new(BashParser::new()?)),
        _ => Err(ClaudegenError::Config(format!(
            "No parser support for language: {language}"
        ))),
    }
}

pub fn create_shared_parser(language: Language) -> Result<SharedParser> {
    let parser = create_parser(language)?;
    Ok(Arc::from(parser))
}

pub fn create_parser_for_path(path: &str) -> Option<Box<dyn Parser>> {
    let language = Language::from_path(path);
    if language.has_parser_support() {
        create_parser(language).ok()
    } else {
        None
    }
}
