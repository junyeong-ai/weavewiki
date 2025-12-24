//! Risk Agent
//!
//! Aggregates file-level risks into project-wide risk map.

use super::{TopDownAgent, TopDownAgentConfig, TopDownContext, run_top_down_agent};
use crate::types::{error::WeaveError, json_string_array};
use crate::wiki::exhaustive::top_down::insights::{
    CrossCuttingRisk, ModificationHotspot, ProjectInsight, RiskArea,
};
use crate::wiki::exhaustive::types::Importance;
use serde_json::json;

#[derive(Default)]
pub struct RiskAgent;

#[async_trait::async_trait]
impl TopDownAgent for RiskAgent {
    fn name(&self) -> &str {
        "risk"
    }

    async fn run(&self, context: &TopDownContext) -> Result<ProjectInsight, WeaveError> {
        run_top_down_agent(
            context,
            TopDownAgentConfig {
                name: "risk",
                build_context: Box::new(Self::build_risk_summary),
                build_prompt: Box::new(Self::build_prompt),
                schema: Self::schema(),
                parse_result: Box::new(Self::parse_result),
                debug_result: Box::new(|insight| {
                    format!(
                        "Found {} risk areas, {} hotspots, {} cross-cutting",
                        insight.risk_map.len(),
                        insight.modification_hotspots.len(),
                        insight.cross_cutting_risks.len()
                    )
                }),
            },
        )
        .await
    }
}

impl RiskAgent {
    fn build_risk_summary(context: &TopDownContext) -> String {
        let mut summary = String::new();
        let mut risks_by_file: Vec<(&str, String)> = Vec::new();

        for fi in &context.file_insights {
            let content_lower = fi.content.to_lowercase();
            if content_lower.contains("risk")
                || content_lower.contains("warning")
                || content_lower.contains("caution")
                || content_lower.contains("danger")
            {
                let first_line = fi
                    .content
                    .lines()
                    .find(|l| {
                        let ll = l.to_lowercase();
                        ll.contains("risk") || ll.contains("warning") || ll.contains("caution")
                    })
                    .unwrap_or("")
                    .to_string();
                risks_by_file.push((&fi.file_path, first_line));
            }
        }

        for (file, risk) in risks_by_file.iter().take(20) {
            summary.push_str(&format!("{}:\n", file));
            summary.push_str(&format!("  - {}\n", risk));
        }

        if risks_by_file.len() > 20 {
            summary.push_str(&format!(
                "... and {} more files with risks\n",
                risks_by_file.len() - 20
            ));
        }

        if summary.is_empty() {
            summary = "No explicit risks documented. Analyze based on code patterns.".to_string();
        }

        summary
    }

    fn build_prompt(context: &TopDownContext, risk_summary: &str) -> String {
        format!(
            r#"<ROLE>
You are a software risk analyst specializing in identifying architectural risks,
modification hotspots, and cross-cutting concerns in codebases.
Your task is to assess risk severity, identify critical modification points,
and highlight systemic vulnerabilities with concrete evidence.
</ROLE>

<OBJECTIVES>
Analyze the project's risks to:
1. Identify risk areas with clear severity levels based on impact and likelihood
2. Locate modification hotspots (files that affect many others when changed)
3. Assess cross-cutting risks (security, performance, reliability, testability)
4. Provide evidence for each risk identified
5. Suggest concrete mitigation strategies
</OBJECTIVES>

## Project Context
Name: {}
Scale: {:?}

## File-Level Risks
{}

<FOCUS>
IMPORTANT: Focus on OBSERVABLE risks in the actual code structure.
- Do NOT speculate about risks without evidence from the provided data
- Do NOT apply generic risk categories without specific justification
- Do NOT recommend solutions beyond architectural improvements
- ONLY identify risks with concrete evidence from files, dependencies, and patterns
</FOCUS>

## Risk Assessment Guidelines
- **Critical**: Code failures cause system outages or data loss
- **High**: Code failures impact core functionality or multiple systems
- **Medium**: Code failures affect specific features
- **Low**: Code failures have limited scope

## Analysis Task
Identify:
1. Risk areas: Specific architectural, operational, or quality risks with severity
2. Modification hotspots: Files that cascade impacts to many dependents when changed
3. Cross-cutting risks: Systemic issues spanning multiple modules

<ANTI-PATTERNS>
WRONG - Vague risks:
- "The codebase has moderate complexity risk"
- "There's a general performance concern"
- "Error handling needs improvement"

WRONG - Speculation:
- "The system might have security vulnerabilities"
- "There could be race conditions in async code"
- "Potential scalability issues with current design"

CORRECT - Evidence-based risks:
- "CRITICAL: Error handling gap in storage/database.rs:120-145. Connection failures during transaction not caught, locks may be held indefinitely. Affects 21 dependent files."
- "HIGH: Circular dependency detected: auth/session.rs:25 imports from auth/token.rs:40 which imports back. Evidence: import statements confirmed."
- "Modification hotspot: src/types/mod.rs imported by 34 files. Changes to core types will require updating all dependents."
</ANTI-PATTERNS>
"#,
            context.profile.name, context.profile.scale, risk_summary
        )
    }

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project risk analysis with severity levels, hotspots, and cross-cutting risks. ALL risks MUST include file references and concrete evidence.",
            "required": ["risk_areas"],
            "additionalProperties": false,
            "properties": {
                "risk_areas": {
                    "type": "array",
                    "description": "Identified risk areas with severity assessment and evidence",
                    "minItems": 0,
                    "maxItems": 30,
                    "items": {
                        "type": "object",
                        "required": ["area", "risk_level", "evidence"],
                        "additionalProperties": false,
                        "properties": {
                            "area": {"type": "string", "description": "Name/description of the risk area", "minLength": 5, "maxLength": 200},
                            "risk_level": {"type": "string", "description": "Severity level", "enum": ["critical", "high", "medium", "low"]},
                            "description": {"type": "string", "description": "Detailed explanation of the risk", "minLength": 20, "maxLength": 500},
                            "files": {"type": "array", "description": "Files involved with line references (file.rs:45-60)", "maxItems": 50, "items": {"type": "string", "maxLength": 300}},
                            "evidence": {"type": "array", "description": "Specific observations supporting risk assessment", "minItems": 1, "maxItems": 10, "items": {"type": "string", "minLength": 10, "maxLength": 500}},
                            "impact": {"type": "string", "description": "What happens if this risk materializes", "maxLength": 300},
                            "likelihood": {"type": "string", "description": "How likely is this to occur", "enum": ["high", "medium", "low"]}
                        }
                    }
                },
                "modification_hotspots": {
                    "type": "array",
                    "description": "Files that cascade impacts to many dependents when modified",
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "required": ["file", "reason", "dependent_count"],
                        "additionalProperties": false,
                        "properties": {
                            "file": {"type": "string", "description": "File path with criticality", "minLength": 1, "maxLength": 300},
                            "reason": {"type": "string", "description": "Why this is a hotspot", "minLength": 10, "maxLength": 300},
                            "dependent_count": {"type": "integer", "description": "Number of files depending on this", "minimum": 1},
                            "dependents": {"type": "array", "description": "List of dependent files", "maxItems": 50, "items": {"type": "string", "maxLength": 300}},
                            "risk": {"type": "string", "description": "Risk created by this hotspot", "maxLength": 300}
                        }
                    }
                },
                "cross_cutting_risks": {
                    "type": "array",
                    "description": "Systemic risks spanning multiple modules/areas",
                    "maxItems": 15,
                    "items": {
                        "type": "object",
                        "required": ["name", "affected_areas"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Risk category name", "minLength": 3, "maxLength": 100},
                            "description": {"type": "string", "description": "What the cross-cutting risk is", "minLength": 10, "maxLength": 500},
                            "affected_areas": {"type": "array", "description": "Modules/areas affected", "minItems": 1, "maxItems": 20, "items": {"type": "string", "maxLength": 100}},
                            "examples": {"type": "array", "description": "Specific examples from code", "maxItems": 10, "items": {"type": "string", "maxLength": 300}},
                            "mitigation": {"type": "string", "description": "Suggested approach to address", "minLength": 10, "maxLength": 500}
                        }
                    }
                }
            }
        })
    }

    fn parse_result(result: &serde_json::Value, insight: &mut ProjectInsight) {
        insight.risk_map = result
            .get("risk_areas")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let risk_level = match v.get("risk_level")?.as_str()? {
                            "critical" => Importance::Critical,
                            "high" => Importance::High,
                            "low" => Importance::Low,
                            _ => Importance::Medium,
                        };
                        Some(RiskArea {
                            area: v.get("area")?.as_str()?.to_string(),
                            risk_level,
                            files: json_string_array(v, "files"),
                            evidence: json_string_array(v, "evidence"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        insight.modification_hotspots = result
            .get("modification_hotspots")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(ModificationHotspot {
                            file: v.get("file")?.as_str()?.to_string(),
                            reason: v.get("reason")?.as_str()?.to_string(),
                            dependents: json_string_array(v, "dependents"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        insight.cross_cutting_risks = result
            .get("cross_cutting_risks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(CrossCuttingRisk {
                            name: v.get("name")?.as_str()?.to_string(),
                            affected_areas: json_string_array(v, "affected_areas"),
                            mitigation: v
                                .get("mitigation")
                                .and_then(|m| m.as_str())
                                .map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}
