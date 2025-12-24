//! Profile Synthesis - Merge agent outputs into unified ProjectProfile

use super::AgentOutput;
use super::profile::ProjectProfile;
use crate::ai::provider::SharedProvider;
use crate::config::{AnalysisMode, ProjectScale};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::types::Importance;
use serde_json::json;

/// Synthesize all agent outputs into a unified ProjectProfile
pub struct ProfileSynthesis {
    project_name: String,
    scale: ProjectScale,
    mode: AnalysisMode,
}

impl ProfileSynthesis {
    pub fn new(project_name: String, scale: ProjectScale, mode: AnalysisMode) -> Self {
        Self {
            project_name,
            scale,
            mode,
        }
    }

    /// Merge Turn 1 and Turn 2 agent outputs into a ProjectProfile
    pub fn synthesize(
        &self,
        turn1_outputs: Vec<AgentOutput>,
        turn2_outputs: Vec<AgentOutput>,
    ) -> Result<ProjectProfile, WeaveError> {
        let mut profile = ProjectProfile::new(self.project_name.clone(), self.scale, self.mode);

        profile.characterization_turns = if turn2_outputs.is_empty() { 1 } else { 2 };

        // Process Turn 1 outputs (Structure, Dependency, EntryPoint)
        for output in &turn1_outputs {
            self.apply_turn1_output(&mut profile, output)?;
        }

        // Process Turn 2 outputs (Purpose, Technical, Domain)
        for output in &turn2_outputs {
            self.apply_turn2_output(&mut profile, output)?;
        }

        Ok(profile)
    }

    /// Enhanced synthesis using LLM for Large+ projects (Turn 3)
    pub async fn synthesize_with_llm(
        &self,
        turn1_outputs: Vec<AgentOutput>,
        turn2_outputs: Vec<AgentOutput>,
        provider: &SharedProvider,
    ) -> Result<ProjectProfile, WeaveError> {
        // First do basic synthesis
        let mut profile = self.synthesize(turn1_outputs.clone(), turn2_outputs.clone())?;
        profile.characterization_turns = 3;

        // Collect all insights for LLM refinement
        let all_insights: Vec<_> = turn1_outputs
            .iter()
            .chain(turn2_outputs.iter())
            .map(|o| format!("{}: {}", o.agent_name, o.insight_json))
            .collect();

        let prompt = format!(
            r#"Analyze and refine this project characterization for "{}".

## Current Agent Insights
{}

## Current Profile Summary
- Purposes: {:?}
- Technical Traits: {:?}
- Domain Traits: {:?}
- Architecture Hints: {:?}

## Instructions
Identify:
1. Any missing key areas that should be prioritized for documentation
2. Potential cross-cutting concerns that span multiple modules
3. Refined understanding of the project's core value proposition

Return JSON with refined insights."#,
            self.project_name,
            all_insights.join("\n\n"),
            profile.purposes,
            profile.technical_traits,
            profile.domain_traits,
            profile.architecture_hints
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "additional_key_areas": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "cross_cutting_concerns": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "refined_purposes": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            }
        });

        let result = provider.generate(&prompt, &schema).await?.content;

        // Apply refined insights
        if let Some(areas) = result
            .get("additional_key_areas")
            .and_then(|v| v.as_array())
        {
            for area in areas {
                if let Some(s) = area.as_str() {
                    profile.key_areas.push(super::profile::KeyArea {
                        path: s.to_string(),
                        importance: Importance::Medium,
                        focus_reasons: vec!["Turn 3 synthesis identified".to_string()],
                    });
                }
            }
        }

        if let Some(concerns) = result
            .get("cross_cutting_concerns")
            .and_then(|v| v.as_array())
        {
            for concern in concerns {
                if let Some(s) = concern.as_str()
                    && !profile.architecture_hints.contains(&s.to_string())
                {
                    profile.architecture_hints.push(s.to_string());
                }
            }
        }

        if let Some(purposes) = result.get("refined_purposes").and_then(|v| v.as_array())
            && !purposes.is_empty()
        {
            profile.purposes.clear();
            for p in purposes {
                if let Some(s) = p.as_str() {
                    profile.purposes.push(s.to_string());
                }
            }
        }

        tracing::debug!(
            "Turn 3 synthesis: added {} key areas, {} cross-cutting concerns",
            result
                .get("additional_key_areas")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            result
                .get("cross_cutting_concerns")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        );

        Ok(profile)
    }

    fn apply_turn1_output(
        &self,
        profile: &mut ProjectProfile,
        output: &AgentOutput,
    ) -> Result<(), WeaveError> {
        match output.agent_name.as_str() {
            "structure" => {
                // Extract organization_style, key_areas
                if let Some(style) = output.insight_json.get("organization_style")
                    && let Some(s) = style.as_str()
                {
                    profile.organization_style = match s {
                        "domain-driven" => super::profile::OrganizationStyle::DomainDriven,
                        "layer-based" => super::profile::OrganizationStyle::LayerBased,
                        "feature-based" => super::profile::OrganizationStyle::FeatureBased,
                        "flat" => super::profile::OrganizationStyle::Flat,
                        _ => super::profile::OrganizationStyle::Hybrid,
                    };
                }
            }
            "dependency" => {
                // Extract technical indicators from dependencies
                if let Some(frameworks) = output.insight_json.get("framework_indicators")
                    && let Some(arr) = frameworks.as_array()
                {
                    for v in arr {
                        if let Some(s) = v.as_str()
                            && !profile.technical_traits.contains(&s.to_string())
                        {
                            profile.technical_traits.push(s.to_string());
                        }
                    }
                }
            }
            "entry_point" => {
                // Extract entry points
                if let Some(entries) = output.insight_json.get("entry_points")
                    && let Some(arr) = entries.as_array()
                {
                    for v in arr {
                        let entry = super::profile::EntryPoint {
                            entry_type: v
                                .get("entry_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            file: v
                                .get("file")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            symbol: v.get("symbol").and_then(|v| v.as_str()).map(String::from),
                        };
                        profile.entry_points.push(entry);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_turn2_output(
        &self,
        profile: &mut ProjectProfile,
        output: &AgentOutput,
    ) -> Result<(), WeaveError> {
        match output.agent_name.as_str() {
            "purpose" => {
                // Extract purposes and target users
                if let Some(purposes) = output.insight_json.get("purposes")
                    && let Some(arr) = purposes.as_array()
                {
                    profile.purposes.clear();
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            profile.purposes.push(s.to_string());
                        }
                    }
                }
                if let Some(users) = output.insight_json.get("target_users")
                    && let Some(arr) = users.as_array()
                {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            profile.target_users.push(s.to_string());
                        }
                    }
                }
            }
            "technical" => {
                // Extract technical traits and architecture hints
                if let Some(traits) = output.insight_json.get("technical_traits")
                    && let Some(arr) = traits.as_array()
                {
                    for v in arr {
                        if let Some(s) = v.as_str()
                            && !profile.technical_traits.contains(&s.to_string())
                        {
                            profile.technical_traits.push(s.to_string());
                        }
                    }
                }
                if let Some(arch) = output.insight_json.get("architecture_patterns")
                    && let Some(arr) = arch.as_array()
                {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            profile.architecture_hints.push(s.to_string());
                        }
                    }
                }
            }
            "domain" => {
                // Extract domain traits and terminology
                if let Some(traits) = output.insight_json.get("domain_traits")
                    && let Some(arr) = traits.as_array()
                {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            profile.domain_traits.push(s.to_string());
                        }
                    }
                }
                if let Some(terms) = output.insight_json.get("terminology")
                    && let Some(arr) = terms.as_array()
                {
                    for v in arr {
                        let term = crate::types::DomainTerm {
                            term: v
                                .get("term")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            definition: v
                                .get("meaning")
                                .or_else(|| v.get("definition"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            context: v
                                .get("evidence")
                                .or_else(|| v.get("context"))
                                .and_then(|v| v.as_str())
                                .map(String::from),
                        };
                        if !term.term.is_empty() {
                            profile.terminology.push(term);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
