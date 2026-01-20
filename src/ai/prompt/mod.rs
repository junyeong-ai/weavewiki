//! Prompt Construction System
//!
//! Simple, composable prompt building for LLM interactions.

/// Builds prompts using plain markdown structure.
/// Designed for modern LLMs that understand markdown well.
#[derive(Debug, Clone, Default)]
pub struct PromptBuilder {
    parts: Vec<String>,
}

impl PromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a role/context introduction
    pub fn role(mut self, role: &str) -> Self {
        self.parts.push(format!("You are {}.\n", role));
        self
    }

    /// Add a section with header
    pub fn section(mut self, header: &str, content: &str) -> Self {
        self.parts.push(format!("## {}\n{}\n", header, content));
        self
    }

    /// Add content without header
    pub fn content(mut self, text: &str) -> Self {
        self.parts.push(format!("{}\n", text));
        self
    }

    /// Add a code block
    pub fn code(mut self, language: &str, code: &str) -> Self {
        self.parts.push(format!("```{}\n{}\n```\n", language, code));
        self
    }

    /// Add a list of items
    pub fn list(mut self, header: &str, items: &[&str]) -> Self {
        let items_str = items
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n");
        self.parts.push(format!("## {}\n{}\n", header, items_str));
        self
    }

    /// Add numbered steps
    pub fn steps(mut self, header: &str, steps: &[&str]) -> Self {
        let steps_str = steps
            .iter()
            .enumerate()
            .map(|(i, step)| format!("{}. {}", i + 1, step))
            .collect::<Vec<_>>()
            .join("\n");
        self.parts.push(format!("## {}\n{}\n", header, steps_str));
        self
    }

    /// Add critical requirements (highlighted)
    pub fn critical(mut self, items: &[&str]) -> Self {
        let items_str = items
            .iter()
            .map(|item| format!("- **{}**", item))
            .collect::<Vec<_>>()
            .join("\n");
        self.parts
            .push(format!("## CRITICAL REQUIREMENTS\n{}\n", items_str));
        self
    }

    /// Build the final prompt
    pub fn build(self) -> String {
        self.parts.join("\n").trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_prompt() {
        let prompt = PromptBuilder::new()
            .role("a code analyst")
            .section("Task", "Analyze the code.")
            .build();

        assert!(prompt.contains("You are a code analyst"));
        assert!(prompt.contains("## Task"));
    }

    #[test]
    fn test_steps() {
        let prompt = PromptBuilder::new()
            .steps("Process", &["First", "Second", "Third"])
            .build();

        assert!(prompt.contains("1. First"));
        assert!(prompt.contains("2. Second"));
        assert!(prompt.contains("3. Third"));
    }
}
