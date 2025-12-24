//! Deep Research Prompts
//!
//! Phase-specific prompts for the Deep Research workflow.
//! Based on DeepWiki's proven multi-turn investigation pattern.

use serde_json::{Value, json};

use super::types::{ResearchContext, ResearchIteration, ResearchPhase};
use crate::analyzer::parser::language::detect_language_or_text;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::bottom_up::RelatedFile;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;

// =============================================================================
// Prompt Builder
// =============================================================================

/// Build phase-specific research prompt
pub fn build_research_prompt(
    phase: ResearchPhase,
    file_path: &str,
    context: &ResearchContext,
    file_content: &str,
    profile: &ProjectProfile,
    max_chars: usize,
) -> String {
    let truncated_content = if file_content.len() > max_chars {
        format!("{}... [truncated]", &file_content[..max_chars])
    } else {
        file_content.to_string()
    };

    let language = detect_language_or_text(file_path).to_string();

    match phase {
        ResearchPhase::Planning => {
            build_planning_prompt(file_path, &truncated_content, &language, profile)
        }
        ResearchPhase::Investigating { iteration } => {
            build_investigation_prompt(file_path, &truncated_content, &language, context, iteration)
        }
        ResearchPhase::Synthesizing => {
            build_synthesis_prompt(file_path, &truncated_content, &language, context, profile)
        }
    }
}

/// Build planning phase prompt (First iteration)
fn build_planning_prompt(
    file_path: &str,
    file_content: &str,
    language: &str,
    profile: &ProjectProfile,
) -> String {
    let tech_stack = if profile.technical_traits.is_empty() {
        "Not specified".to_string()
    } else {
        profile.technical_traits.join(", ")
    };

    format!(
        r##"<ROLE>
You are conducting a Deep Research investigation of the file: `{file_path}`.
This is the FIRST iteration of a multi-turn research process.
Your goal is to create a research plan and identify key aspects to investigate.
</ROLE>

<PROJECT_CONTEXT>
**Project**: {project_name}
**Purpose**: {project_purpose}
**Tech Stack**: {tech_stack}
</PROJECT_CONTEXT>

<GUIDELINES>
- Start your response with a "Research Plan" heading
- Outline your approach to documenting this file comprehensively
- Identify the KEY ASPECTS that need investigation:
  * Architecture and design patterns
  * Critical invariants and constraints
  * Integration points with other components
  * Error handling and edge cases
  * State management (if applicable)
  * Performance considerations
- Provide INITIAL FINDINGS based on code analysis
- End with "Next Steps" indicating what to investigate in the next iteration
- Do NOT provide a final conclusion yet
- Focus EXCLUSIVELY on this file: `{file_path}`
- Do NOT drift to related topics or general concepts
</GUIDELINES>

<FILE_CONTENT path="{file_path}">
```{language}
{file_content}
```
</FILE_CONTENT>

<OUTPUT_REQUIREMENTS>
Your response MUST include these sections:
1. Research Plan - Your investigation approach
2. Key Aspects to Investigate - List of areas requiring deep analysis
3. Initial Findings - What you've discovered from first analysis
4. Next Steps - What to focus on in the next iteration

NEVER respond with just "Continue the research" - provide substantive findings.
</OUTPUT_REQUIREMENTS>

Now create your Research Plan for this file.
"##,
        file_path = file_path,
        project_name = profile.name,
        project_purpose = profile.purposes.join(", "),
        tech_stack = tech_stack,
        language = language,
        file_content = file_content
    )
}

/// Build investigation phase prompt (Intermediate iterations)
fn build_investigation_prompt(
    file_path: &str,
    file_content: &str,
    language: &str,
    context: &ResearchContext,
    iteration: u8,
) -> String {
    let previous_findings = context.summarize_findings();
    let covered = context.covered_aspects_str();

    format!(
        r##"<ROLE>
You are in iteration {iteration} of a Deep Research investigation of: `{file_path}`.
Your goal is to go DEEPER into aspects NOT yet covered.
</ROLE>

<GUIDELINES>
- Start with a "Research Update {iteration}" heading
- REVIEW what has been covered: {covered}
- Focus on ONE specific aspect that needs deeper investigation
- Provide NEW insights not covered in previous iterations
- Build on previous findings, don't repeat them
- Do NOT include information already documented
- If this is iteration 3+, prepare for final synthesis in the next iteration
</GUIDELINES>

<PREVIOUS_RESEARCH>
{previous_findings}
</PREVIOUS_RESEARCH>

<INVESTIGATION_FOCUS>
Choose ONE of these areas for deep dive (or identify a gap):
- Subtle invariants that aren't immediately obvious
- Error paths and recovery mechanisms
- Extension points and their constraints
- Threading/concurrency implications
- Configuration options and their effects
- Hidden dependencies or assumptions
</INVESTIGATION_FOCUS>

<FILE_CONTENT path="{file_path}">
```{language}
{file_content}
```
</FILE_CONTENT>

<OUTPUT_REQUIREMENTS>
Your response MUST include these sections:
1. Research Update {iteration} - Header
2. Focus Area - What specific aspect you're investigating
3. New Findings - Insights NOT covered in previous iterations
4. Implications - How this affects understanding of the file

DO NOT repeat covered topics. Only provide NEW insights.
</OUTPUT_REQUIREMENTS>

Provide your Research Update.
"##,
        file_path = file_path,
        iteration = iteration,
        covered = covered,
        previous_findings = previous_findings,
        language = language,
        file_content = file_content
    )
}

/// Build synthesis phase prompt (Final iteration)
fn build_synthesis_prompt(
    file_path: &str,
    file_content: &str,
    language: &str,
    context: &ResearchContext,
    profile: &ProjectProfile,
) -> String {
    let all_findings = context.summarize_findings();

    format!(
        r##"<ROLE>
You are in the FINAL iteration of a Deep Research investigation of: `{file_path}`.
Your goal is to SYNTHESIZE all findings into comprehensive, production-ready documentation.
</ROLE>

<PROJECT_CONTEXT>
**Project**: {project_name}
**Purpose**: {project_purpose}
</PROJECT_CONTEXT>

<ALL_PREVIOUS_RESEARCH>
{all_findings}
</ALL_PREVIOUS_RESEARCH>

<GUIDELINES>
- Synthesize ALL findings from previous iterations
- Create complete, standalone documentation including:
  * Purpose and architectural role
  * How it works (with code references like `file:line`)
  * Critical invariants and constraints
  * Integration patterns with other components
  * Mermaid diagram showing key relationships
- Reference specific code locations
- Ensure documentation is complete - a developer should understand this file fully
- Do NOT say "Continue the research" - provide definitive documentation
</GUIDELINES>

<FILE_CONTENT path="{file_path}">
```{language}
{file_content}
```
</FILE_CONTENT>

<OUTPUT_REQUIREMENTS>
Your response MUST synthesize everything into final documentation following this exact JSON schema.
The output should be comprehensive and directly usable as documentation.

Include:
1. "purpose": Clear 1-2 sentence purpose statement
2. "content": Rich markdown documentation with:
   - Overview section
   - How It Works section with code references
   - Critical Details section
   - Integration Points section
3. "diagram": Mermaid diagram showing architecture/flow (no wrapper)
4. "related_files": Array of related files with relationship types
5. "aspects_covered": Array of all aspects investigated

This is the FINAL output - make it complete and professional.
</OUTPUT_REQUIREMENTS>

Now synthesize everything into final documentation.
"##,
        file_path = file_path,
        project_name = profile.name,
        project_purpose = profile.purposes.join(", "),
        all_findings = all_findings,
        language = language,
        file_content = file_content
    )
}

// =============================================================================
// Output Schema
// =============================================================================

/// Get JSON schema for research output based on phase
pub fn research_output_schema(phase: ResearchPhase) -> Value {
    match phase {
        ResearchPhase::Planning | ResearchPhase::Investigating { .. } => json!({
            "type": "object",
            "description": "Research iteration output",
            "required": ["findings", "new_aspects"],
            "additionalProperties": false,
            "properties": {
                "findings": {
                    "type": "string",
                    "description": "Markdown findings for this iteration"
                },
                "new_aspects": {
                    "type": "array",
                    "description": "New aspects discovered/investigated in this iteration",
                    "items": {"type": "string"}
                }
            }
        }),
        ResearchPhase::Synthesizing => json!({
            "type": "object",
            "description": "Final synthesis output - complete file documentation",
            "required": ["purpose", "content", "aspects_covered"],
            "additionalProperties": false,
            "properties": {
                "purpose": {
                    "type": "string",
                    "description": "Clear 1-2 sentence purpose statement"
                },
                "content": {
                    "type": "string",
                    "description": "Rich markdown documentation with all sections"
                },
                "diagram": {
                    "type": "string",
                    "description": "Mermaid diagram (no wrapper, just diagram code)"
                },
                "related_files": {
                    "type": "array",
                    "description": "Related files discovered during research",
                    "items": {
                        "type": "object",
                        "required": ["path", "relationship"],
                        "properties": {
                            "path": {"type": "string"},
                            "relationship": {"type": "string"}
                        }
                    }
                },
                "aspects_covered": {
                    "type": "array",
                    "description": "All aspects covered in research",
                    "items": {"type": "string"}
                }
            }
        }),
    }
}

// =============================================================================
// Output Parser
// =============================================================================

/// Parse research output from LLM response
pub fn parse_research_output(
    phase: ResearchPhase,
    response: &Value,
) -> Result<ResearchIteration, WeaveError> {
    match phase {
        ResearchPhase::Planning | ResearchPhase::Investigating { .. } => {
            let findings = response
                .get("findings")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let new_aspects = response
                .get("new_aspects")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            Ok(ResearchIteration {
                phase,
                findings,
                new_aspects,
                purpose: None,
                content: None,
                diagram: None,
                related_files: vec![],
            })
        }
        ResearchPhase::Synthesizing => {
            let purpose = response
                .get("purpose")
                .and_then(|v| v.as_str())
                .unwrap_or("Purpose not specified")
                .to_string();

            let content = response
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let diagram = response
                .get("diagram")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);

            let related_files = response
                .get("related_files")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            let path = v.get("path")?.as_str()?;
                            let relationship = v.get("relationship")?.as_str()?;
                            Some(RelatedFile::new(path, relationship))
                        })
                        .collect()
                })
                .unwrap_or_default();

            let new_aspects = response
                .get("aspects_covered")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            // For synthesis, findings = content
            Ok(ResearchIteration {
                phase,
                findings: content.clone(),
                new_aspects,
                purpose: Some(purpose),
                content: Some(content),
                diagram,
                related_files,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_output_schema_planning() {
        let schema = research_output_schema(ResearchPhase::Planning);
        assert!(schema.get("properties").is_some());
        assert!(schema["properties"].get("findings").is_some());
        assert!(schema["properties"].get("new_aspects").is_some());
    }

    #[test]
    fn test_research_output_schema_synthesis() {
        let schema = research_output_schema(ResearchPhase::Synthesizing);
        assert!(schema.get("properties").is_some());
        assert!(schema["properties"].get("purpose").is_some());
        assert!(schema["properties"].get("content").is_some());
        assert!(schema["properties"].get("diagram").is_some());
        assert!(schema["properties"].get("related_files").is_some());
    }

    #[test]
    fn test_parse_planning_output() {
        let response = json!({
            "findings": "This file handles...",
            "new_aspects": ["architecture", "error_handling"]
        });

        let result = parse_research_output(ResearchPhase::Planning, &response).unwrap();
        assert_eq!(result.findings, "This file handles...");
        assert_eq!(result.new_aspects.len(), 2);
        assert!(result.purpose.is_none()); // Only set in synthesis
    }

    #[test]
    fn test_parse_synthesis_output() {
        let response = json!({
            "purpose": "Handles user authentication",
            "content": "Overview section content...",
            "diagram": "graph TD; A-->B",
            "related_files": [
                {"path": "src/auth/types.rs", "relationship": "imports"}
            ],
            "aspects_covered": ["auth", "tokens"]
        });

        let result = parse_research_output(ResearchPhase::Synthesizing, &response).unwrap();
        assert_eq!(
            result.purpose,
            Some("Handles user authentication".to_string())
        );
        assert!(result.content.is_some());
        assert!(result.diagram.is_some());
        assert_eq!(result.related_files.len(), 1);
    }
}
