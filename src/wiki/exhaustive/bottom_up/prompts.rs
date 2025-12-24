//! Analysis Prompts
//!
//! Generates tier-aware prompts for file analysis.
//!
//! ## Design Principles
//!
//! 1. **Role + Objectives**: Clear AI role definition (from CodeWiki)
//! 2. **Tier-Aware**: Different depth/style per ProcessingTier
//! 3. **Child Context**: Important/Core files get pre-documented dependency context
//! 4. **Multi-Iteration**: Core files support deepening prompts
//! 5. **Focus Enforcement**: Prevent topic drift (from DeepWiki)
//! 6. **Bad Examples**: Explicit anti-patterns with examples (from DeepWiki)
//! 7. **Richness Guidance**: How to make documentation valuable

use super::graph_context::FileStructuralContext;
use super::types::{AnalysisRequest, ChildDocContext, ProcessingTier};
use crate::analyzer::parser::language::detect_language_or_text;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use serde_json::json;

/// Token budget for child context section
const MAX_CHILD_CONTEXT_TOKENS: usize = 2000;

/// Build analysis prompt based on request
pub fn build_analysis_prompt(
    request: &AnalysisRequest,
    file_content: &str,
    profile: &ProjectProfile,
    structural_context: Option<&FileStructuralContext>,
    max_chars: usize,
) -> String {
    let truncated_content = if file_content.len() > max_chars {
        format!("{}... [truncated]", &file_content[..max_chars])
    } else {
        file_content.to_string()
    };

    let language = detect_language_or_text(&request.file_path);

    let mut prompt = String::new();

    // Role and Objectives (CodeWiki pattern)
    prompt.push_str(&build_role_and_objectives(request.tier));

    // Project context
    prompt.push_str("# Project Context\n\n");
    prompt.push_str(&format!("**Project**: {}\n", profile.name));
    prompt.push_str(&format!("**Purpose**: {}\n", profile.purposes.join(", ")));

    if !profile.technical_traits.is_empty() {
        prompt.push_str(&format!(
            "**Tech Stack**: {}\n",
            profile.technical_traits.join(", ")
        ));
    }

    if !profile.terminology.is_empty() {
        prompt.push_str("\n**Key Terms**:\n");
        for term in profile.terminology.iter().take(5) {
            prompt.push_str(&format!("- **{}**: {}\n", term.term, term.definition));
        }
    }
    prompt.push('\n');

    // Structural facts from parser
    if let Some(ctx) = structural_context {
        prompt.push_str(&ctx.to_prompt_section());
    }

    // Child documentation context (for Important/Core tiers)
    if !request.child_contexts.is_empty() {
        prompt.push_str(&build_child_context_section(&request.child_contexts));
    }

    // Previous iteration context (for deepening)
    if let Some(ref prev) = request.previous_insight {
        prompt.push_str("# Previous Analysis\n\n");
        prompt.push_str(&format!("**Purpose identified**: {}\n\n", prev.purpose));
        prompt.push_str("**Initial documentation**:\n");
        prompt.push_str(&prev.content);
        prompt.push_str("\n\n---\n\n");
        prompt.push_str("Now DEEPEN this analysis. Focus on:\n");
        prompt.push_str("- Aspects not covered in the initial analysis\n");
        prompt.push_str("- Deeper architectural implications\n");
        prompt.push_str("- Cross-cutting concerns\n");
        prompt.push_str("- Integration patterns\n\n");
    }

    // File to analyze
    prompt.push_str(&format!("# File: `{}`\n\n", request.file_path));
    prompt.push_str(&format!("```{}\n", language));
    prompt.push_str(&truncated_content);
    prompt.push_str("\n```\n\n");

    // Focus enforcement (DeepWiki pattern)
    prompt.push_str(&build_focus_enforcement(&request.file_path));

    // Tier-specific instructions with richness guidance
    prompt.push_str(&build_tier_instructions(
        request.tier,
        request.is_deepening(),
    ));

    // Bad examples section (DeepWiki pattern)
    prompt.push_str(&build_bad_examples());

    prompt
}

/// Build role and objectives section (CodeWiki pattern)
fn build_role_and_objectives(tier: ProcessingTier) -> String {
    let tier_focus = match tier {
        ProcessingTier::Leaf => "utility and helper files",
        ProcessingTier::Standard => "standard codebase files",
        ProcessingTier::Important => "architecturally significant files",
        ProcessingTier::Core => "core architecture and entry point files",
    };

    format!(
        r#"<ROLE>
You are an expert code documentation assistant specializing in {tier_focus}.
Your task is to generate comprehensive, fact-based documentation that helps developers understand and safely modify this code.
</ROLE>

<OBJECTIVES>
Create documentation that:
1. Explains WHAT this file does and WHY it exists
2. Shows HOW it works with specific code references (file:line)
3. Reveals the DESIGN DECISIONS and their rationale
4. Identifies CRITICAL INVARIANTS and potential pitfalls
5. Connects this file to the broader system architecture
</OBJECTIVES>

"#,
        tier_focus = tier_focus
    )
}

/// Build focus enforcement section (DeepWiki pattern)
fn build_focus_enforcement(file_path: &str) -> String {
    format!(
        r#"<FOCUS>
IMPORTANT: Focus EXCLUSIVELY on this specific file: `{file_path}`
- Do NOT drift to general concepts or related topics
- Do NOT explain basic language syntax or common patterns
- Do NOT speculate about code you cannot see
- ONLY document facts observable in the provided code
- When referencing other files, LINK to them instead of explaining them
</FOCUS>

"#,
        file_path = file_path
    )
}

/// Build child context section with token budget
fn build_child_context_section(contexts: &[ChildDocContext]) -> String {
    let mut section = String::new();
    section.push_str("# Already Documented Dependencies\n\n");
    section
        .push_str("These files are already documented. LINK to them instead of duplicating:\n\n");

    let mut total_tokens = 0;
    for ctx in contexts {
        let tokens = ctx.estimated_tokens();
        if total_tokens + tokens > MAX_CHILD_CONTEXT_TOKENS {
            section.push_str(&format!("- `{}` - {}\n", ctx.path, ctx.purpose));
        } else {
            section.push_str(&format!("### `{}`\n", ctx.path));
            section.push_str(&format!("**Purpose**: {}\n", ctx.purpose));
            section.push_str(&format!("**Summary**: {}\n\n", ctx.summary));
            total_tokens += tokens;
        }
    }

    section.push('\n');
    section
}

/// Build tier-specific instructions with richness guidance
fn build_tier_instructions(tier: ProcessingTier, is_deepening: bool) -> String {
    let mut instructions = String::new();

    instructions.push_str("# Documentation Task\n\n");

    match tier {
        ProcessingTier::Leaf => {
            instructions.push_str(
                r#"Write **concise** documentation for this utility/helper file.

## Output Requirements

1. **Purpose**: 1 sentence - what this file provides
2. **Usage**: Brief explanation of key functions/types with example patterns
3. **No diagram required** for utilities

Keep it brief - this is a supporting file, not core architecture.
"#,
            );
        }
        ProcessingTier::Standard => {
            instructions.push_str(
                r#"Write documentation for this file.

## Output Requirements

1. **Purpose**: 1-2 sentences - what this file does and why it exists
2. **How It Works**: Key mechanisms with file:line references
3. **Diagram**: If this file has complex flow/state, include Mermaid diagram

Focus on what a developer needs to modify this code safely.

## Richness Guidelines

To make documentation valuable:
- Explain WHY the code is designed this way, not just WHAT it does
- Include usage scenarios: "When X happens, this code Y"
- Note potential pitfalls: "Be careful when... because..."
- Show the decision rationale: "This uses pattern X instead of Y because..."

## Output Formatting

- **Scope Opening**: Start with a clear scope statement
  Example: "This file handles X. For related functionality, see [other-file.rs]"
- **Sources**: End key sections with source references
  Example: "Sources: [file.rs:45-60]()"
- **Tables**: Use tables for comparison or feature matrices when comparing 3+ items
"#,
            );
        }
        ProcessingTier::Important => {
            instructions.push_str(
                r#"Write comprehensive documentation for this important file.

## Output Requirements

1. **Purpose**: What this file does and its architectural role
2. **How It Works**: Detailed walkthrough with natural headings
3. **Diagram**: Required - show architecture, data flow, or state transitions
4. **Integration**: How this file connects to others (LINK to documented dependencies)
5. **Critical Details**: Invariants, error handling, performance considerations

Reference already-documented dependencies instead of explaining them.

## Richness Guidelines

Make documentation genuinely useful:
- **Real Scenarios**: "When a user does X, this flow happens..."
- **Design Rationale**: "This architecture was chosen because..."
- **Invariants**: "This MUST be true or Y will break..."
- **Edge Cases**: "When X is empty/null, behavior is..."
- **Performance Notes**: "This operation is O(n) because..."

## Output Formatting

- **Scope Opening**: Start with scope and navigation hints
  Example: "This file manages X and Y. For database operations, see [storage/mod.rs]. For configuration, see [config.rs]."
- **Section Sources**: End each major section with line references
  Example: "Sources: [file.rs:100-150](), [types.rs:20-40]()"
- **Tables**: Use tables when comparing options, listing capabilities, or showing mappings
  Example: | Feature | Supported | Notes |
- **Navigation**: Include "For more on X, see [Section](#id)" patterns
"#,
            );
        }
        ProcessingTier::Core => {
            if is_deepening {
                instructions.push_str(
                    r#"DEEPEN the previous analysis of this core architecture file.

## Focus Areas for This Iteration

1. **Gaps**: What was missed in the initial analysis?
2. **Architecture**: Deeper patterns and design decisions
3. **Cross-Cutting**: Error handling, logging, configuration patterns
4. **Evolution**: How this file might change and what to preserve

Provide NEW insights that enrich the previous documentation. Do NOT repeat.

## Deepening Strategies

- Find subtle invariants that aren't obvious
- Trace error paths and recovery mechanisms
- Identify extension points and their constraints
- Document threading/concurrency implications
- Note configuration options and their effects
"#,
                );
            } else {
                instructions.push_str(
                    r#"Write comprehensive documentation for this **core architecture** file.

## Output Requirements

1. **Purpose**: Architectural role and why this file exists
2. **How It Works**: Complete walkthrough with multiple sections
3. **Diagram**: Required - architecture diagram showing key relationships
4. **Dependencies**: How this orchestrates other modules (LINK to them)
5. **Critical Details**: Everything a new developer must know

This is a core file - be thorough. Document what's not obvious from reading code.

## Richness Guidelines

Core documentation MUST include:
- **Architectural Context**: Where this fits in the system
- **Design Decisions**: Why this design over alternatives
- **Entry Points**: How external code invokes this
- **State Management**: What state is maintained and why
- **Error Handling Strategy**: How errors propagate
- **Performance Characteristics**: Time/space complexity of key operations
- **Security Considerations**: Trust boundaries, validation points
- **Extension Points**: How to add new features safely

## Output Formatting (CRITICAL for Core files)

- **Scope + Navigation Opening**: Start with scope definition and key navigation
  Example: "This file is the main orchestrator for X. It coordinates [module-a](module-a.md), [module-b](module-b.md), and [module-c](module-c.md). For configuration details, see [Configuration](#configuration)."
- **Section Sources**: REQUIRED - end each major section with source attribution
  Example: "Sources: [main.rs:50-120](), [types.rs:10-30]()"
- **Capability Tables**: Use tables for feature matrices, component responsibilities, or API endpoints
  Example:
  | Component | Responsibility | Key File |
  |-----------|---------------|----------|
  | Manager   | Lifecycle     | manager.rs |
- **Cross-References**: Use "[Section Name](#section-id)" for internal navigation
  Example: "For error handling details, see [Error Recovery](#error-recovery)"
- **Related Files Footer**: End with related files section
  Example: "**Related:** [config.rs]() [types.rs]() [error.rs]()"
"#,
                );
            }
        }
    }

    instructions
}

/// Build bad examples section (DeepWiki pattern)
fn build_bad_examples() -> String {
    "
## ANTI-PATTERNS (DO NOT)

<what_not_to_do>
WRONG - Starting with preamble:
- \"Here is the documentation for this file...\"
- \"This file contains the implementation of...\"
- \"Let me explain what this code does...\"

WRONG - Generic statements:
- \"This is a well-structured module\"
- \"The code follows best practices\"
- \"This file is important for the system\"

WRONG - Explaining language syntax (any language):
- \"The pub/public/export keyword makes this accessible\"
- \"This uses async/await for asynchronous operations\"
- \"The def/function keyword defines a function\"
- \"This class inherits from the base class\"
- \"The import statement brings in dependencies\"

WRONG - Empty/placeholder content:
- \"TODO: Document error handling\"
- \"[This section needs more detail]\"

WRONG - Repeating obvious information:
- \"The function process_data processes data\"
- \"The Config class is a configuration class\"
- \"This module handles module operations\"
</what_not_to_do>

<what_to_do>
CORRECT - Explain DESIGN intent, not syntax:
- \"Exposed as part of the public API contract for external integrations\"
- \"Asynchronous design enables handling N concurrent requests without blocking\"
- \"Centralizes validation logic to ensure consistency across all input sources\"

CORRECT - Design rationale:
- \"Uses leaf-first processing order so parent modules can reference already-documented children\"

CORRECT - Specific invariant:
- \"The session_id MUST be unique per run; reusing IDs corrupts the checkpoint store\"

CORRECT - Real scenario:
- \"When analyzing a 10k file project, this batches into groups of 50 to stay within token limits\"

CORRECT - Architectural significance:
- \"Acts as the single entry point for all database operations, enforcing transaction boundaries\"
</what_to_do>

"
    .to_string()
}

/// Schema for file documentation output
pub fn file_insight_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "Documentation output. NEVER include preambles or markdown fences.",
        "required": ["purpose", "importance", "content"],
        "additionalProperties": false,
        "properties": {
            "purpose": {
                "type": "string",
                "description": "1-2 sentences: what this file does and why. Direct, no preamble."
            },
            "importance": {
                "type": "string",
                "enum": ["critical", "high", "medium", "low"],
                "description": "critical=entry points/core, high=key logic, medium=standard, low=utilities"
            },
            "content": {
                "type": "string",
                "description": "Rich markdown with natural headings. Include file:line references. Start with scope statement. NO preambles. NO code fences wrapping."
            },
            "diagram": {
                "type": "string",
                "description": "Mermaid diagram code ONLY (no ```mermaid wrapper). Required for Important/Core files."
            },
            "source_sections": {
                "type": "array",
                "description": "Key source line ranges referenced in this documentation. Used for 'Relevant source files' section.",
                "items": {
                    "type": "object",
                    "required": ["file", "lines"],
                    "additionalProperties": false,
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "File path (relative)"
                        },
                        "lines": {
                            "type": "string",
                            "description": "Line range, e.g., '45-120' or '10-30, 50-80'"
                        },
                        "description": {
                            "type": "string",
                            "description": "Brief description of what this section covers"
                        }
                    }
                }
            },
            "related_files": {
                "type": "array",
                "description": "Files this code interacts with. LINK instead of duplicating.",
                "items": {
                    "type": "object",
                    "required": ["path", "relationship"],
                    "additionalProperties": false,
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to related file"
                        },
                        "relationship": {
                            "type": "string",
                            "description": "Type: imports, exports, calls, implements, configures, extends"
                        }
                    }
                }
            }
        }
    })
}

/// Schema for diagram regeneration
pub fn diagram_fix_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["diagram"],
        "properties": {
            "diagram": {
                "type": "string",
                "description": "Corrected Mermaid diagram code. NO ```mermaid wrapper."
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_instructions_differ() {
        let leaf = build_tier_instructions(ProcessingTier::Leaf, false);
        let core = build_tier_instructions(ProcessingTier::Core, false);
        let core_deep = build_tier_instructions(ProcessingTier::Core, true);

        assert!(leaf.contains("concise"));
        assert!(core.contains("comprehensive"));
        assert!(core_deep.contains("DEEPEN"));
    }

    #[test]
    fn test_child_context_section() {
        let contexts = vec![ChildDocContext {
            path: "src/utils/helper.rs".to_string(),
            purpose: "Utility functions".to_string(),
            importance: crate::wiki::exhaustive::types::Importance::Low,
            summary: "Provides string helpers.".to_string(),
        }];

        let section = build_child_context_section(&contexts);
        assert!(section.contains("Already Documented"));
        assert!(section.contains("src/utils/helper.rs"));
        assert!(section.contains("LINK"));
    }

    #[test]
    fn test_schema_required_fields() {
        let schema = file_insight_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("purpose")));
        assert!(required.contains(&json!("importance")));
        assert!(required.contains(&json!("content")));
    }
}
