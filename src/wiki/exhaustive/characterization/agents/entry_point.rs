//! Entry Point Agent - Turn 1
//!
//! Detects application entry points and public API surfaces using LLM.

use super::helpers::{AgentConfig, calculate_confidence, run_agent};
use super::{EntryPointInfo, EntryPointInsight};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};

#[derive(Default)]
pub struct EntryPointAgent;

impl EntryPointAgent {
    /// Build file samples for the prompt
    fn build_file_samples(context: &CharacterizationContext) -> String {
        let mut samples = String::new();
        samples.push_str("Key files that may contain entry points:\n");

        // Focus on common entry point files (language-agnostic patterns)
        const ENTRY_PATTERNS: &[&str] = &[
            "main.rs",
            "lib.rs",
            "cli.rs",
            "main.go",
            "cmd/",
            "main.py",
            "app.py",
            "server.py",
            "__main__.py",
            "manage.py",
            "wsgi.py",
            "asgi.py",
            "index.ts",
            "index.js",
            "index.tsx",
            "index.jsx",
            "main.ts",
            "main.js",
            "app.ts",
            "app.js",
            "server.ts",
            "server.js",
            "Main.java",
            "Application.java",
            "App.java",
            "Main.kt",
            "Application.kt",
            "main.c",
            "main.cpp",
            "main.cc",
            "Program.cs",
            "Startup.cs",
            "Main.cs",
            "main.rb",
            "app.rb",
            "application.rb",
            "config.ru",
            "index.php",
            "artisan",
            "public/index.php",
            "main.swift",
            "App.swift",
            "AppDelegate.swift",
            "bin/",
            "src/bin/",
            "entry",
            "bootstrap",
        ];

        for file in &context.files {
            let is_entry = ENTRY_PATTERNS.iter().any(|p| file.path.contains(p));
            if is_entry {
                samples.push_str(&format!(
                    "- {} ({}, {} lines) - LIKELY ENTRY POINT\n",
                    file.path,
                    file.language.as_deref().unwrap_or("unknown"),
                    file.line_count
                ));
            }
        }

        if samples.lines().count() <= 1 {
            samples.push_str("\nAll project files:\n");
            for file in context.files.iter().take(50) {
                samples.push_str(&format!(
                    "- {} ({})\n",
                    file.path,
                    file.language.as_deref().unwrap_or("unknown")
                ));
            }
        }

        samples
    }

    /// Fallback analysis when LLM fails (language-agnostic)
    fn fallback_analysis(context: &CharacterizationContext) -> EntryPointInsight {
        let mut entry_points = vec![];
        let mut public_surface = vec![];
        let mut cli_commands = vec![];

        const MAIN_PATTERNS: &[&str] = &[
            "main.rs",
            "main.go",
            "main.py",
            "main.c",
            "main.cpp",
            "main.cc",
            "main.java",
            "Main.java",
            "main.kt",
            "Main.kt",
            "main.swift",
            "main.rb",
            "Program.cs",
            "__main__.py",
        ];

        const LIB_PATTERNS: &[&str] = &[
            "lib.rs",
            "index.ts",
            "index.js",
            "index.tsx",
            "index.jsx",
            "index.mjs",
            "__init__.py",
            "mod.rs",
        ];

        const APP_PATTERNS: &[&str] = &[
            "app.py",
            "app.ts",
            "app.js",
            "server.py",
            "server.ts",
            "server.js",
            "Application.java",
            "App.java",
            "Application.kt",
            "App.kt",
            "Startup.cs",
            "app.rb",
            "application.rb",
        ];

        for file in &context.files {
            let filename = file.path.split('/').next_back().unwrap_or(&file.path);

            if MAIN_PATTERNS
                .iter()
                .any(|p| filename == *p || file.path.ends_with(p))
            {
                entry_points.push(EntryPointInfo {
                    entry_type: "main".to_string(),
                    file: file.path.clone(),
                    symbol: Some("main".to_string()),
                });
            } else if LIB_PATTERNS
                .iter()
                .any(|p| filename == *p || file.path.ends_with(p))
            {
                entry_points.push(EntryPointInfo {
                    entry_type: "lib".to_string(),
                    file: file.path.clone(),
                    symbol: None,
                });
                public_surface.push(file.path.clone());
            } else if APP_PATTERNS
                .iter()
                .any(|p| filename == *p || file.path.ends_with(p))
            {
                entry_points.push(EntryPointInfo {
                    entry_type: "app".to_string(),
                    file: file.path.clone(),
                    symbol: None,
                });
            } else if file.path.contains("/bin/") || file.path.contains("/cmd/") {
                let name = filename.split('.').next().unwrap_or("unknown");
                entry_points.push(EntryPointInfo {
                    entry_type: "bin".to_string(),
                    file: file.path.clone(),
                    symbol: Some(name.to_string()),
                });
                cli_commands.push(name.to_string());
            }
        }

        EntryPointInsight {
            entry_points,
            public_surface,
            cli_commands,
        }
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for EntryPointAgent {
    fn name(&self) -> &str {
        "entry_point"
    }

    fn turn(&self) -> u8 {
        1
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        run_agent(
            context,
            AgentConfig {
                name: "entry_point",
                turn: 1,
                schema: AgentSchemas::entry_point_schema(),
                build_prompt: Box::new(|ctx| {
                    let file_samples = Self::build_file_samples(ctx);
                    AgentPrompts::entry_point_prompt(&file_samples)
                }),
                fallback: Box::new(Self::fallback_analysis),
                confidence: Box::new(|insight: &EntryPointInsight| {
                    calculate_confidence(insight.entry_points.is_empty())
                }),
                debug_result: Box::new(|insight: &EntryPointInsight| {
                    format!(
                        "Found {} entry points, {} CLI commands",
                        insight.entry_points.len(),
                        insight.cli_commands.len()
                    )
                }),
            },
        )
        .await
    }
}
