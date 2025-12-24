//! Flow Agent
//!
//! Traces end-to-end business flows across files.

use super::{TopDownAgent, TopDownAgentConfig, TopDownContext, run_top_down_agent};
use crate::types::{error::WeaveError, json_string_array};
use crate::wiki::exhaustive::top_down::insights::{
    BusinessFlow, DataPipeline, EventFlow, ProjectInsight,
};
use serde_json::json;

#[derive(Default)]
pub struct FlowAgent;

#[async_trait::async_trait]
impl TopDownAgent for FlowAgent {
    fn name(&self) -> &str {
        "flow"
    }

    async fn run(&self, context: &TopDownContext) -> Result<ProjectInsight, WeaveError> {
        run_top_down_agent(
            context,
            TopDownAgentConfig {
                name: "flow",
                build_context: Box::new(Self::build_workflow_summary),
                build_prompt: Box::new(Self::build_prompt),
                schema: Self::schema(),
                parse_result: Box::new(Self::parse_result),
                debug_result: Box::new(|insight| {
                    format!(
                        "Found {} business flows, {} event flows, {} pipelines",
                        insight.business_flows.len(),
                        insight.event_flows.len(),
                        insight.data_pipelines.len()
                    )
                }),
            },
        )
        .await
    }
}

impl FlowAgent {
    fn build_workflow_summary(context: &TopDownContext) -> String {
        let mut summary = String::new();

        for fi in context.file_insights.iter().take(30) {
            let content_lower = fi.content.to_lowercase();
            if content_lower.contains("flow")
                || content_lower.contains("pipeline")
                || content_lower.contains("state")
                || content_lower.contains("workflow")
            {
                summary.push_str(&format!("{}:\n", fi.file_path));
                if let Some(first_line) = fi.content.lines().next() {
                    summary.push_str(&format!("  - {}\n", first_line));
                }
            }
        }

        if summary.is_empty() {
            summary =
                "No explicit workflows detected. Analyze based on file structure.".to_string();
        }

        summary
    }

    fn build_prompt(context: &TopDownContext, workflow_summary: &str) -> String {
        format!(
            r#"<ROLE>
You are a flow and process analysis expert specializing in tracing end-to-end workflows
across systems. Your task is to identify and document business flows, event flows, and
data pipelines with concrete code references.
</ROLE>

<OBJECTIVES>
Analyze the project's flows to:
1. Identify business flows (user-initiated workflows, API request journeys)
2. Map event-driven flows (event producers, subscribers, handlers)
3. Document data pipelines (data sources, transformations, destinations)
4. Understand flow dependencies and integration points
</OBJECTIVES>

## Project Context
Name: {}
Purposes: {}
Technical Traits: {}

## Detected Workflows
{}

<FOCUS>
IMPORTANT: Focus EXCLUSIVELY on flows traceable in the provided code and structure.
- Do NOT speculate about flows not evident in the codebase
- Do NOT describe generic flow concepts
- Do NOT trace flows beyond what the code reveals
- ONLY document flows that can be verified by code references (file:line)
</FOCUS>

## Analysis Task
Analyze code structure to identify:
1. Business flows: End-to-end workflows initiated by users or external systems
2. Event flows: Event-driven patterns (publishers, subscribers, handlers)
3. Data pipelines: Data transformation workflows (source → processing → destination)
4. Integration points between flows

Include Mermaid flowchart diagrams for complex flows.

<ANTI-PATTERNS>
WRONG - Speculative flows:
- "Users probably create accounts, which triggers X"
- "Likely the system processes data in batches"
- "Assuming a webhook sends data from external system"

WRONG - Generic flow descriptions:
- "The system has a workflow"
- "Data is processed through a pipeline"
- "Events flow through the system"

CORRECT - Evidence-based flows:
- "Business flow 'CreateUser': InitiateRequest (api/handler.rs:45) → ValidateInput (domain/user.rs:120) → SaveToDB (storage/db.rs:87)"
- "Event flow 'UserCreated': Published at storage/db.rs:110, Subscribed by notifications/emailer.rs:25"
- "Data pipeline 'LogProcessing': FileSource → JsonParser → MetricsAggregator → MetricsWriter"
</ANTI-PATTERNS>
"#,
            context.profile.name,
            context.profile.purposes.join(", "),
            context.profile.technical_traits.join(", "),
            workflow_summary
        )
    }

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Business flows, event flows, and data pipelines. Include file:line references in all descriptions.",
            "required": ["business_flows"],
            "additionalProperties": false,
            "properties": {
                "business_flows": {
                    "type": "array",
                    "description": "End-to-end business workflows with step-by-step execution paths",
                    "minItems": 0,
                    "maxItems": 30,
                    "items": {
                        "type": "object",
                        "required": ["name", "steps"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Flow name (e.g., 'CreateUser', 'ProcessPayment')", "minLength": 2, "maxLength": 100},
                            "trigger": {"type": "string", "description": "What initiates this flow", "maxLength": 200},
                            "steps": {"type": "array", "description": "Sequential steps with file:line references", "minItems": 1, "maxItems": 20, "items": {"type": "string", "minLength": 5, "maxLength": 300}},
                            "diagram": {"type": "string", "description": "Mermaid flowchart (NO ```mermaid wrapper). Use 'graph TD;' or 'graph LR;' format.", "maxLength": 5000}
                        }
                    }
                },
                "event_flows": {
                    "type": "array",
                    "description": "Event-driven patterns with producers and subscribers",
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "required": ["name", "events"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Event flow name", "minLength": 2, "maxLength": 100},
                            "events": {"type": "array", "description": "Events published/emitted", "minItems": 1, "maxItems": 20, "items": {"type": "string", "maxLength": 200}},
                            "producers": {"type": "array", "description": "File:line locations that emit events", "maxItems": 20, "items": {"type": "string", "maxLength": 200}},
                            "handlers": {"type": "array", "description": "File:line locations that handle events", "maxItems": 20, "items": {"type": "string", "maxLength": 200}}
                        }
                    }
                },
                "data_pipelines": {
                    "type": "array",
                    "description": "Data transformation pipelines from source to destination",
                    "maxItems": 15,
                    "items": {
                        "type": "object",
                        "required": ["name", "stages"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Pipeline name", "minLength": 2, "maxLength": 100},
                            "description": {"type": "string", "description": "What this pipeline does", "maxLength": 300},
                            "stages": {"type": "array", "description": "Processing stages with file:line", "minItems": 1, "maxItems": 15, "items": {"type": "string", "maxLength": 300}},
                            "source": {"type": "string", "description": "Data source with file:line reference", "maxLength": 300},
                            "destination": {"type": "string", "description": "Data destination with file:line reference", "maxLength": 300}
                        }
                    }
                }
            }
        })
    }

    fn parse_result(result: &serde_json::Value, insight: &mut ProjectInsight) {
        insight.business_flows = result
            .get("business_flows")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(BusinessFlow {
                            name: v.get("name")?.as_str()?.to_string(),
                            steps: json_string_array(v, "steps"),
                            diagram: v.get("diagram").and_then(|d| d.as_str()).map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        insight.event_flows = result
            .get("event_flows")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(EventFlow {
                            name: v.get("name")?.as_str()?.to_string(),
                            events: json_string_array(v, "events"),
                            handlers: json_string_array(v, "handlers"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        insight.data_pipelines = result
            .get("data_pipelines")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(DataPipeline {
                            name: v.get("name")?.as_str()?.to_string(),
                            stages: json_string_array(v, "stages"),
                            source: v.get("source").and_then(|s| s.as_str()).map(String::from),
                            destination: v
                                .get("destination")
                                .and_then(|d| d.as_str())
                                .map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}
