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

        // Use tier-based model selection:
        // - Fast (Haiku): detection, convention inference
        // - Performance (Opus): constraint extraction (critical analysis)
        // - Balanced (Sonnet): generation, verification
        let model = self.config.llm.model_for_phase(phase.as_str());
        PhaseProviderConfig::new(model, default_timeout)
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

    #[test]
    fn test_phase_as_str() {
        assert_eq!(Phase::ProjectDetection.as_str(), "project_detection");
        assert_eq!(Phase::ConstraintExtraction.as_str(), "constraint_extraction");
    }

    #[test]
    fn test_default_config_uses_default_model() {
        let config = Config::default();
        let budget = create_shared_budget(1_000_000);
        let factory = PhaseProviderFactory::new(config, budget);

        // Without tier config, all phases use default_model
        let detection = factory.get_phase_config(Phase::ProjectDetection);
        let extraction = factory.get_phase_config(Phase::ConstraintExtraction);
        let generation = factory.get_phase_config(Phase::Generation);

        assert_eq!(detection.model, extraction.model);
        assert_eq!(extraction.model, generation.model);
        assert!(detection.model.contains("sonnet"));
    }

    #[test]
    fn test_tier_config_overrides() {
        let mut config = Config::default();
        config.llm.fast_model = Some("claude-haiku-4-5-20251001".into());
        config.llm.performance_model = Some("claude-opus-4-5-20251101".into());
        let budget = create_shared_budget(1_000_000);
        let factory = PhaseProviderFactory::new(config, budget);

        let detection = factory.get_phase_config(Phase::ProjectDetection);
        assert!(detection.model.contains("haiku"));

        let extraction = factory.get_phase_config(Phase::ConstraintExtraction);
        assert!(extraction.model.contains("opus"));

        let generation = factory.get_phase_config(Phase::Generation);
        assert!(generation.model.contains("sonnet"));
    }
}
