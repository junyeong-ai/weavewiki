//! Global Constants - Truly constant values only
//! All configurable values have been moved to config/types.rs

pub mod plugin {
    pub const DEFAULT_NAME: &str = ".claudegen";
    pub const DEFAULT_AUTHOR: &str = "claudegen";
    pub const DEFAULT_DESCRIPTION: &str =
        "AI-generated Claude Code plugin with project-specific skills and agents";
    pub const MAX_NAME_LENGTH: usize = 64;
}

pub mod tools {
    pub const DEFAULT_TOOLS: &[&str] = &["Read", "Glob", "Grep"];
    pub const BASH: &str = "Bash";
}

pub mod llm {
    pub const MIN_TEMPERATURE: f32 = 0.0;
    pub const MAX_TEMPERATURE: f32 = 2.0;
}

pub mod provider {
    pub const DEFAULT_MAX_TOKENS: usize = 4096;
    pub const CLAUDE_AGENT_MAX_TOKENS: usize = 16384;
    pub const HEALTH_CHECK_MAX_TOKENS: u32 = 10;
    pub const RATE_LIMIT_MAX_DELAY_SECS: u64 = 300;
}

pub mod validation {
    pub const MAX_REPAIR_ATTEMPTS: usize = 3;
    pub const ERROR_PREVIEW_LENGTH: usize = 200;
}

pub mod cli {
    pub const PROGRESS_REPORT_INTERVAL: usize = 100;
}
