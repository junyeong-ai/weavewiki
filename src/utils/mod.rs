//! Utility modules - No pipeline dependencies
//!
//! Pure utility functions that can be used by both types and pipeline modules.

pub mod path;
pub mod patterns;
pub mod tools;

pub use path::{PathResolution, safe_join, safe_resolve};
pub use patterns::*;
pub use tools::{
    MCP_TOOL_PREFIX, VALID_TOOLS, is_bash_pattern, is_mcp_tool, is_valid_tool, parse_bash_pattern,
    parse_mcp_tool, requires_permission,
};

/// Convert a string to kebab-case (lowercase with hyphens).
pub fn to_kebab_case(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_whitespace() || c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<String>()
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Capitalize the first character of a string.
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}
