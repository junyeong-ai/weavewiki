//! Project Characterization
//!
//! Multi-agent, multi-turn analysis to discover project characteristics.
//! Outputs a ProjectProfile that guides all subsequent phases.
//!
//! ## Turn Structure
//!
//! - Turn 1: Structure, Dependency, Entry Point agents (parallel)
//! - Turn 2: Purpose, Technical, Domain agents (parallel, with Turn 1 context)
//! - Turn 3: Section Discovery agent (discovers domain-specific sections to extract)
//! - Synthesis: Merge all outputs into ProjectProfile

pub mod agents;
pub mod profile;
pub mod schemas;
pub mod synthesis;

use crate::ai::provider::SharedProvider;
use crate::config::{AnalysisMode, ModeConfig, ProjectScale};
use crate::storage::SharedDatabase;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
pub use profile::ProjectProfile;
use std::collections::HashSet;
use std::path::Path;

pub struct CharacterizationAnalyzer {
    project_root: std::path::PathBuf,
    mode: AnalysisMode,
    scale: ProjectScale,
    /// Mode configuration for analysis depth
    config: ModeConfig,
    /// LLM provider for agent interactions
    provider: SharedProvider,
    checkpoint: Option<CheckpointContext>,
}

impl CharacterizationAnalyzer {
    pub fn new(
        project_root: impl AsRef<Path>,
        mode: AnalysisMode,
        scale: ProjectScale,
        config: ModeConfig,
        provider: SharedProvider,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            mode,
            scale,
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

    /// Run characterization with checkpoint/resume support
    ///
    /// ## Refinement Depth
    ///
    /// When `char_refinement_rounds > 0`, additional refinement rounds are performed:
    /// - After initial synthesis, Turn 2 agents re-run with synthesized profile context
    /// - This enables deeper understanding for Large/Enterprise projects
    pub async fn run(&self) -> Result<ProjectProfile, WeaveError> {
        let refinement_depth = self.config.char_refinement_rounds;
        tracing::info!(
            "Characterization: Starting (mode={}, scale={}, refinement_depth={})",
            self.mode,
            self.scale,
            refinement_depth
        );

        // Load any previously completed agent outputs (for resume)
        let (completed_agents, prior_outputs) = self.load_checkpoint()?;

        if !completed_agents.is_empty() {
            tracing::info!(
                "Characterization: Resuming with {} completed agents: {:?}",
                completed_agents.len(),
                completed_agents
            );
        }

        // Collect file info from project
        let files = self.collect_file_info()?;

        // Check for flat structure - use fallback if too simple
        if Self::is_flat_structure(&files) && files.len() < 10 {
            return Ok(self.create_flat_project_profile(&files));
        }

        // === Turn 1: Structure, Dependency, EntryPoint (parallel) ===
        tracing::info!(
            "Characterization: Running Turn 1 agents (structure, dependency, entry_point)"
        );
        let turn1_outputs = self
            .run_turn1_agents(&completed_agents, &prior_outputs, &files)
            .await?;

        // === Turn 2: Purpose, Technical, Domain (parallel, with Turn 1 context) ===
        tracing::info!("Characterization: Running Turn 2 agents (purpose, technical, domain)");
        let mut turn2_outputs = self
            .run_turn2_agents(&completed_agents, &turn1_outputs, &files)
            .await?;

        // === Turn 3: Section Discovery (with Turn 1+2 context) ===
        // Only run Turn 3 when enabled (typically for Large/Enterprise projects)
        let turn3_outputs = if self.config.char_turn3_enabled {
            tracing::info!("Characterization: Running Turn 3 agent (section_discovery)");
            let all_prior_outputs: Vec<AgentOutput> = turn1_outputs
                .iter()
                .chain(turn2_outputs.iter())
                .cloned()
                .collect();
            self.run_turn3_agents(&completed_agents, &all_prior_outputs, &files)
                .await?
        } else {
            tracing::debug!("Characterization: Skipping Turn 3 (disabled for this mode/scale)");
            vec![]
        };

        // === Synthesis: Merge all outputs into ProjectProfile ===
        let project_name = self
            .project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let synthesizer =
            synthesis::ProfileSynthesis::new(project_name.clone(), self.scale, self.mode);

        // Synthesize profile from Turn 1+2 outputs
        let mut profile = if self.config.char_turn3_enabled
            && matches!(self.scale, ProjectScale::Large | ProjectScale::Enterprise)
        {
            tracing::info!("Characterization: Running enhanced synthesis for large project");
            synthesizer
                .synthesize_with_llm(turn1_outputs.clone(), turn2_outputs.clone(), &self.provider)
                .await?
        } else {
            synthesizer.synthesize(turn1_outputs.clone(), turn2_outputs.clone())?
        };

        // Merge Turn 3 section discovery into profile
        self.merge_section_discovery(&mut profile, &turn3_outputs)?;

        // === Refinement Rounds (if char_refinement_rounds > 0) ===
        for round in 0..refinement_depth {
            tracing::info!(
                "Characterization: Running refinement round {}/{} with synthesized context",
                round + 1,
                refinement_depth
            );

            // Re-run Turn 2 agents with prior insights for deeper analysis
            let refined_outputs = self
                .run_turn2_refinement(&files, &turn1_outputs, &turn2_outputs)
                .await?;

            // Merge refined outputs into turn2_outputs by combining insight_json
            for refined in refined_outputs {
                if let Some(existing) = turn2_outputs
                    .iter_mut()
                    .find(|o| o.agent_name == refined.agent_name)
                {
                    // Merge insight_json by combining arrays or objects
                    if let (Some(existing_obj), Some(refined_obj)) = (
                        existing.insight_json.as_object_mut(),
                        refined.insight_json.as_object(),
                    ) {
                        for (key, value) in refined_obj {
                            if let Some(existing_arr) =
                                existing_obj.get_mut(key).and_then(|v| v.as_array_mut())
                            {
                                if let Some(refined_arr) = value.as_array() {
                                    existing_arr.extend(refined_arr.clone());
                                }
                            } else {
                                existing_obj.insert(key.clone(), value.clone());
                            }
                        }
                    }
                    // Average confidence
                    existing.confidence = (existing.confidence + refined.confidence) / 2.0;
                } else {
                    turn2_outputs.push(refined);
                }
            }

            // Re-synthesize with refined outputs
            let new_synthesizer =
                synthesis::ProfileSynthesis::new(project_name.clone(), self.scale, self.mode);
            profile = if self.config.char_turn3_enabled
                && matches!(self.scale, ProjectScale::Large | ProjectScale::Enterprise)
            {
                new_synthesizer
                    .synthesize_with_llm(
                        turn1_outputs.clone(),
                        turn2_outputs.clone(),
                        &self.provider,
                    )
                    .await?
            } else {
                new_synthesizer.synthesize(turn1_outputs.clone(), turn2_outputs.clone())?
            };

            // Re-merge section discovery
            self.merge_section_discovery(&mut profile, &turn3_outputs)?;

            // Update characterization turns count
            profile.characterization_turns += 1;
        }

        // Store the final profile if checkpointing is enabled
        self.store_project_profile(&profile)?;

        tracing::info!(
            "Characterization: Complete (purposes={}, traits={}, turns={})",
            profile.purposes.len(),
            profile.technical_traits.len(),
            profile.characterization_turns
        );
        Ok(profile)
    }

    /// Run Turn 2 agents with prior insights (for refinement)
    ///
    /// Takes the existing Turn 1 and Turn 2 outputs as prior insights,
    /// allowing agents to build upon and refine earlier analysis.
    async fn run_turn2_refinement(
        &self,
        files: &[FileInfo],
        turn1_outputs: &[AgentOutput],
        turn2_outputs: &[AgentOutput],
    ) -> Result<Vec<AgentOutput>, WeaveError> {
        use agents::{PurposeAgent, TechnicalAgent, TerminologyAgent};

        // Build prior insights from all previous agent outputs
        let prior_insights: Vec<AgentOutput> = turn1_outputs
            .iter()
            .chain(turn2_outputs.iter())
            .cloned()
            .collect();

        let context = CharacterizationContext {
            project_root: self.project_root.clone(),
            files: files.to_vec(),
            prior_insights,
            provider: self.provider.clone(),
        };

        // Run Purpose, Technical, Terminology agents in parallel with enriched context
        let (purpose_result, technical_result, terminology_result) = tokio::join!(
            PurposeAgent.run(&context),
            TechnicalAgent.run(&context),
            TerminologyAgent.run(&context),
        );

        let mut outputs = Vec::new();

        if let Ok(output) = purpose_result {
            outputs.push(output);
        }
        if let Ok(output) = technical_result {
            outputs.push(output);
        }
        if let Ok(output) = terminology_result {
            outputs.push(output);
        }

        Ok(outputs)
    }

    /// Collect file information from project root
    fn collect_file_info(&self) -> Result<Vec<FileInfo>, WeaveError> {
        use crate::analyzer::scanner::FileScanner;

        let scanner = FileScanner::source_files(&self.project_root);
        let scanned_files = scanner.scan()?;
        let mut files = Vec::new();

        for entry in scanned_files {
            let path = entry
                .path
                .strip_prefix(&self.project_root)
                .unwrap_or(&entry.path)
                .to_string_lossy()
                .to_string();

            let language = entry
                .extension
                .as_deref()
                .map(|ext| match ext {
                    "rs" => "Rust",
                    "py" => "Python",
                    "ts" | "tsx" => "TypeScript",
                    "js" | "jsx" => "JavaScript",
                    "go" => "Go",
                    "java" => "Java",
                    "kt" => "Kotlin",
                    "rb" => "Ruby",
                    "c" | "cpp" | "h" | "hpp" => "C/C++",
                    _ => ext,
                })
                .map(String::from);

            // Rough line count estimation (avoid reading all files)
            let line_count = (entry.size / 40) as usize; // ~40 chars per line average

            files.push(FileInfo {
                path,
                language,
                line_count,
            });
        }

        Ok(files)
    }

    /// Run Turn 1 agents in parallel
    async fn run_turn1_agents(
        &self,
        completed: &HashSet<String>,
        prior_outputs: &[AgentOutput],
        files: &[FileInfo],
    ) -> Result<Vec<AgentOutput>, WeaveError> {
        use agents::{DependencyAgent, EntryPointAgent, StructureAgent};

        let context = CharacterizationContext {
            project_root: self.project_root.clone(),
            files: files.to_vec(),
            prior_insights: vec![],
            provider: self.provider.clone(),
        };

        let mut outputs: Vec<AgentOutput> = prior_outputs
            .iter()
            .filter(|o| o.turn == 1)
            .cloned()
            .collect();

        // Run agents that haven't completed yet
        let structure_agent = StructureAgent;
        let dependency_agent = DependencyAgent;
        let entry_point_agent = EntryPointAgent;

        // Run in parallel using tokio::join!
        let (structure_result, dependency_result, entry_result) = tokio::join!(
            async {
                if completed.contains(structure_agent.name()) {
                    None
                } else {
                    Some(structure_agent.run(&context).await)
                }
            },
            async {
                if completed.contains(dependency_agent.name()) {
                    None
                } else {
                    Some(dependency_agent.run(&context).await)
                }
            },
            async {
                if completed.contains(entry_point_agent.name()) {
                    None
                } else {
                    Some(entry_point_agent.run(&context).await)
                }
            }
        );

        // Collect results and checkpoint
        if let Some(result) = structure_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }
        if let Some(result) = dependency_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }
        if let Some(result) = entry_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }

        tracing::debug!(
            "Characterization: Turn 1 complete with {} outputs",
            outputs.len()
        );
        Ok(outputs)
    }

    /// Run Turn 2 agents in parallel (with Turn 1 context)
    async fn run_turn2_agents(
        &self,
        completed: &HashSet<String>,
        turn1_outputs: &[AgentOutput],
        files: &[FileInfo],
    ) -> Result<Vec<AgentOutput>, WeaveError> {
        use agents::{PurposeAgent, TechnicalAgent, TerminologyAgent};

        let context = CharacterizationContext {
            project_root: self.project_root.clone(),
            files: files.to_vec(),
            prior_insights: turn1_outputs.to_vec(),
            provider: self.provider.clone(),
        };

        let mut outputs: Vec<AgentOutput> = vec![];

        let purpose_agent = PurposeAgent;
        let technical_agent = TechnicalAgent;
        let terminology_agent = TerminologyAgent;

        // Run in parallel
        let (purpose_result, technical_result, terminology_result) = tokio::join!(
            async {
                if completed.contains(purpose_agent.name()) {
                    None
                } else {
                    Some(purpose_agent.run(&context).await)
                }
            },
            async {
                if completed.contains(technical_agent.name()) {
                    None
                } else {
                    Some(technical_agent.run(&context).await)
                }
            },
            async {
                if completed.contains(terminology_agent.name()) {
                    None
                } else {
                    Some(terminology_agent.run(&context).await)
                }
            }
        );

        // Collect results and checkpoint
        if let Some(result) = purpose_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }
        if let Some(result) = technical_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }
        if let Some(result) = terminology_result {
            let output = result?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }

        tracing::debug!(
            "Characterization: Turn 2 complete with {} outputs",
            outputs.len()
        );
        Ok(outputs)
    }

    /// Run Turn 3 agents (with Turn 1+2 context)
    async fn run_turn3_agents(
        &self,
        completed: &HashSet<String>,
        prior_outputs: &[AgentOutput],
        files: &[FileInfo],
    ) -> Result<Vec<AgentOutput>, WeaveError> {
        use agents::SectionDiscoveryAgent;

        let context = CharacterizationContext {
            project_root: self.project_root.clone(),
            files: files.to_vec(),
            prior_insights: prior_outputs.to_vec(),
            provider: self.provider.clone(),
        };

        let mut outputs: Vec<AgentOutput> = vec![];

        let section_discovery_agent = SectionDiscoveryAgent;

        if !completed.contains(section_discovery_agent.name()) {
            let output = section_discovery_agent.run(&context).await?;
            self.store_agent_output(&output)?;
            outputs.push(output);
        }

        tracing::debug!(
            "Characterization: Turn 3 complete with {} outputs",
            outputs.len()
        );
        Ok(outputs)
    }

    /// Merge section discovery output into profile
    fn merge_section_discovery(
        &self,
        profile: &mut ProjectProfile,
        turn3_outputs: &[AgentOutput],
    ) -> Result<(), WeaveError> {
        use crate::wiki::exhaustive::types::Importance;
        use profile::{DynamicSection, SectionContentType};

        for output in turn3_outputs {
            if output.agent_name == "section_discovery"
                && let Some(sections) = output
                    .insight_json
                    .get("sections")
                    .and_then(|v| v.as_array())
            {
                for section in sections {
                    let name = section
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let description = section
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let content_type_str = section
                        .get("content_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("freeform");
                    let importance_str = section
                        .get("importance")
                        .and_then(|v| v.as_str())
                        .unwrap_or("medium");

                    let content_type = match content_type_str {
                        "state_transitions" => SectionContentType::StateTransitions,
                        "flow" => SectionContentType::Flow,
                        "rules" => SectionContentType::Rules,
                        "data_transform" => SectionContentType::DataTransform,
                        "api_contract" => SectionContentType::ApiContract,
                        "configuration" => SectionContentType::Configuration,
                        _ => SectionContentType::Freeform,
                    };

                    let importance = match importance_str {
                        "critical" => Importance::Critical,
                        "high" => Importance::High,
                        "low" => Importance::Low,
                        _ => Importance::Medium,
                    };

                    let extraction_hints: Vec<String> = section
                        .get("extraction_hints")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    let file_patterns: Vec<String> = section
                        .get("file_patterns")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    if !name.is_empty() {
                        profile.dynamic_sections.push(DynamicSection {
                            name,
                            description,
                            content_type,
                            extraction_hints,
                            importance,
                            file_patterns,
                        });
                    }
                }
            }
        }

        tracing::info!(
            "Characterization: Merged {} dynamic sections into profile",
            profile.dynamic_sections.len()
        );
        Ok(())
    }

    /// Load checkpoint data from database (completed agents and their outputs)
    fn load_checkpoint(&self) -> Result<(HashSet<String>, Vec<AgentOutput>), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok((HashSet::new(), vec![]));
        };

        // Load completed agents
        let completed = ctx.db.get_completed_agents(&ctx.session_id)?;
        let completed_set: HashSet<String> = completed.into_iter().map(|(name, _)| name).collect();

        // Load agent outputs for Turn 2 context (convert from generic AgentInsight)
        let insights = ctx.db.load_agent_insights(&ctx.session_id)?;
        let outputs: Vec<AgentOutput> = insights
            .into_iter()
            .map(|i| AgentOutput {
                agent_name: i.agent_name,
                turn: i.turn,
                insight_json: i.insight_json,
                confidence: i.confidence,
            })
            .collect();

        Ok((completed_set, outputs))
    }

    /// Store an agent output to the database checkpoint
    fn store_agent_output(&self, output: &AgentOutput) -> Result<(), WeaveError> {
        use crate::storage::database::AgentInsight;

        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        // Convert to generic AgentInsight
        let insight = AgentInsight {
            agent_name: output.agent_name.clone(),
            turn: output.turn,
            insight_json: output.insight_json.clone(),
            confidence: output.confidence,
        };
        ctx.db.store_agent_insight(&ctx.session_id, &insight)?;
        tracing::debug!(
            "Characterization: Checkpointed agent '{}' (turn {})",
            output.agent_name,
            output.turn
        );

        Ok(())
    }

    /// Store the final project profile
    fn store_project_profile(&self, profile: &ProjectProfile) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        // Serialize profile to JSON for generic storage
        let profile_json = serde_json::to_value(profile)?;
        ctx.db
            .store_session_profile(&ctx.session_id, &profile_json)?;
        tracing::debug!("Characterization: Stored project profile checkpoint");

        Ok(())
    }

    /// Detect if project has flat structure (T084)
    ///
    /// A project is considered "flat" if:
    /// - All source files are at root level (no subdirectories)
    /// - Very few unique directories (< 3)
    /// - No standard structure patterns (src/, lib/, etc.)
    fn is_flat_structure(files: &[FileInfo]) -> bool {
        if files.is_empty() {
            return true;
        }

        // Check directory depth distribution
        let depths: Vec<usize> = files.iter().map(|f| f.path.matches('/').count()).collect();

        let max_depth = depths.iter().max().copied().unwrap_or(0);
        let avg_depth: f32 = depths.iter().sum::<usize>() as f32 / depths.len() as f32;

        // Flat if max depth <= 1 and average is very low
        if max_depth <= 1 && avg_depth < 0.5 {
            return true;
        }

        // Check for standard structure patterns
        let has_src = files.iter().any(|f| f.path.starts_with("src/"));
        let has_lib = files.iter().any(|f| f.path.starts_with("lib/"));
        let has_app = files.iter().any(|f| f.path.starts_with("app/"));
        let has_pkg = files.iter().any(|f| f.path.starts_with("pkg/"));

        if has_src || has_lib || has_app || has_pkg {
            return false; // Has standard structure
        }

        // Count unique top-level directories
        let top_dirs: HashSet<&str> = files
            .iter()
            .filter_map(|f| f.path.split('/').next())
            .filter(|d| !d.contains('.')) // Exclude files
            .collect();

        top_dirs.len() < 3
    }

    /// Create a fallback profile for flat/simple projects (T084)
    fn create_flat_project_profile(&self, files: &[FileInfo]) -> ProjectProfile {
        tracing::info!(
            "Characterization: Using flat directory fallback for simple project structure"
        );

        let mut profile = ProjectProfile::new(
            self.project_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            self.scale,
            self.mode,
        );

        // Set basic purpose based on file types
        let has_main = files.iter().any(|f| {
            f.path.contains("main.") || f.path.contains("index.") || f.path.contains("app.")
        });

        if has_main {
            profile.purposes = vec!["Simple application".to_string()];
        } else {
            profile.purposes = vec!["Script collection".to_string()];
        }

        // Set organization style to Flat
        profile.organization_style = profile::OrganizationStyle::Flat;

        // Detect language from files
        let languages: HashSet<&str> = files.iter().filter_map(|f| f.language.as_deref()).collect();

        if !languages.is_empty() {
            profile.technical_traits = languages.iter().map(|l| format!("{} code", l)).collect();
        }

        profile.characterization_turns = 0; // Mark as fallback (no LLM calls)

        profile
    }
}

/// Trait for characterization agents
#[async_trait::async_trait]
pub trait CharacterizationAgent: Send + Sync {
    /// Agent identifier
    fn name(&self) -> &str;

    /// Turn number this agent runs in (1 or 2)
    fn turn(&self) -> u8;

    /// Run the agent with context
    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError>;
}

/// Context passed to characterization agents
pub struct CharacterizationContext {
    /// Project root path
    pub project_root: std::path::PathBuf,
    /// File list with metadata
    pub files: Vec<FileInfo>,
    /// Previous turn insights (empty for Turn 1)
    pub prior_insights: Vec<AgentOutput>,
    /// LLM provider
    pub provider: SharedProvider,
}

/// File information for context
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: String,
    pub language: Option<String>,
    pub line_count: usize,
}

/// Output from a characterization agent
#[derive(Debug, Clone)]
pub struct AgentOutput {
    pub agent_name: String,
    pub turn: u8,
    pub insight_json: serde_json::Value,
    pub confidence: f32,
}
