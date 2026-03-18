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

pub mod context {
    /// Default model context window limit (200k tokens).
    pub const MODEL_CONTEXT_LIMIT: usize = 200_000;

    /// Return the context window limit for a given model name.
    pub fn context_limit_for_model(model: &str) -> usize {
        let m = model.to_lowercase();
        if m.contains("gpt-4o") || m.contains("gpt-4-turbo") {
            128_000
        } else {
            // Claude, o1, Gemini, and unknown models get the full window
            MODEL_CONTEXT_LIMIT
        }
    }
}

pub mod token_estimation {
    /// Average characters per token for ASCII text.
    pub const CHARS_PER_TOKEN: usize = 4;
    /// Average characters per token for non-ASCII text (CJK, Cyrillic, etc.).
    pub const NON_ASCII_CHARS_PER_TOKEN: usize = 2;
}

pub mod refinement {
    pub const DEFAULT_CHECKPOINT_EVERY_ITERATIONS: usize = 5;
    pub const MAX_LEVEL_HISTORY: usize = 100;
    pub const MAX_QUALITY_HISTORY: usize = 100;
    pub const CONVERGENCE_ACCEPTABLE_RATIO: f32 = 0.8;
    pub const DIMENSION_THRESHOLD_MULTIPLIER: f32 = 0.9;
    pub const FLOOR_CONVERGENCE_PASS_MULTIPLIER: f32 = 1.5;
    pub const LOW_VALIDITY_SCORE_THRESHOLD: f32 = 0.3;
    pub const MIN_ACTIONABILITY_THRESHOLD: f32 = 0.3;
    pub const MIN_ARTIFACT_CONTENT_LENGTH: usize = 50;
    pub const OSCILLATION_DETECTION_THRESHOLD: f32 = 0.6;
    pub const REDUNDANCY_THRESHOLD: f32 = 0.7;
}

pub mod event_store {
    pub const DEFAULT_SHARD_SIZE: usize = 1000;
    pub const INDEX_SAVE_INTERVAL: usize = 100;
}

pub mod artifact_dirs {
    pub const AGENTS_DIR: &str = ".claudegen/agents";
    pub const RULES_DIR: &str = ".claudegen/rules";
    pub const SKILLS_DIR: &str = ".claudegen/skills";
}

pub mod scanner {
    pub const SOURCE_EXTENSIONS: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "kt", "java", "cs",
        "cpp", "c", "h", "hpp", "swift", "rb", "php", "scala", "ex", "exs",
        "clj", "hs", "ml", "fs", "dart", "lua", "r", "jl", "zig", "nim",
        "cr", "v", "d", "ada", "pl", "pm", "sh", "bash", "zsh",
    ];
}
