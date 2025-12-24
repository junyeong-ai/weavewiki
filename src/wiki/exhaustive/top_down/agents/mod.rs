//! Top-Down Agents

pub mod architecture;
pub mod domain;
pub mod flow;
pub mod helpers;
pub mod risk;

pub use architecture::ArchitectureAgent;
pub use domain::DomainAgent;
pub use flow::FlowAgent;
pub use helpers::{TopDownAgentConfig, run_top_down_agent};
pub use risk::RiskAgent;

use super::TopDownContext;
use super::insights::ProjectInsight;
use crate::types::error::WeaveError;

/// Trait for top-down agents
#[async_trait::async_trait]
pub trait TopDownAgent: Send + Sync {
    /// Agent name
    fn name(&self) -> &str;

    /// Run analysis and return project insight
    async fn run(&self, context: &TopDownContext) -> Result<ProjectInsight, WeaveError>;
}
