//! Deep Research Types
//!
//! Core types for the Deep Research workflow.

use serde::{Deserialize, Serialize};

use crate::wiki::exhaustive::bottom_up::RelatedFile;

// =============================================================================
// Research Phase
// =============================================================================

/// Research iteration phase in Deep Research workflow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResearchPhase {
    /// First iteration: Research plan + initial findings
    Planning,
    /// Intermediate iterations: Deep dives on specific aspects
    Investigating { iteration: u8 },
    /// Final iteration: Comprehensive synthesis
    Synthesizing,
}

impl ResearchPhase {
    /// Determine phase from iteration number
    pub fn from_iteration(current: u8, total: u8) -> Self {
        match current {
            1 => Self::Planning,
            n if n >= total => Self::Synthesizing,
            n => Self::Investigating { iteration: n },
        }
    }

    /// Get section header for this phase
    pub fn section_header(&self) -> String {
        match self {
            Self::Planning => "## Research Plan".to_string(),
            Self::Investigating { iteration } => format!("## Research Update {}", iteration),
            Self::Synthesizing => "## Final Conclusion".to_string(),
        }
    }

    /// Check if this is the planning phase
    pub fn is_planning(&self) -> bool {
        matches!(self, Self::Planning)
    }

    /// Check if this is the synthesis phase
    pub fn is_synthesizing(&self) -> bool {
        matches!(self, Self::Synthesizing)
    }
}

// =============================================================================
// Research Iteration
// =============================================================================

/// Result of a single research iteration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchIteration {
    /// Phase of this iteration
    pub phase: ResearchPhase,

    /// Findings from this iteration (markdown)
    pub findings: String,

    /// New aspects discovered/covered in this iteration
    pub new_aspects: Vec<String>,

    /// Purpose statement (populated in synthesis)
    #[serde(default)]
    pub purpose: Option<String>,

    /// Full content (populated in synthesis)
    #[serde(default)]
    pub content: Option<String>,

    /// Diagram (populated in synthesis)
    #[serde(default)]
    pub diagram: Option<String>,

    /// Related files (populated in synthesis)
    #[serde(default)]
    pub related_files: Vec<RelatedFile>,
}

// =============================================================================
// Research Context
// =============================================================================

/// Accumulated context across research iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchContext {
    /// Original research topic (file path)
    pub topic: String,

    /// Findings from each iteration
    pub iterations: Vec<ResearchIteration>,

    /// All aspects covered across iterations (for anti-repetition)
    pub covered_aspects: Vec<String>,
}

impl ResearchContext {
    /// Create new research context
    pub fn new(topic: String) -> Self {
        Self {
            topic,
            iterations: Vec::new(),
            covered_aspects: Vec::new(),
        }
    }

    /// Add an iteration result to context
    pub fn add_iteration(&mut self, iteration: ResearchIteration) {
        // Track covered aspects
        for aspect in &iteration.new_aspects {
            if !self.is_covered(aspect) {
                self.covered_aspects.push(aspect.clone());
            }
        }
        self.iterations.push(iteration);
    }

    /// Check if an aspect has been covered (case-insensitive)
    pub fn is_covered(&self, aspect: &str) -> bool {
        let lower = aspect.to_lowercase();
        self.covered_aspects
            .iter()
            .any(|a| a.to_lowercase().contains(&lower) || lower.contains(&a.to_lowercase()))
    }

    /// Get all findings summarized for synthesis prompt
    pub fn summarize_findings(&self) -> String {
        self.iterations
            .iter()
            .map(|i| format!("{}\n\n{}", i.phase.section_header(), i.findings))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    /// Get covered aspects as comma-separated string
    pub fn covered_aspects_str(&self) -> String {
        if self.covered_aspects.is_empty() {
            "None yet".to_string()
        } else {
            self.covered_aspects.join(", ")
        }
    }

    /// Get the synthesis iteration (final result)
    pub fn get_synthesis(&self) -> Option<&ResearchIteration> {
        self.iterations
            .iter()
            .find(|i| matches!(i.phase, ResearchPhase::Synthesizing))
    }

    /// Get pending areas to investigate (from planning phase)
    pub fn pending_areas(&self) -> Vec<String> {
        // Extract from planning iteration if available
        self.iterations
            .iter()
            .find(|i| matches!(i.phase, ResearchPhase::Planning))
            .map(|i| {
                // Filter out covered aspects
                i.new_aspects
                    .iter()
                    .filter(|a| !self.is_covered(a))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

// =============================================================================
// Synthesis Result
// =============================================================================

/// Final synthesized documentation result
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SynthesisResult {
    /// Clear purpose statement
    pub purpose: String,

    /// Full markdown content
    pub content: String,

    /// Mermaid diagram (optional)
    pub diagram: Option<String>,

    /// Related files discovered during research
    pub related_files: Vec<RelatedFile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_phase_section_headers() {
        assert_eq!(ResearchPhase::Planning.section_header(), "## Research Plan");
        assert_eq!(
            ResearchPhase::Investigating { iteration: 2 }.section_header(),
            "## Research Update 2"
        );
        assert_eq!(
            ResearchPhase::Synthesizing.section_header(),
            "## Final Conclusion"
        );
    }

    #[test]
    fn test_research_context_covered_aspects() {
        let mut ctx = ResearchContext::new("test.rs".to_string());

        // Initially nothing covered
        assert!(!ctx.is_covered("architecture"));

        // Add aspects
        ctx.covered_aspects.push("Architecture".to_string());
        ctx.covered_aspects.push("Error Handling".to_string());

        // Case-insensitive matching
        assert!(ctx.is_covered("architecture"));
        assert!(ctx.is_covered("ARCHITECTURE"));
        assert!(ctx.is_covered("error handling"));

        // Partial matching
        assert!(ctx.is_covered("error"));

        // Not covered
        assert!(!ctx.is_covered("performance"));
    }

    #[test]
    fn test_summarize_findings() {
        let mut ctx = ResearchContext::new("test.rs".to_string());

        ctx.iterations.push(ResearchIteration {
            phase: ResearchPhase::Planning,
            findings: "Planning findings".to_string(),
            new_aspects: vec![],
            purpose: None,
            content: None,
            diagram: None,
            related_files: vec![],
        });

        ctx.iterations.push(ResearchIteration {
            phase: ResearchPhase::Investigating { iteration: 2 },
            findings: "Investigation findings".to_string(),
            new_aspects: vec![],
            purpose: None,
            content: None,
            diagram: None,
            related_files: vec![],
        });

        let summary = ctx.summarize_findings();
        assert!(summary.contains("## Research Plan"));
        assert!(summary.contains("Planning findings"));
        assert!(summary.contains("## Research Update 2"));
        assert!(summary.contains("Investigation findings"));
    }
}
