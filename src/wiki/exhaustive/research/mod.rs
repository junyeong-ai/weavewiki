//! Deep Research Workflow
//!
//! Multi-turn investigation workflow for comprehensive code documentation.
//! Based on DeepWiki's proven Deep Research pattern.
//!
//! ## Workflow Phases
//!
//! ```text
//! ┌────────────────┐    ┌────────────────┐    ┌────────────────┐
//! │   Planning     │───▶│  Investigating │───▶│  Synthesizing  │
//! │  (Iteration 1) │    │  (Iterations   │    │   (Final)      │
//! │                │    │     2..N-1)    │    │                │
//! │ • Research Plan│    │ • Updates      │    │ • Conclusion   │
//! │ • Key Aspects  │    │ • New Insights │    │ • Full Docs    │
//! │ • Next Steps   │    │ • No Repetition│    │ • Diagrams     │
//! └────────────────┘    └────────────────┘    └────────────────┘
//! ```
//!
//! ## Design Principles
//!
//! 1. **Focus Enforcement**: Stay on topic, prevent drift
//! 2. **No Repetition**: Track covered aspects, only add new insights
//! 3. **Progressive Depth**: Each iteration goes deeper
//! 4. **Synthesis Focus**: Final iteration produces complete documentation
//!
//! ## Integration
//!
//! Deep Research is integrated into `FileAnalyzer` for Important/Core tiers.
//! Use `ProcessingTier::uses_deep_research()` to check if a tier supports it.

pub mod prompts;
mod types;

pub use prompts::build_research_prompt;
pub use types::{ResearchContext, ResearchIteration, ResearchPhase, SynthesisResult};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_phase_from_iteration() {
        // 4 iterations (Core)
        assert!(matches!(
            ResearchPhase::from_iteration(1, 4),
            ResearchPhase::Planning
        ));
        assert!(matches!(
            ResearchPhase::from_iteration(2, 4),
            ResearchPhase::Investigating { iteration: 2 }
        ));
        assert!(matches!(
            ResearchPhase::from_iteration(3, 4),
            ResearchPhase::Investigating { iteration: 3 }
        ));
        assert!(matches!(
            ResearchPhase::from_iteration(4, 4),
            ResearchPhase::Synthesizing
        ));

        // 3 iterations (Important)
        assert!(matches!(
            ResearchPhase::from_iteration(1, 3),
            ResearchPhase::Planning
        ));
        assert!(matches!(
            ResearchPhase::from_iteration(2, 3),
            ResearchPhase::Investigating { iteration: 2 }
        ));
        assert!(matches!(
            ResearchPhase::from_iteration(3, 3),
            ResearchPhase::Synthesizing
        ));
    }

    #[test]
    fn test_research_context() {
        let mut ctx = ResearchContext::new("test.rs".to_string());
        assert!(ctx.iterations.is_empty());
        assert!(!ctx.is_covered("architecture"));

        // Add planning iteration
        ctx.add_iteration(ResearchIteration {
            phase: ResearchPhase::Planning,
            findings: "Initial findings".to_string(),
            new_aspects: vec!["architecture".to_string(), "error_handling".to_string()],
            purpose: None,
            content: None,
            diagram: None,
            related_files: vec![],
        });

        assert!(ctx.is_covered("architecture"));
        assert!(ctx.is_covered("ARCHITECTURE")); // case-insensitive
        assert!(!ctx.is_covered("performance"));
    }
}
