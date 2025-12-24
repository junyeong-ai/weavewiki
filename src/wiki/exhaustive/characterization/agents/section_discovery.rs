//! Section Discovery Agent - Turn 3
//!
//! Discovers domain-specific sections to extract based on project analysis.
//! Uses Turn 1+2 context to determine what business-value sections should
//! be extracted from the codebase.

use super::SectionDiscoveryInsight;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};
use serde_json::json;

#[derive(Default)]
pub struct SectionDiscoveryAgent;

#[async_trait::async_trait]
impl CharacterizationAgent for SectionDiscoveryAgent {
    fn name(&self) -> &str {
        "section_discovery"
    }

    fn turn(&self) -> u8 {
        3
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        tracing::debug!(
            "SectionDiscoveryAgent: Analyzing with {} prior insights",
            context.prior_insights.len()
        );

        let domain_context = self.extract_domain_context(context);
        let purpose_context = self.extract_purpose_context(context);
        let technical_context = self.extract_technical_context(context);

        let prompt = self.build_prompt(&domain_context, &purpose_context, &technical_context);
        let schema = self.schema();
        let full_prompt = format!("{}\n\n{}", Self::system_prompt(), prompt);

        let response = context
            .provider
            .generate(&full_prompt, &schema)
            .await
            .map_err(|e| {
                WeaveError::LlmApi(format!("Section discovery agent LLM call failed: {}", e))
            })?;

        let insight = match Self::parse_response(&response.content) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(
                    "SectionDiscoveryAgent: Failed to parse response, using fallback: {}",
                    e
                );
                Self::fallback_sections(&domain_context)
            }
        };

        let insight_json = serde_json::to_value(&insight)
            .map_err(|e| WeaveError::LlmApi(format!("Failed to serialize insight: {}", e)))?;

        tracing::info!(
            "SectionDiscoveryAgent: Discovered {} domain-specific sections",
            insight.sections.len()
        );

        Ok(AgentOutput {
            agent_name: self.name().to_string(),
            turn: self.turn(),
            insight_json,
            confidence: if insight.sections.is_empty() {
                0.3
            } else {
                0.85
            },
        })
    }
}

impl SectionDiscoveryAgent {
    fn system_prompt() -> &'static str {
        r#"You are a domain expert that identifies what business-critical information
should be extracted from a codebase for documentation.

Your task is to discover domain-specific sections that provide high business value.
These sections should be:
1. Specific to this project's domain (not generic programming concepts)
2. Extractable from code (observable patterns, not speculation)
3. High value for developers working with this codebase

Focus on FACTS observable in the code structure and domain."#
    }

    fn build_prompt(
        &self,
        domain_context: &str,
        purpose_context: &str,
        technical_context: &str,
    ) -> String {
        format!(
            r#"Based on the project analysis, determine what domain-specific sections should be extracted for comprehensive documentation.

## Project Analysis

### Domain Characteristics
{domain_context}

### Project Purpose
{purpose_context}

### Technical Traits
{technical_context}

## Instructions

Based on this project's specific characteristics, propose 3-7 domain-specific sections.

For each section provide:
1. **name**: Clear, descriptive name
2. **description**: What this section documents and why it matters
3. **content_type**: One of: state_transitions, flow, rules, data_transform, api_contract, configuration, freeform
4. **extraction_hints**: What to look for in code (function names, patterns, keywords)
5. **importance**: critical, high, medium, low
6. **file_patterns**: Glob patterns for likely files (optional)

Return JSON matching the schema exactly."#
        )
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "SectionDiscoveryInsight",
            "type": "object",
            "required": ["sections"],
            "properties": {
                "sections": {
                    "type": "array",
                    "description": "Domain-specific sections to extract",
                    "items": {
                        "type": "object",
                        "required": ["name", "description", "content_type", "extraction_hints", "importance"],
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "content_type": {
                                "type": "string",
                                "enum": ["state_transitions", "flow", "rules", "data_transform", "api_contract", "configuration", "freeform"]
                            },
                            "extraction_hints": { "type": "array", "items": {"type": "string"} },
                            "importance": { "type": "string", "enum": ["critical", "high", "medium", "low"] },
                            "file_patterns": { "type": "array", "items": {"type": "string"} }
                        }
                    },
                    "minItems": 1,
                    "maxItems": 7
                }
            }
        })
    }

    fn extract_domain_context(&self, context: &CharacterizationContext) -> String {
        for output in &context.prior_insights {
            if output.agent_name == "domain"
                && let Some(traits) = output.insight_json.get("domain_traits")
                && let Some(arr) = traits.as_array()
            {
                let traits: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                return format!("Domain traits: {}", traits.join(", "));
            }
        }
        "Domain: General software application".to_string()
    }

    fn extract_purpose_context(&self, context: &CharacterizationContext) -> String {
        for output in &context.prior_insights {
            if output.agent_name == "purpose" {
                let mut parts = vec![];
                if let Some(purposes) = output
                    .insight_json
                    .get("purposes")
                    .and_then(|v| v.as_array())
                {
                    let p: Vec<&str> = purposes.iter().filter_map(|v| v.as_str()).collect();
                    if !p.is_empty() {
                        parts.push(format!("Purposes: {}", p.join(", ")));
                    }
                }
                if let Some(users) = output
                    .insight_json
                    .get("target_users")
                    .and_then(|v| v.as_array())
                {
                    let u: Vec<&str> = users.iter().filter_map(|v| v.as_str()).collect();
                    if !u.is_empty() {
                        parts.push(format!("Target users: {}", u.join(", ")));
                    }
                }
                if !parts.is_empty() {
                    return parts.join("\n");
                }
            }
        }
        "Purpose: Unknown".to_string()
    }

    fn extract_technical_context(&self, context: &CharacterizationContext) -> String {
        for output in &context.prior_insights {
            if output.agent_name == "technical"
                && let Some(traits) = output
                    .insight_json
                    .get("technical_traits")
                    .and_then(|v| v.as_array())
            {
                let t: Vec<&str> = traits.iter().filter_map(|v| v.as_str()).collect();
                if !t.is_empty() {
                    return format!("Technical traits: {}", t.join(", "));
                }
            }
        }
        "Technical: Standard patterns".to_string()
    }

    fn parse_response(response: &serde_json::Value) -> Result<SectionDiscoveryInsight, WeaveError> {
        serde_json::from_value::<SectionDiscoveryInsight>(response.clone()).map_err(|e| {
            WeaveError::LlmApi(format!("Failed to parse SectionDiscoveryInsight: {}", e))
        })
    }

    fn fallback_sections(domain_context: &str) -> SectionDiscoveryInsight {
        use super::DiscoveredSection;

        let domain_lower = domain_context.to_lowercase();

        let sections = if domain_lower.contains("api") || domain_lower.contains("server") {
            vec![DiscoveredSection {
                name: "Request Processing Flow".to_string(),
                description: "How requests are received, validated, and processed".to_string(),
                content_type: "flow".to_string(),
                extraction_hints: vec![
                    "handler functions".to_string(),
                    "middleware".to_string(),
                    "request/response types".to_string(),
                ],
                importance: "high".to_string(),
                file_patterns: vec!["**/handler*".to_string(), "**/api*".to_string()],
            }]
        } else if domain_lower.contains("cli") || domain_lower.contains("command") {
            vec![DiscoveredSection {
                name: "Command Structure".to_string(),
                description: "Available commands and their arguments".to_string(),
                content_type: "api_contract".to_string(),
                extraction_hints: vec![
                    "command definitions".to_string(),
                    "argument parsing".to_string(),
                    "subcommands".to_string(),
                ],
                importance: "high".to_string(),
                file_patterns: vec!["**/cli*".to_string(), "**/command*".to_string()],
            }]
        } else {
            vec![DiscoveredSection {
                name: "Core Business Logic".to_string(),
                description: "Main business rules and processing logic".to_string(),
                content_type: "freeform".to_string(),
                extraction_hints: vec![
                    "main processing functions".to_string(),
                    "business rule implementations".to_string(),
                ],
                importance: "high".to_string(),
                file_patterns: vec![],
            }]
        };

        SectionDiscoveryInsight { sections }
    }
}
