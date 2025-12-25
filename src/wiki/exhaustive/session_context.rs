//! Session-level context for token optimization
//!
//! Project context is included once per session, not per-file.
//! This avoids repeating 300-400 tokens of project context in every file prompt.
//!
//! ## Token Savings
//!
//! Without SessionContext: 300-400 tokens × N files = 30K-40K tokens for 100 files
//! With SessionContext: 300-400 tokens × 1 = 300-400 tokens total
//!
//! For a 1000-file project, this saves ~400K tokens.

use crate::wiki::exhaustive::characterization::profile::ProjectProfile;

/// Session context that persists across file analyses
///
/// This avoids repeating 300-400 tokens of project context per file.
/// The system prompt includes this context once, and all subsequent
/// file-level prompts omit it.
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// Compact project context for system prompt
    pub system_context: String,

    /// Key terminology (top 10 terms)
    pub key_terms: Vec<(String, String)>,

    /// Technical stack summary
    pub tech_stack: String,
}

impl SessionContext {
    /// Build session context from project profile
    ///
    /// This is called once per session (not per file).
    pub fn from_profile(profile: &ProjectProfile) -> Self {
        let system_context = Self::build_system_context(profile);

        let key_terms: Vec<(String, String)> = profile
            .terminology
            .iter()
            .take(10)
            .map(|t| (t.term.clone(), t.definition.clone()))
            .collect();

        let tech_stack = profile.technical_traits.join(", ");

        Self {
            system_context,
            key_terms,
            tech_stack,
        }
    }

    /// Build compact system context
    ///
    /// This creates a minimal project summary suitable for system prompts.
    fn build_system_context(profile: &ProjectProfile) -> String {
        let mut ctx = String::new();

        ctx.push_str(&format!("# Project: {}\n", profile.name));
        ctx.push_str(&format!("Purpose: {}\n", profile.purposes.join("; ")));
        ctx.push_str(&format!("Tech: {}\n", profile.technical_traits.join(", ")));

        if !profile.domain_traits.is_empty() {
            ctx.push_str(&format!("Domain: {}\n", profile.domain_traits.join(", ")));
        }

        ctx
    }

    /// Token estimate for this context
    ///
    /// Uses rough approximation: 1 token ≈ 4 characters
    pub fn estimated_tokens(&self) -> usize {
        // System context tokens
        let system_tokens = self.system_context.len() / 4;

        // Key terms tokens
        let terms_tokens: usize = self
            .key_terms
            .iter()
            .map(|(t, d)| (t.len() + d.len()) / 4)
            .sum();

        system_tokens + terms_tokens
    }

    /// Get full context string for inclusion in prompts (legacy support)
    ///
    /// When SessionContext is not used, this generates the full project context
    /// that would normally be included in each file prompt.
    pub fn full_context_string(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.system_context);

        if !self.key_terms.is_empty() {
            s.push_str("\n**Key Terms**:\n");
            for (term, def) in &self.key_terms {
                s.push_str(&format!("- **{}**: {}\n", term, def));
            }
        }

        s
    }
}

/// Tier-specific anti-patterns (simplified for Leaf/Standard)
///
/// Lower tiers get minimal anti-patterns to reduce prompt bloat.
/// Higher tiers get comprehensive anti-patterns for quality control.
pub struct TierAntiPatterns;

impl TierAntiPatterns {
    /// Get anti-patterns for tier (simplified for lower tiers)
    pub fn for_tier(tier: &str) -> &'static str {
        match tier {
            "leaf" | "standard" => {
                // Minimal anti-patterns for simple files
                r#"## ANTI-PATTERNS (DO NOT)

WRONG - Preamble:
- "Here is the documentation for this file..."
- "This file contains..."

WRONG - Syntax explanation:
- "The pub keyword makes this accessible"
- "This uses async/await"

CORRECT - Design intent:
- "Exposed as part of public API contract"
- "Async enables N concurrent requests"
"#
            }
            "important" | "core" => {
                // Full anti-patterns for complex files
                r#"## ANTI-PATTERNS (DO NOT)

<what_not_to_do>
WRONG - Starting with preamble:
- "Here is the documentation for this file..."
- "This file contains the implementation of..."
- "Let me explain what this code does..."

WRONG - Generic statements:
- "This is a well-structured module"
- "The code follows best practices"
- "This file is important for the system"

WRONG - Explaining language syntax (any language):
- "The pub/public/export keyword makes this accessible"
- "This uses async/await for asynchronous operations"
- "The def/function keyword defines a function"
- "This class inherits from the base class"
- "The import statement brings in dependencies"

WRONG - Empty/placeholder content:
- "TODO: Document error handling"
- "[This section needs more detail]"

WRONG - Repeating obvious information:
- "The function process_data processes data"
- "The Config class is a configuration class"
- "This module handles module operations"
</what_not_to_do>

<what_to_do>
CORRECT - Explain DESIGN intent, not syntax:
- "Exposed as part of the public API contract for external integrations"
- "Asynchronous design enables handling N concurrent requests without blocking"
- "Centralizes validation logic to ensure consistency across all input sources"

CORRECT - Design rationale:
- "Uses leaf-first processing order so parent modules can reference already-documented children"

CORRECT - Specific invariant:
- "The session_id MUST be unique per run; reusing IDs corrupts the checkpoint store"

CORRECT - Real scenario:
- "When analyzing a 10k file project, this batches into groups of 50 to stay within token limits"

CORRECT - Architectural significance:
- "Acts as the single entry point for all database operations, enforcing transaction boundaries"
</what_to_do>
"#
            }
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnalysisMode, ProjectScale};
    use crate::types::DomainTerm;

    #[test]
    fn test_session_context_from_profile() {
        let mut profile = ProjectProfile::new(
            "TestProject".to_string(),
            ProjectScale::Medium,
            AnalysisMode::Standard,
        );
        profile.purposes = vec!["CLI Tool".to_string()];
        profile.technical_traits = vec!["Rust".to_string(), "Async".to_string()];
        profile.domain_traits = vec!["Code Analysis".to_string()];
        profile.terminology = vec![
            DomainTerm {
                term: "Parser".to_string(),
                definition: "Code syntax analyzer".to_string(),
                context: None,
            },
            DomainTerm {
                term: "AST".to_string(),
                definition: "Abstract Syntax Tree".to_string(),
                context: None,
            },
        ];

        let ctx = SessionContext::from_profile(&profile);

        assert!(ctx.system_context.contains("TestProject"));
        assert!(ctx.system_context.contains("CLI Tool"));
        assert!(ctx.system_context.contains("Rust, Async"));
        assert!(ctx.system_context.contains("Code Analysis"));
        assert_eq!(ctx.key_terms.len(), 2);
        assert_eq!(ctx.tech_stack, "Rust, Async");
    }

    #[test]
    fn test_session_context_token_estimate() {
        let mut profile =
            ProjectProfile::new("Test".to_string(), ProjectScale::Small, AnalysisMode::Fast);
        profile.purposes = vec!["Test".to_string()];
        profile.technical_traits = vec!["Rust".to_string()];

        let ctx = SessionContext::from_profile(&profile);
        let tokens = ctx.estimated_tokens();

        // Should be a reasonable estimate (rough: 1 token per 4 chars)
        assert!(tokens > 0);
        assert!(tokens < 200); // Should be much less than typical per-file context
    }

    #[test]
    fn test_tier_anti_patterns() {
        let leaf = TierAntiPatterns::for_tier("leaf");
        let core = TierAntiPatterns::for_tier("core");

        assert!(leaf.contains("ANTI-PATTERNS"));
        assert!(core.contains("ANTI-PATTERNS"));

        // Core should have more detailed anti-patterns
        assert!(core.len() > leaf.len());

        // Core should have <what_not_to_do> tags
        assert!(core.contains("<what_not_to_do>"));
        assert!(!leaf.contains("<what_not_to_do>"));
    }

    #[test]
    fn test_key_terms_limit() {
        let mut profile =
            ProjectProfile::new("Test".to_string(), ProjectScale::Small, AnalysisMode::Fast);
        profile.purposes = vec!["Test".to_string()];

        // Add 15 terms
        for i in 0..15 {
            profile.terminology.push(DomainTerm {
                term: format!("Term{}", i),
                definition: format!("Definition{}", i),
                context: None,
            });
        }

        let ctx = SessionContext::from_profile(&profile);

        // Should only keep top 10
        assert_eq!(ctx.key_terms.len(), 10);
    }

    #[test]
    fn test_full_context_string() {
        let mut profile = ProjectProfile::new(
            "TestProject".to_string(),
            ProjectScale::Medium,
            AnalysisMode::Standard,
        );
        profile.purposes = vec!["CLI Tool".to_string()];
        profile.technical_traits = vec!["Rust".to_string()];
        profile.terminology = vec![DomainTerm {
            term: "Parser".to_string(),
            definition: "Code analyzer".to_string(),
            context: None,
        }];

        let ctx = SessionContext::from_profile(&profile);
        let full = ctx.full_context_string();

        assert!(full.contains("TestProject"));
        assert!(full.contains("CLI Tool"));
        assert!(full.contains("**Key Terms**:"));
        assert!(full.contains("**Parser**: Code analyzer"));
    }
}
