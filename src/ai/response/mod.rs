//! Unified LLM Response Parsing System
//!
//! Schema-first architecture with partial parsing support.
//!
//! # Design Philosophy
//!
//! Traditional LLM response parsing fails entirely when any field doesn't match
//! the expected schema, losing all information. This module provides:
//!
//! 1. **Partial Parsing**: Extract as much valid data as possible
//! 2. **Field-level Recovery**: Default values for unparseable fields
//! 3. **Diagnostic Reporting**: Detailed error information for debugging
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::ai::response::{parse_partial, PartialParseable, ParseResult};
//!
//! let result: ParseResult<MyType> = parse_partial(&json_value, "context");
//! if result.is_usable() {
//!     // Use result.data even if some fields failed
//! }
//! result.log_status("my_operation");
//! ```

mod partial;
mod schema;
mod types;

pub use partial::{
    FieldParseError, PartialParseable, SetFieldResult, extract_bool, extract_f32, extract_field,
    extract_optional, extract_string, extract_string_array, extract_u32, parse_partial,
};
pub use schema::{generate_schema, transform_for_strict};
pub use types::{FieldError, ParseResult, ParseStatus, RecoveryAction};
