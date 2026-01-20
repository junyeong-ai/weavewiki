//! Phase-specific Provider Factory
//!
//! Creates LLM providers with phase-appropriate settings.
//! Each pipeline phase can have different model, timeout, and temperature.

use std::sync::Arc;

use crate::ai::{LlmProvider, SharedBudget};
use crate::config::{Config, PhaseProviderConfig};
use crate::types::Result;

#[cfg(feature = "claude-agent")]
use crate::ai::ClaudeAgentProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    ProjectDetection,
    ConventionInference,
    ConstraintExtraction,
    Generation,
    Verification,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectDetection => "project_detection",
            Self::ConventionInference => "convention_inference",
            Self::ConstraintExtraction => "constraint_extraction",
            Self::Generation => "generation",
            Self::Verification => "verification",
        }
    }
}

pub struct PhaseProviderFactory {
    config: Config,
    budget: SharedBudget,
}

impl PhaseProviderFactory {
    pub fn new(config: Config, budget: SharedBudget) -> Self {
        Self { config, budget }
    }

    #[cfg(feature = "claude-agent")]
    pub async fn create(&self, phase: Phase) -> Result<Arc<dyn LlmProvider>> {
        let phase_config = self.get_phase_config(phase);

        tracing::debug!(
            phase = phase.as_str(),
            model = %phase_config.model,
            timeout = phase_config.timeout_secs,
            "Creating phase provider"
        );

        let provider = ClaudeAgentProvider::from_env(&phase_config.model).await?;
        Ok(Arc::new(provider))
    }

    #[cfg(not(feature = "claude-agent"))]
    pub async fn create(&self, _phase: Phase) -> Result<Arc<dyn LlmProvider>> {
        Err(crate::types::ClaudegenError::Config(
            "No LLM provider available. Enable 'claude-agent' feature.".into(),
        ))
    }

    #[cfg(feature = "claude-agent")]
    pub async fn create_default(&self) -> Result<Arc<dyn LlmProvider>> {
        let provider = ClaudeAgentProvider::from_env(&self.config.llm.default_model).await?;
        Ok(Arc::new(provider))
    }

    #[cfg(not(feature = "claude-agent"))]
    pub async fn create_default(&self) -> Result<Arc<dyn LlmProvider>> {
        Err(crate::types::ClaudegenError::Config(
            "No LLM provider available. Enable 'claude-agent' feature.".into(),
        ))
    }

    pub fn get_phase_config(&self, phase: Phase) -> PhaseProviderConfig {
        let default_timeout = self.config.llm.timeout_secs;

        if let Some(phases) = &self.config.llm.phases {
            let model = match phase {
                Phase::ProjectDetection => phases.project_detection.clone(),
                Phase::ConventionInference => phases.convention_inference.clone(),
                Phase::ConstraintExtraction => phases.constraint_extraction.clone(),
                Phase::Generation => phases.generation.clone(),
                Phase::Verification => phases.verification.clone(),
            };
            PhaseProviderConfig::new(&model, default_timeout)
        } else {
            PhaseProviderConfig::new(&self.config.llm.default_model, default_timeout)
        }
    }

    pub fn budget(&self) -> &SharedBudget {
        &self.budget
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::create_shared_budget;
    use crate::config::PhaseModels;

    #[test]
    fn test_phase_as_str() {
        assert_eq!(Phase::ProjectDetection.as_str(), "project_detection");
        assert_eq!(Phase::ConstraintExtraction.as_str(), "constraint_extraction");
    }

    #[test]
    fn test_get_phase_config() {
        let config = Config::default();
        let budget = create_shared_budget(1_000_000);
        let factory = PhaseProviderFactory::new(config, budget);

        // When phases are not configured, all phases use the default model
        let detection_config = factory.get_phase_config(Phase::ProjectDetection);
        assert!(detection_config.model.contains("sonnet"));

        let extraction_config = factory.get_phase_config(Phase::ConstraintExtraction);
        assert!(extraction_config.model.contains("sonnet"));

        // Test with custom phases
        let mut config_with_phases = Config::default();
        config_with_phases.llm.phases = Some(PhaseModels::default());
        let factory2 = PhaseProviderFactory::new(config_with_phases, create_shared_budget(1_000_000));

        let detection_config2 = factory2.get_phase_config(Phase::ProjectDetection);
        assert!(detection_config2.model.contains("haiku"));
    }
}
