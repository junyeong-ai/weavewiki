//! Top-Down Analysis
//!
//! Project-level analysis using dynamically selected agents.
//! Agents are chosen based on ProjectProfile.purposes and technical_traits.

pub mod agents;
pub mod insights;

use crate::ai::provider::SharedProvider;
use crate::config::{ModeConfig, ProjectScale};
use crate::storage::SharedDatabase;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::bottom_up::FileInsight;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
use agents::{ArchitectureAgent, DomainAgent, FlowAgent, RiskAgent, TopDownAgent};
use insights::ProjectInsight;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Orchestrator for top-down analysis with checkpoint/resume
pub struct TopDownAnalyzer {
    project_root: std::path::PathBuf,
    profile: Arc<ProjectProfile>,
    config: ModeConfig,
    provider: SharedProvider,
    checkpoint: Option<CheckpointContext>,
}

impl TopDownAnalyzer {
    pub fn new(
        project_root: impl AsRef<Path>,
        profile: Arc<ProjectProfile>,
        config: ModeConfig,
        provider: SharedProvider,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            profile,
            config,
            provider,
            checkpoint: None,
        }
    }

    /// Enable checkpoint/resume with database storage
    pub fn with_checkpoint(mut self, db: SharedDatabase, session_id: String) -> Self {
        self.checkpoint = Some(CheckpointContext::new(db, session_id));
        self
    }

    /// Run top-down analysis with selected agents
    pub async fn run(
        &self,
        file_insights: &[FileInsight],
    ) -> Result<Vec<ProjectInsight>, WeaveError> {
        tracing::info!("Top-Down: Starting contextual top-down analysis");

        // Load completed agents (for resume)
        let completed_agents = self.load_completed_agents()?;
        if !completed_agents.is_empty() {
            tracing::info!(
                "Top-Down: Resuming with {} completed agents",
                completed_agents.len()
            );
        }

        // Select agents based on profile
        let selected_agents = self.select_agents();
        let remaining: Vec<&str> = selected_agents
            .iter()
            .filter(|a| !completed_agents.contains(**a))
            .copied()
            .collect();

        tracing::info!(
            "Top-Down: Selected {} agents, {} remaining: {:?}",
            selected_agents.len(),
            remaining.len(),
            remaining
        );

        // Build context for agents (Arc-wrapped for sharing across tasks)
        let context = Arc::new(TopDownContext {
            project_root: self.project_root.clone(),
            profile: self.profile.clone(),
            file_insights: file_insights.to_vec(),
            provider: self.provider.clone(),
        });

        // Run agents in parallel using tokio::join!
        let run_arch = remaining.contains(&"architecture");
        let run_risk = remaining.contains(&"risk");
        let run_flow = remaining.contains(&"flow");
        let run_domain = remaining.contains(&"domain");

        let (arch_result, risk_result, flow_result, domain_result) = tokio::join!(
            async {
                if run_arch {
                    Some(ArchitectureAgent.run(&context).await)
                } else {
                    None
                }
            },
            async {
                if run_risk {
                    Some(RiskAgent.run(&context).await)
                } else {
                    None
                }
            },
            async {
                if run_flow {
                    Some(FlowAgent.run(&context).await)
                } else {
                    None
                }
            },
            async {
                if run_domain {
                    Some(DomainAgent.run(&context).await)
                } else {
                    None
                }
            }
        );

        // Collect results and checkpoint
        let mut insights = Vec::new();

        if let Some(result) = arch_result {
            match result {
                Ok(insight) => {
                    self.store_agent_insight("architecture", &insight)?;
                    insights.push(insight);
                    tracing::debug!("Top-Down: architecture agent complete");
                }
                Err(e) => tracing::warn!("Top-Down: architecture agent failed: {}", e),
            }
        }

        if let Some(result) = risk_result {
            match result {
                Ok(insight) => {
                    self.store_agent_insight("risk", &insight)?;
                    insights.push(insight);
                    tracing::debug!("Top-Down: risk agent complete");
                }
                Err(e) => tracing::warn!("Top-Down: risk agent failed: {}", e),
            }
        }

        if let Some(result) = flow_result {
            match result {
                Ok(insight) => {
                    self.store_agent_insight("flow", &insight)?;
                    insights.push(insight);
                    tracing::debug!("Top-Down: flow agent complete");
                }
                Err(e) => tracing::warn!("Top-Down: flow agent failed: {}", e),
            }
        }

        if let Some(result) = domain_result {
            match result {
                Ok(insight) => {
                    self.store_agent_insight("domain", &insight)?;
                    insights.push(insight);
                    tracing::debug!("Top-Down: domain agent complete");
                }
                Err(e) => tracing::warn!("Top-Down: domain agent failed: {}", e),
            }
        }

        tracing::info!(
            "Top-Down: Top-down analysis complete ({} insights)",
            insights.len()
        );
        Ok(insights)
    }

    /// Select agents based on ProjectProfile
    fn select_agents(&self) -> Vec<&'static str> {
        let mut agents = vec!["architecture"]; // Always include

        // Add risk agent for complex projects
        if self.profile.scale != ProjectScale::Small {
            agents.push("risk");
        }

        // Add flow agent if async patterns detected
        if !self.profile.technical_traits.is_empty() {
            agents.push("flow");
        }

        // Add domain agent if domain traits detected
        if !self.profile.domain_traits.is_empty() {
            agents.push("domain");
        }

        // Limit by config
        agents.truncate(self.config.top_down_max_agents);
        agents
    }

    /// Load completed agent names (for resume)
    fn load_completed_agents(&self) -> Result<HashSet<String>, WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(HashSet::new());
        };

        // Check for completed top-down agents in module_summaries
        let conn = ctx.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT module_path FROM module_summaries WHERE session_id = ?1 AND module_path LIKE 'top_down:%'",
        )?;

        let agents: HashSet<String> = stmt
            .query_map([&ctx.session_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|p| p.strip_prefix("top_down:").map(String::from))
            .collect();

        Ok(agents)
    }

    /// Store agent insight checkpoint
    fn store_agent_insight(
        &self,
        agent_name: &str,
        insight: &ProjectInsight,
    ) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        let id = uuid::Uuid::new_v4().to_string();
        let sections = serde_json::to_string(insight)?;
        let now = chrono::Utc::now().to_rfc3339();

        // Use architecture_pattern as purpose description, or agent name as fallback
        let purpose = insight
            .architecture_pattern
            .clone()
            .unwrap_or_else(|| insight.agent.clone());
        let module_path = format!("top_down:{}", agent_name);
        let role = "TopDownAgent".to_string();

        ctx.db.execute(
            "INSERT INTO module_summaries
             (id, session_id, module_path, module_name, role, purpose, sections, synthesized_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                &id,
                &ctx.session_id,
                &module_path,
                &agent_name.to_string(),
                &role,
                &purpose,
                &sections,
                &now,
            ],
        )?;

        tracing::debug!("Top-Down: Checkpointed agent insight for {}", agent_name);
        Ok(())
    }
}

/// Context passed to top-down agents
pub struct TopDownContext {
    /// Project root path for file access
    pub project_root: std::path::PathBuf,
    /// Project profile with characteristics
    pub profile: Arc<ProjectProfile>,
    /// File insights from bottom-up analysis
    pub file_insights: Vec<FileInsight>,
    /// LLM provider
    pub provider: SharedProvider,
}
