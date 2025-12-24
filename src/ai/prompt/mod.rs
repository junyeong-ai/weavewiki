//! Prompt Builder System
//!
//! Standardized prompt construction for LLM interactions.
//! Provides consistent structure across all pipeline prompts.
//!
//! ## Design Principles
//!
//! 1. **Role Definition**: Clear AI role for each task
//! 2. **Structured Objectives**: Numbered goals
//! 3. **Context Sections**: Organized input data
//! 4. **Focus Enforcement**: Prevent topic drift
//! 5. **Anti-Patterns**: Explicit bad examples
//! 6. **Output Schema**: JSON structure definition

use std::collections::HashMap;

/// Prompt section types
#[derive(Debug, Clone)]
pub enum PromptSection {
    /// Role definition with expertise area
    Role { expertise: String, task: String },
    /// Numbered objectives
    Objectives(Vec<String>),
    /// Context with key-value pairs
    Context(HashMap<String, String>),
    /// Raw text section with optional header
    Text {
        header: Option<String>,
        content: String,
    },
    /// Code block with language
    Code { language: String, content: String },
    /// Focus enforcement with restrictions
    Focus {
        target: String,
        restrictions: Vec<String>,
    },
    /// Anti-patterns with good/bad examples
    AntiPatterns { bad: Vec<String>, good: Vec<String> },
    /// Custom section
    Custom(String),
}

/// Prompt builder for consistent prompt construction
#[derive(Debug, Clone, Default)]
pub struct PromptBuilder {
    sections: Vec<PromptSection>,
}

impl PromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a role definition section
    pub fn role(mut self, expertise: &str, task: &str) -> Self {
        self.sections.push(PromptSection::Role {
            expertise: expertise.to_string(),
            task: task.to_string(),
        });
        self
    }

    /// Add objectives section
    pub fn objectives(mut self, objectives: Vec<&str>) -> Self {
        self.sections.push(PromptSection::Objectives(
            objectives.into_iter().map(String::from).collect(),
        ));
        self
    }

    /// Add context section
    pub fn context(mut self, context: HashMap<String, String>) -> Self {
        self.sections.push(PromptSection::Context(context));
        self
    }

    /// Add a context item (convenience method)
    pub fn context_item(mut self, key: &str, value: &str) -> Self {
        // Find existing context or create new
        let mut found = false;
        for section in &mut self.sections {
            if let PromptSection::Context(ctx) = section {
                ctx.insert(key.to_string(), value.to_string());
                found = true;
                break;
            }
        }
        if !found {
            let mut ctx = HashMap::new();
            ctx.insert(key.to_string(), value.to_string());
            self.sections.push(PromptSection::Context(ctx));
        }
        self
    }

    /// Add text section
    pub fn text(mut self, content: &str) -> Self {
        self.sections.push(PromptSection::Text {
            header: None,
            content: content.to_string(),
        });
        self
    }

    /// Add text section with header
    pub fn section(mut self, header: &str, content: &str) -> Self {
        self.sections.push(PromptSection::Text {
            header: Some(header.to_string()),
            content: content.to_string(),
        });
        self
    }

    /// Add code block
    pub fn code(mut self, language: &str, content: &str) -> Self {
        self.sections.push(PromptSection::Code {
            language: language.to_string(),
            content: content.to_string(),
        });
        self
    }

    /// Add focus enforcement section
    pub fn focus(mut self, target: &str, restrictions: Vec<&str>) -> Self {
        self.sections.push(PromptSection::Focus {
            target: target.to_string(),
            restrictions: restrictions.into_iter().map(String::from).collect(),
        });
        self
    }

    /// Add anti-patterns section
    pub fn anti_patterns(mut self, bad: Vec<&str>, good: Vec<&str>) -> Self {
        self.sections.push(PromptSection::AntiPatterns {
            bad: bad.into_iter().map(String::from).collect(),
            good: good.into_iter().map(String::from).collect(),
        });
        self
    }

    /// Add custom section
    pub fn custom(mut self, content: &str) -> Self {
        self.sections
            .push(PromptSection::Custom(content.to_string()));
        self
    }

    /// Build the final prompt string
    pub fn build(self) -> String {
        let mut prompt = String::new();

        for section in self.sections {
            match section {
                PromptSection::Role { expertise, task } => {
                    prompt.push_str("<ROLE>\n");
                    prompt.push_str(&format!(
                        "You are an expert {} specializing in {}.\n",
                        expertise, task
                    ));
                    prompt.push_str("</ROLE>\n\n");
                }
                PromptSection::Objectives(objectives) => {
                    prompt.push_str("<OBJECTIVES>\n");
                    for (i, obj) in objectives.iter().enumerate() {
                        prompt.push_str(&format!("{}. {}\n", i + 1, obj));
                    }
                    prompt.push_str("</OBJECTIVES>\n\n");
                }
                PromptSection::Context(ctx) => {
                    prompt.push_str("# Context\n\n");
                    for (key, value) in ctx {
                        prompt.push_str(&format!("**{}**: {}\n", key, value));
                    }
                    prompt.push('\n');
                }
                PromptSection::Text { header, content } => {
                    if let Some(h) = header {
                        prompt.push_str(&format!("# {}\n\n", h));
                    }
                    prompt.push_str(&content);
                    prompt.push_str("\n\n");
                }
                PromptSection::Code { language, content } => {
                    prompt.push_str(&format!("```{}\n", language));
                    prompt.push_str(&content);
                    prompt.push_str("\n```\n\n");
                }
                PromptSection::Focus {
                    target,
                    restrictions,
                } => {
                    prompt.push_str("<FOCUS>\n");
                    prompt.push_str(&format!("IMPORTANT: Focus EXCLUSIVELY on: {}\n", target));
                    for restriction in restrictions {
                        prompt.push_str(&format!("- {}\n", restriction));
                    }
                    prompt.push_str("</FOCUS>\n\n");
                }
                PromptSection::AntiPatterns { bad, good } => {
                    prompt.push_str("## ANTI-PATTERNS\n\n");
                    prompt.push_str("<what_not_to_do>\n");
                    for example in bad {
                        prompt.push_str(&format!("WRONG: {}\n", example));
                    }
                    prompt.push_str("</what_not_to_do>\n\n");
                    prompt.push_str("<what_to_do>\n");
                    for example in good {
                        prompt.push_str(&format!("CORRECT: {}\n", example));
                    }
                    prompt.push_str("</what_to_do>\n\n");
                }
                PromptSection::Custom(content) => {
                    prompt.push_str(&content);
                    prompt.push_str("\n\n");
                }
            }
        }

        prompt.trim_end().to_string()
    }
}

/// Preset prompt templates for common use cases
pub struct PromptTemplates;

impl PromptTemplates {
    /// Template for file analysis prompts
    pub fn file_analysis(file_path: &str, tier: &str) -> PromptBuilder {
        PromptBuilder::new()
            .role(
                "code documentation assistant",
                &format!("documenting {} files", tier),
            )
            .objectives(vec![
                "Explain WHAT this file does and WHY it exists",
                "Show HOW it works with specific code references (file:line)",
                "Reveal DESIGN DECISIONS and their rationale",
                "Identify CRITICAL INVARIANTS and potential pitfalls",
                "Connect this file to the broader system architecture",
            ])
            .focus(
                file_path,
                vec![
                    "Do NOT drift to general concepts or related topics",
                    "Do NOT explain basic language syntax or common patterns",
                    "Do NOT speculate about code you cannot see",
                    "ONLY document facts observable in the provided code",
                ],
            )
    }

    /// Template for project characterization prompts
    pub fn characterization(agent_type: &str) -> PromptBuilder {
        PromptBuilder::new()
            .role(
                "software architecture analyst",
                &format!("{} analysis", agent_type),
            )
            .objectives(vec![
                "Analyze the provided codebase structure",
                "Identify key patterns and characteristics",
                "Provide actionable insights for documentation",
            ])
    }

    /// Template for domain synthesis prompts
    pub fn domain_synthesis(domain_name: &str) -> PromptBuilder {
        PromptBuilder::new()
            .role("technical writer", "synthesizing domain documentation")
            .objectives(vec![
                &format!(
                    "Create cohesive documentation for the {} domain",
                    domain_name
                ),
                "Synthesize insights from individual file documentation",
                "Eliminate redundancy while preserving unique insights",
                "Create a narrative that flows logically",
            ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_prompt() {
        let prompt = PromptBuilder::new()
            .role("code analyst", "Rust documentation")
            .objectives(vec!["Analyze code", "Generate docs"])
            .build();

        assert!(prompt.contains("<ROLE>"));
        assert!(prompt.contains("code analyst"));
        assert!(prompt.contains("<OBJECTIVES>"));
        assert!(prompt.contains("1. Analyze code"));
        assert!(prompt.contains("2. Generate docs"));
    }

    #[test]
    fn test_focus_section() {
        let prompt = PromptBuilder::new()
            .focus("src/main.rs", vec!["Do NOT speculate", "Stay focused"])
            .build();

        assert!(prompt.contains("<FOCUS>"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("Do NOT speculate"));
    }

    #[test]
    fn test_anti_patterns() {
        let prompt = PromptBuilder::new()
            .anti_patterns(
                vec!["Generic statements", "Obvious comments"],
                vec!["Specific insights", "Design rationale"],
            )
            .build();

        assert!(prompt.contains("WRONG: Generic statements"));
        assert!(prompt.contains("CORRECT: Specific insights"));
    }

    #[test]
    fn test_context_items() {
        let prompt = PromptBuilder::new()
            .context_item("Project", "WeaveWiki")
            .context_item("Language", "Rust")
            .build();

        assert!(prompt.contains("**Project**: WeaveWiki"));
        assert!(prompt.contains("**Language**: Rust"));
    }

    #[test]
    fn test_template() {
        let prompt = PromptTemplates::file_analysis("src/main.rs", "core")
            .code("rust", "fn main() {}")
            .build();

        assert!(prompt.contains("core files"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("```rust"));
    }
}
