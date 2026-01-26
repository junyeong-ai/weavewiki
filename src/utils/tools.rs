//! Claude Code Tool Definitions
//!
//! Canonical source of truth for Claude Code tool validation.
//! Based on Claude Code latest version (claude-opus-4-5-20251101)
//!
//! Tool naming patterns:
//! - Built-in tools: PascalCase (e.g., "Read", "WebFetch")
//! - MCP tools: `mcp__<server>__<tool>` (e.g., "mcp__context7__query-docs")
//! - Bash with patterns: `Bash(<pattern>)` (e.g., "Bash(npm run *)")

/// All valid Claude Code built-in tools
///
/// Updated: 2025-01 (Claude Code v1.0.33+)
pub const VALID_TOOLS: &[&str] = &[
    // File operations
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "NotebookEdit",
    // Execution
    "Bash",
    // Web
    "WebFetch",
    "WebSearch",
    // Task management
    "Task",
    "TaskOutput",
    "TaskStop",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskUpdate",
    // User interaction
    "AskUserQuestion",
    "Skill",
    // Plan mode
    "EnterPlanMode",
    "ExitPlanMode",
    // MCP
    "MCPSearch",
];

/// Tools that do NOT require permission prompts
pub const PERMISSION_FREE_TOOLS: &[&str] = &[
    "AskUserQuestion",
    "Glob",
    "Grep",
    "Read",
    "Task",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TaskUpdate",
];

/// MCP tool name prefix
pub const MCP_TOOL_PREFIX: &str = "mcp__";

/// Check if a tool name is valid
///
/// Accepts:
/// - Built-in tools from VALID_TOOLS
/// - MCP tools with `mcp__<server>__<tool>` pattern
/// - Bash with command patterns like `Bash(npm run *)`
#[inline]
pub fn is_valid_tool(name: &str) -> bool {
    // Check built-in tools first (most common case)
    if VALID_TOOLS.contains(&name) {
        return true;
    }

    // Check MCP tool pattern: mcp__<server>__<tool>
    if is_mcp_tool(name) {
        return true;
    }

    // Check Bash with command pattern: Bash(pattern)
    if is_bash_pattern(name) {
        return true;
    }

    false
}

/// Check if the tool name is an MCP tool
///
/// MCP tools follow the pattern: `mcp__<server>__<tool>`
/// Example: `mcp__context7__query-docs`
#[inline]
pub fn is_mcp_tool(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix(MCP_TOOL_PREFIX) {
        // Must have at least server__tool (contains another __)
        rest.contains("__") && !rest.starts_with('_') && !rest.ends_with('_')
    } else {
        false
    }
}

/// Check if the tool name is a Bash command pattern
///
/// Bash patterns allow restricting Bash to specific commands.
/// Example: `Bash(npm run *)`, `Bash(cargo test*)`
#[inline]
pub fn is_bash_pattern(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("Bash(") {
        rest.ends_with(')') && rest.len() > 1
    } else {
        false
    }
}

/// Extract MCP server and tool names from an MCP tool
///
/// Returns `Some((server, tool))` if valid MCP tool, `None` otherwise.
pub fn parse_mcp_tool(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix(MCP_TOOL_PREFIX)?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// Extract the command pattern from a Bash pattern tool
///
/// Returns `Some(pattern)` if valid Bash pattern, `None` otherwise.
pub fn parse_bash_pattern(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("Bash(")?;
    let pattern = rest.strip_suffix(')')?;
    if pattern.is_empty() {
        return None;
    }
    Some(pattern)
}

/// Check if a tool requires permission
#[inline]
pub fn requires_permission(name: &str) -> bool {
    !PERMISSION_FREE_TOOLS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_valid() {
        // All permission-free tools should be valid
        for tool in PERMISSION_FREE_TOOLS {
            assert!(
                is_valid_tool(tool),
                "{} is permission-free but not in VALID_TOOLS",
                tool
            );
        }
    }

    #[test]
    fn test_permission_required_tools() {
        // These tools require permission
        assert!(requires_permission("Bash"));
        assert!(requires_permission("Edit"));
        assert!(requires_permission("Write"));
        assert!(requires_permission("WebFetch"));

        // These do not
        assert!(!requires_permission("Read"));
        assert!(!requires_permission("Glob"));
        assert!(!requires_permission("Task"));
    }

    #[test]
    fn test_valid_tool_check() {
        assert!(is_valid_tool("Read"));
        assert!(is_valid_tool("TaskCreate"));
        assert!(is_valid_tool("TaskStop"));
        assert!(is_valid_tool("EnterPlanMode"));
        assert!(!is_valid_tool("TodoWrite")); // Removed
        assert!(!is_valid_tool("KillShell")); // Renamed to TaskStop
        assert!(!is_valid_tool("InvalidTool"));
    }

    #[test]
    fn test_mcp_tools() {
        // Valid MCP tools
        assert!(is_valid_tool("mcp__context7__query-docs"));
        assert!(is_valid_tool("mcp__context7__resolve-library-id"));
        assert!(is_valid_tool(
            "mcp__sequential-thinking__sequentialthinking"
        ));
        assert!(is_valid_tool("mcp__my-server__my-tool"));

        // Invalid MCP patterns
        assert!(!is_valid_tool("mcp__")); // No server or tool
        assert!(!is_valid_tool("mcp__server")); // No tool
        assert!(!is_valid_tool("mcp____tool")); // Empty server
        assert!(!is_valid_tool("mcp__server__")); // Empty tool
        assert!(!is_mcp_tool("Read")); // Not MCP
    }

    #[test]
    fn test_bash_patterns() {
        // Valid Bash patterns
        assert!(is_valid_tool("Bash(npm run *)"));
        assert!(is_valid_tool("Bash(cargo test*)"));
        assert!(is_valid_tool("Bash(git commit -m *)"));
        assert!(is_valid_tool("Bash(echo hello)"));

        // Invalid Bash patterns
        assert!(!is_valid_tool("Bash()")); // Empty pattern
        assert!(!is_valid_tool("Bash(")); // Missing close
        assert!(!is_bash_pattern("Bash")); // No pattern
    }

    #[test]
    fn test_parse_mcp_tool() {
        assert_eq!(
            parse_mcp_tool("mcp__context7__query-docs"),
            Some(("context7", "query-docs"))
        );
        assert_eq!(
            parse_mcp_tool("mcp__my-server__my-tool"),
            Some(("my-server", "my-tool"))
        );
        assert_eq!(parse_mcp_tool("Read"), None);
        assert_eq!(parse_mcp_tool("mcp__server"), None);
    }

    #[test]
    fn test_parse_bash_pattern() {
        assert_eq!(parse_bash_pattern("Bash(npm run *)"), Some("npm run *"));
        assert_eq!(parse_bash_pattern("Bash(cargo test)"), Some("cargo test"));
        assert_eq!(parse_bash_pattern("Bash()"), None);
        assert_eq!(parse_bash_pattern("Bash"), None);
    }
}
