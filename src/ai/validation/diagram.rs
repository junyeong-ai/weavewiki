//! Mermaid Diagram Validation
//!
//! Validates Mermaid diagram syntax to ensure diagrams are renderable.
//! Follows CodeWiki's strict validation approach: fail loudly on invalid diagrams.
//!
//! ## Supported Diagram Types
//! - flowchart/graph (TD, LR, RL, BT)
//! - sequenceDiagram
//! - classDiagram
//! - stateDiagram
//! - erDiagram
//! - gantt
//! - pie
//!
//! ## Validation Strategy
//! 1. Detect diagram type from first line
//! 2. Validate basic syntax for that type
//! 3. Check for common errors (unclosed brackets, invalid arrows)
//! 4. Return detailed error with line numbers

use std::collections::HashSet;

/// Result of Mermaid diagram validation
#[derive(Debug, Clone)]
pub struct DiagramValidation {
    /// Whether the diagram is valid
    pub is_valid: bool,
    /// Detected diagram type (or "unknown")
    pub diagram_type: String,
    /// Validation errors with line numbers
    pub errors: Vec<DiagramError>,
    /// Warnings (valid but potentially problematic)
    pub warnings: Vec<DiagramWarning>,
}

impl DiagramValidation {
    pub fn valid(diagram_type: impl Into<String>) -> Self {
        Self {
            is_valid: true,
            diagram_type: diagram_type.into(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn invalid(diagram_type: impl Into<String>, errors: Vec<DiagramError>) -> Self {
        Self {
            is_valid: false,
            diagram_type: diagram_type.into(),
            errors,
            warnings: Vec::new(),
        }
    }

    pub fn with_warnings(mut self, warnings: Vec<DiagramWarning>) -> Self {
        self.warnings = warnings;
        self
    }
}

/// Diagram validation error
#[derive(Debug, Clone)]
pub struct DiagramError {
    pub line: usize,
    pub message: String,
    pub suggestion: Option<String>,
}

impl DiagramError {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl std::fmt::Display for DiagramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)?;
        if let Some(ref suggestion) = self.suggestion {
            write!(f, " ({})", suggestion)?;
        }
        Ok(())
    }
}

/// Diagram warning (valid but potentially problematic)
#[derive(Debug, Clone)]
pub struct DiagramWarning {
    pub line: usize,
    pub message: String,
}

/// Mermaid diagram validator
pub struct DiagramValidator;

impl Default for DiagramValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramValidator {
    pub fn new() -> Self {
        Self
    }

    /// Validate a Mermaid diagram
    ///
    /// Strips ```mermaid wrappers if present.
    /// Returns detailed validation result with line-accurate errors.
    pub fn validate(&self, content: &str) -> DiagramValidation {
        // Strip mermaid code fence if present
        let content = self.strip_code_fence(content);
        let content = content.trim();

        if content.is_empty() {
            return DiagramValidation::invalid(
                "empty",
                vec![DiagramError::new(1, "Empty diagram")],
            );
        }

        // Detect diagram type from first line
        let first_line = content.lines().next().unwrap_or("").trim().to_lowercase();
        let diagram_type = self.detect_diagram_type(&first_line);

        // Validate based on type
        match diagram_type.as_str() {
            "flowchart" | "graph" => self.validate_flowchart(content),
            "sequenceDiagram" | "sequence" => self.validate_sequence(content),
            "classDiagram" | "class" => self.validate_class(content),
            "stateDiagram" | "state" => self.validate_state(content),
            "erDiagram" | "er" => self.validate_er(content),
            "gantt" => self.validate_gantt(content),
            "pie" => self.validate_pie(content),
            _ => {
                // Unknown type - do basic validation
                self.validate_generic(content)
            }
        }
    }

    /// Strip ```mermaid wrapper if present
    fn strip_code_fence(&self, content: &str) -> String {
        let content = content.trim();

        // Check for opening fence
        if content.starts_with("```mermaid") || content.starts_with("```Mermaid") {
            let content = content
                .strip_prefix("```mermaid")
                .or_else(|| content.strip_prefix("```Mermaid"))
                .unwrap_or(content);

            // Check for closing fence
            if content.trim().ends_with("```") {
                let content = content.trim();
                return content[..content.len() - 3].trim().to_string();
            }
            return content.trim().to_string();
        }

        content.to_string()
    }

    /// Detect diagram type from first line
    fn detect_diagram_type(&self, first_line: &str) -> String {
        let line = first_line.to_lowercase();

        if line.starts_with("flowchart") || line.starts_with("graph") {
            "flowchart".to_string()
        } else if line.starts_with("sequencediagram") {
            "sequenceDiagram".to_string()
        } else if line.starts_with("classdiagram") {
            "classDiagram".to_string()
        } else if line.starts_with("statediagram") {
            "stateDiagram".to_string()
        } else if line.starts_with("erdiagram") {
            "erDiagram".to_string()
        } else if line.starts_with("gantt") {
            "gantt".to_string()
        } else if line.starts_with("pie") {
            "pie".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Validate flowchart/graph diagram
    fn validate_flowchart(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut node_ids: HashSet<String> = HashSet::new();
        let mut referenced_ids: HashSet<String> = HashSet::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("%%") {
                continue;
            }

            // Skip first line (diagram type declaration)
            if idx == 0 {
                continue;
            }

            // Check for unclosed brackets
            let open_brackets = line.matches('[').count() + line.matches('(').count();
            let close_brackets = line.matches(']').count() + line.matches(')').count();
            if open_brackets != close_brackets {
                errors.push(
                    DiagramError::new(line_num, "Unclosed bracket")
                        .with_suggestion("Check matching [] or ()"),
                );
            }

            // Check for unclosed quotes
            let quotes = line.matches('"').count();
            if quotes % 2 != 0 {
                errors.push(
                    DiagramError::new(line_num, "Unclosed quote")
                        .with_suggestion("Check matching \"\""),
                );
            }

            // Extract node IDs from connections
            // Pattern: A --> B, A --- B, A -.- B, etc.
            if line.contains("-->")
                || line.contains("---")
                || line.contains("-.-")
                || line.contains("==>")
            {
                let parts: Vec<&str> = line
                    .split(['-', '>', '=', '.'])
                    .filter(|s| !s.is_empty())
                    .collect();

                for part in parts {
                    let id = part
                        .trim()
                        .split(['[', '(', '{'])
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        referenced_ids.insert(id.to_string());
                    }
                }
            }

            // Extract node definitions
            // Pattern: A[Label], B(Label), C{Label}
            if line.contains('[') || line.contains('(') || line.contains('{') {
                let id = line
                    .split(['[', '(', '{'])
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .last()
                    .unwrap_or("");
                if !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    node_ids.insert(id.to_string());
                }
            }
        }

        // Check for undefined node references (warning only, not error)
        for ref_id in &referenced_ids {
            if !node_ids.contains(ref_id) {
                warnings.push(DiagramWarning {
                    line: 0,
                    message: format!("Node '{}' referenced but not explicitly defined", ref_id),
                });
            }
        }

        if errors.is_empty() {
            DiagramValidation::valid("flowchart").with_warnings(warnings)
        } else {
            DiagramValidation::invalid("flowchart", errors).with_warnings(warnings)
        }
    }

    /// Validate sequence diagram
    fn validate_sequence(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut participants: HashSet<String> = HashSet::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            // Check participant definitions
            if line.starts_with("participant") || line.starts_with("actor") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    participants.insert(parts[1].to_string());
                }
            }

            // Check arrow syntax
            if line.contains("->>") || line.contains("-->>") || line.contains("-x") {
                // Valid arrow patterns
                if !line.contains(':') {
                    errors.push(
                        DiagramError::new(line_num, "Message arrow missing ':' and label")
                            .with_suggestion("Format: A->>B: Message"),
                    );
                }
            }

            // Check for unclosed notes
            if line.starts_with("note") && !line.contains(':') {
                // Multi-line note - check if closed
                let remaining: String =
                    content.lines().skip(idx + 1).collect::<Vec<_>>().join("\n");
                if !remaining.contains("end note") {
                    errors.push(
                        DiagramError::new(line_num, "Unclosed note block")
                            .with_suggestion("Add 'end note' to close"),
                    );
                }
            }
        }

        if errors.is_empty() {
            DiagramValidation::valid("sequenceDiagram")
        } else {
            DiagramValidation::invalid("sequenceDiagram", errors)
        }
    }

    /// Validate class diagram
    fn validate_class(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            // Check class definitions
            if line.starts_with("class ") {
                // Should have class name
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 2 {
                    errors.push(DiagramError::new(line_num, "Class definition missing name"));
                }
            }

            // Check relationship syntax
            if line.contains("<|--")
                || line.contains("*--")
                || line.contains("o--")
                || line.contains("--")
            {
                // Valid relationship - check format
                if line.contains("::") && line.matches("::").count() > 2 {
                    errors.push(
                        DiagramError::new(line_num, "Too many :: separators")
                            .with_suggestion("Check class method syntax"),
                    );
                }
            }
        }

        if errors.is_empty() {
            DiagramValidation::valid("classDiagram")
        } else {
            DiagramValidation::invalid("classDiagram", errors)
        }
    }

    /// Validate state diagram
    fn validate_state(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut open_states = 0;

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            // Track state blocks
            if line.starts_with("state ") && line.ends_with('{') {
                open_states += 1;
            }
            if line == "}" {
                if open_states > 0 {
                    open_states -= 1;
                } else {
                    errors.push(DiagramError::new(line_num, "Unexpected closing brace"));
                }
            }

            // Check transition syntax
            if line.contains("-->") {
                // Valid transition
                if line.starts_with("[*]") || line.ends_with("[*]") {
                    // Start/end state - valid
                } else if !line
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic())
                    .unwrap_or(false)
                {
                    errors.push(
                        DiagramError::new(line_num, "State name should start with letter")
                            .with_suggestion("Use alphabetic state names"),
                    );
                }
            }
        }

        if open_states > 0 {
            errors.push(
                DiagramError::new(0, format!("{} unclosed state blocks", open_states))
                    .with_suggestion("Add closing braces"),
            );
        }

        if errors.is_empty() {
            DiagramValidation::valid("stateDiagram")
        } else {
            DiagramValidation::invalid("stateDiagram", errors)
        }
    }

    /// Validate ER diagram
    fn validate_er(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            // Check relationship syntax
            if line.contains("||--") || line.contains("}o--") || line.contains("}|--") {
                // Valid relationship pattern
                if !line.contains(':') {
                    errors.push(
                        DiagramError::new(line_num, "ER relationship missing ':' label")
                            .with_suggestion("Format: ENTITY1 ||--o{ ENTITY2 : label"),
                    );
                }
            }

            // Check entity definitions
            if line.contains('{') && !line.contains("||") && !line.contains("}|") {
                // Entity block - should have matching braces
                let remaining: String =
                    content.lines().skip(idx + 1).collect::<Vec<_>>().join("\n");
                if !remaining.contains('}') {
                    errors.push(DiagramError::new(line_num, "Unclosed entity block"));
                }
            }
        }

        if errors.is_empty() {
            DiagramValidation::valid("erDiagram")
        } else {
            DiagramValidation::invalid("erDiagram", errors)
        }
    }

    /// Validate gantt chart
    fn validate_gantt(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut has_title = false;
        let mut has_section = false;

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            if line.starts_with("title") {
                has_title = true;
            }

            if line.starts_with("section") {
                has_section = true;
            }

            // Check task format
            if !line.starts_with("title")
                && !line.starts_with("section")
                && !line.starts_with("dateFormat")
                && !line.starts_with("excludes")
                && !line.starts_with("axisFormat")
            {
                // Should be a task - check format
                if !line.contains(':') {
                    errors.push(
                        DiagramError::new(line_num, "Task missing ':' separator")
                            .with_suggestion("Format: Task name : status, date, duration"),
                    );
                }
            }
        }

        if !has_title {
            errors.push(
                DiagramError::new(0, "Gantt chart missing 'title'")
                    .with_suggestion("Add 'title Chart Title'"),
            );
        }

        if errors.is_empty() {
            let mut validation = DiagramValidation::valid("gantt");
            if !has_section {
                validation.warnings.push(DiagramWarning {
                    line: 0,
                    message: "Gantt chart has no sections".to_string(),
                });
            }
            validation
        } else {
            DiagramValidation::invalid("gantt", errors)
        }
    }

    /// Validate pie chart
    fn validate_pie(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut has_data = false;

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("%%") || idx == 0 {
                continue;
            }

            if line.starts_with("title") || line.starts_with("showData") {
                continue;
            }

            // Check pie slice format
            if line.starts_with('"') {
                has_data = true;
                // Format: "Label" : value
                if !line.contains(':') {
                    errors.push(
                        DiagramError::new(line_num, "Pie slice missing ':' and value")
                            .with_suggestion("Format: \"Label\" : 42"),
                    );
                }
            }
        }

        if !has_data {
            errors.push(
                DiagramError::new(0, "Pie chart has no data slices")
                    .with_suggestion("Add slices like: \"Label\" : 25"),
            );
        }

        if errors.is_empty() {
            DiagramValidation::valid("pie")
        } else {
            DiagramValidation::invalid("pie", errors)
        }
    }

    /// Generic validation for unknown diagram types
    fn validate_generic(&self, content: &str) -> DiagramValidation {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Basic checks
        let open_braces = content.matches('{').count();
        let close_braces = content.matches('}').count();
        if open_braces != close_braces {
            errors.push(
                DiagramError::new(0, "Mismatched braces").with_suggestion(format!(
                    "Found {} '{{' and {} '}}'",
                    open_braces, close_braces
                )),
            );
        }

        let open_brackets = content.matches('[').count();
        let close_brackets = content.matches(']').count();
        if open_brackets != close_brackets {
            errors.push(
                DiagramError::new(0, "Mismatched brackets").with_suggestion(format!(
                    "Found {} '[' and {} ']'",
                    open_brackets, close_brackets
                )),
            );
        }

        let open_parens = content.matches('(').count();
        let close_parens = content.matches(')').count();
        if open_parens != close_parens {
            errors.push(
                DiagramError::new(0, "Mismatched parentheses").with_suggestion(format!(
                    "Found {} '(' and {} ')'",
                    open_parens, close_parens
                )),
            );
        }

        // Check for unknown diagram type
        let first_line = content.lines().next().unwrap_or("").trim().to_lowercase();
        if !first_line.starts_with("graph")
            && !first_line.starts_with("flowchart")
            && !first_line.starts_with("sequence")
            && !first_line.starts_with("class")
            && !first_line.starts_with("state")
            && !first_line.starts_with("er")
            && !first_line.starts_with("gantt")
            && !first_line.starts_with("pie")
        {
            warnings.push(DiagramWarning {
                line: 1,
                message: format!(
                    "Unknown diagram type: '{}'. Expected: flowchart, sequenceDiagram, classDiagram, etc.",
                    first_line.split_whitespace().next().unwrap_or("(empty)")
                ),
            });
        }

        if errors.is_empty() {
            DiagramValidation::valid("unknown").with_warnings(warnings)
        } else {
            DiagramValidation::invalid("unknown", errors).with_warnings(warnings)
        }
    }
}

/// Quick validation function for use in pipelines
pub fn validate_mermaid(content: &str) -> DiagramValidation {
    DiagramValidator::new().validate(content)
}

/// Check if diagram is valid (convenience function)
pub fn is_valid_mermaid(content: &str) -> bool {
    validate_mermaid(content).is_valid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_flowchart() {
        let diagram = r#"
flowchart TD
    A[Start] --> B{Decision}
    B -->|Yes| C[Action]
    B -->|No| D[End]
"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
        assert_eq!(result.diagram_type, "flowchart");
    }

    #[test]
    fn test_invalid_unclosed_bracket() {
        let diagram = r#"
flowchart TD
    A[Start --> B[End]
"#;
        let result = validate_mermaid(diagram);
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_valid_sequence() {
        let diagram = r#"
sequenceDiagram
    participant A
    participant B
    A->>B: Hello
    B-->>A: Hi
"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
        assert_eq!(result.diagram_type, "sequenceDiagram");
    }

    #[test]
    fn test_strips_code_fence() {
        let diagram = r#"```mermaid
flowchart TD
    A --> B
```"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
    }

    #[test]
    fn test_empty_diagram() {
        let result = validate_mermaid("");
        assert!(!result.is_valid);
        assert_eq!(result.diagram_type, "empty");
    }

    #[test]
    fn test_unknown_type_warning() {
        let diagram = "unknown_type TD\n    A --> B";
        let result = validate_mermaid(diagram);
        assert!(result.is_valid); // Still valid as generic
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_valid_state() {
        let diagram = r#"
stateDiagram-v2
    [*] --> Active
    Active --> Inactive
    Inactive --> [*]
"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
    }

    #[test]
    fn test_valid_class() {
        let diagram = r#"
classDiagram
    class Animal {
        +String name
        +eat()
    }
    Animal <|-- Dog
"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
    }

    #[test]
    fn test_valid_pie() {
        let diagram = r#"
pie
    title Distribution
    "A" : 50
    "B" : 30
    "C" : 20
"#;
        let result = validate_mermaid(diagram);
        assert!(result.is_valid);
    }

    #[test]
    fn test_pie_missing_data() {
        let diagram = "pie\n    title Empty Pie";
        let result = validate_mermaid(diagram);
        assert!(!result.is_valid);
    }
}
