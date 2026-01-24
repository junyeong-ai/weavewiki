//! Claude Code Hook types based on official documentation

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(rename = "PreToolUse", skip_serializing_if = "Option::is_none")]
    pub pre_tool_use: Option<Vec<Hook>>,
    #[serde(rename = "PostToolUse", skip_serializing_if = "Option::is_none")]
    pub post_tool_use: Option<Vec<Hook>>,
    #[serde(rename = "PermissionRequest", skip_serializing_if = "Option::is_none")]
    pub permission_request: Option<Vec<Hook>>,
    #[serde(rename = "UserPromptSubmit", skip_serializing_if = "Option::is_none")]
    pub user_prompt_submit: Option<Vec<Hook>>,
    #[serde(rename = "Notification", skip_serializing_if = "Option::is_none")]
    pub notification: Option<Vec<Hook>>,
    #[serde(rename = "Stop", skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<Hook>>,
    #[serde(rename = "SubagentStop", skip_serializing_if = "Option::is_none")]
    pub subagent_stop: Option<Vec<Hook>>,
    #[serde(rename = "SessionStart", skip_serializing_if = "Option::is_none")]
    pub session_start: Option<Vec<Hook>>,
    #[serde(rename = "SessionEnd", skip_serializing_if = "Option::is_none")]
    pub session_end: Option<Vec<Hook>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hook {
    pub matcher: String,
    pub hooks: Vec<HookCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookCommand {
    #[serde(rename = "type")]
    pub command_type: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub once: Option<bool>,
}

impl Hook {
    pub fn new(matcher: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            matcher: matcher.into(),
            hooks: vec![HookCommand {
                command_type: "command".to_string(),
                command: command.into(),
                once: None,
            }],
        }
    }

    pub fn with_once(mut self) -> Self {
        if let Some(cmd) = self.hooks.first_mut() {
            cmd.once = Some(true);
        }
        self
    }
}

/// Simplified hooks for Skills and Agents (PreToolUse/PostToolUse only)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolHooks {
    #[serde(rename = "PreToolUse", skip_serializing_if = "Option::is_none")]
    pub pre_tool_use: Option<Vec<Hook>>,
    #[serde(rename = "PostToolUse", skip_serializing_if = "Option::is_none")]
    pub post_tool_use: Option<Vec<Hook>>,
}
