//! Adaptive Pipeline - Project-Type Agnostic Generation
//!
//! A new generation pipeline that:
//! 1. Detects project type automatically
//! 2. Analyzes monorepo structure if applicable
//! 3. Infers conventions using few-shot learning
//! 4. Extracts hidden constraints (Tier 3 value)
//! 5. Routes to appropriate output strategy
//! 6. Generates filtered, project-consistent output

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::fs;
use tokio::sync::OnceCell;

use crate::ai::{LlmProvider, with_timeout};
use crate::config::Config;
use crate::types::{Agent, AgentModel, Plugin, PluginManifest, ProjectMemory, Result, Rule, Skill};

use super::analysis::{
    AnalysisSynthesizer, AstEnricher, DeepAnalysisResult, DeepAnalyzer, SynthesizedAnalysis,
    multi_agent::{AnalysisContext, MultiAgentAnalyzer},
};
use super::context::VerifiedFileRegistry;

use super::context::ClaudegenContext;
use super::enrichment::{EnrichedPlan, EnrichmentEngine};
use super::generation::path_rules::{ClaudeMdGenerator, PathRulesGenerator};
use super::phases::{
    constraint_extraction::{self, ExtractedConstraints},
    convention_inference::{self, InferredConventions},
    monorepo_analyzer::{self, MonorepoAnalysis},
    output_router::{self, OutputPlan},
    project_detection::{self, ProjectDetection},
};
use super::reference_extractor::ReferenceExtractor;
use super::refinement::RefinementEngine;
use super::validation::{ConsistencyResult, CrossValidationResult, TierFilterResult};

#[derive(Debug, Clone)]
pub struct AdaptivePipelineOutput {
    pub claude_md: ProjectMemory,
    pub plugin: Plugin,
    pub rules: Vec<Rule>,
    pub detection: ProjectDetection,
    pub deep_analysis: Option<DeepAnalysisResult>,
    pub synthesis: Option<SynthesizedAnalysis>,
    pub output_plan: OutputPlan,
    pub enriched_plan: Option<EnrichedPlan>,
    pub tier_filter_result: TierFilterResult,
    pub consistency_result: ConsistencyResult,
    pub cross_validation_result: CrossValidationResult,
    pub quality_score: f32,
    pub refinement_iterations: usize,
    pub refinement_converged: bool,
    pub context: ClaudegenContext,
}

pub struct AdaptivePipeline {
    project_root: PathBuf,
    provider: Arc<dyn LlmProvider>,
    config: Config,
    file_registry: OnceCell<VerifiedFileRegistry>,
}

impl AdaptivePipeline {
    pub fn new(project_root: PathBuf, provider: Arc<dyn LlmProvider>, config: Config) -> Self {
        Self {
            project_root,
            provider,
            config,
            file_registry: OnceCell::new(),
        }
    }

    async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
        self.file_registry
            .get_or_try_init(|| async { VerifiedFileRegistry::build(&self.project_root).await })
            .await
            .cloned()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    async fn validate_preconditions(&self) -> Result<()> {
        use crate::types::ClaudegenError;

        // Check project root exists and is a directory
        if !self.project_root.exists() {
            return Err(ClaudegenError::Config(format!(
                "Project root does not exist: {}",
                self.project_root.display()
            )));
        }
        if !self.project_root.is_dir() {
            return Err(ClaudegenError::Config(format!(
                "Project root is not a directory: {}",
                self.project_root.display()
            )));
        }

        // Validate configuration
        self.config.validate()?;

        tracing::debug!(project = ?self.project_root, "Preconditions validated");
        Ok(())
    }

    pub async fn run(&self) -> Result<AdaptivePipelineOutput> {
        // Precondition validation
        self.validate_preconditions().await?;

        tracing::info!(project = ?self.project_root, "Starting adaptive pipeline");

        // Initialize ClaudegenContext for zero information loss
        let mut ctx = ClaudegenContext::new(&self.project_root);

        // Phase 1: Project Detection
        let detection = self.detect_project().await?;
        ctx.set_detection(detection.clone());
        tracing::info!(
            project_type = ?detection.primary_type,
            is_monorepo = detection.is_monorepo,
            languages = ?detection.languages.iter().map(|l| &l.language).collect::<Vec<_>>(),
            "Project detected"
        );

        // Phase 2: Monorepo Analysis (if applicable)
        let monorepo = if detection.is_monorepo {
            Some(self.analyze_monorepo(&detection).await?)
        } else {
            None
        };
        if let Some(ref mono) = monorepo {
            tracing::info!(
                subprojects = mono.subprojects.len(),
                shared_packages = mono.shared_packages.len(),
                output_strategy = ?mono.output_strategy,
                "Monorepo analyzed"
            );
        }

        // Get cached file registry for reference validation throughout pipeline
        let file_registry = self.get_file_registry().await?;
        tracing::debug!(
            file_count = file_registry.file_count(),
            "File registry ready for reference validation"
        );

        // Phase 2.5: Deep Analysis (if enabled) with timeout
        let analysis_timeout =
            Duration::from_secs(self.config.timeout().analysis_phase_timeout_secs);
        let deep_analysis = with_timeout(
            analysis_timeout,
            self.run_deep_analysis(&detection),
            "deep_analysis",
        )
        .await?;
        if let Some(ref analysis) = deep_analysis {
            tracing::info!(
                patterns = analysis.patterns.len(),
                constraints = analysis.constraints.len(),
                insights = analysis.insights.len(),
                abstractions = analysis.key_abstractions.len(),
                "Deep analysis complete"
            );
        }

        // Phase 2.6: Analysis Synthesis with min_confidence gating and re-analysis
        let (deep_analysis, synthesis) = if let Some(analysis) = deep_analysis {
            let synthesizer = AnalysisSynthesizer::new(self.config.analysis.clone());
            let min_confidence = self.config.deep_analysis().min_confidence;
            let max_synthesis_retries = self.config.quality_loop().max_iterations;

            let mut current_analysis = analysis;
            let mut synth_result = synthesizer.synthesize(
                current_analysis.clone(),
                None, // Structural analysis happens during refinement
                &detection,
                &file_registry,
            );

            // Re-analyze if confidence is below threshold (with retry limit)
            let mut retry_count = 0;
            while !synthesizer.meets_requirements(&synth_result, min_confidence)
                && retry_count < max_synthesis_retries
            {
                retry_count += 1;
                let reanalysis_targets =
                    synthesizer.get_reanalysis_targets(&synth_result, min_confidence);

                if !reanalysis_targets.needs_reanalysis() {
                    tracing::debug!(
                        retry = retry_count,
                        "No specific reanalysis targets identified, breaking retry loop"
                    );
                    break;
                }

                tracing::info!(
                    retry = retry_count,
                    max_retries = max_synthesis_retries,
                    confidence = format!("{:.1}%", synth_result.confidence.overall * 100.0),
                    target = format!("{:.1}%", min_confidence * 100.0),
                    reasons = ?reanalysis_targets.reasons,
                    "Synthesis below confidence threshold, re-analyzing"
                );

                // Re-run deep analysis focusing on weak areas
                match self
                    .run_targeted_reanalysis(&detection, &reanalysis_targets)
                    .await
                {
                    Ok(Some(enhanced_analysis)) => {
                        // Merge enhanced analysis with existing
                        current_analysis =
                            self.merge_analysis_results(current_analysis, enhanced_analysis);

                        // Re-synthesize
                        synth_result = synthesizer.synthesize(
                            current_analysis.clone(),
                            None,
                            &detection,
                            &file_registry,
                        );
                    }
                    Ok(None) => {
                        tracing::debug!(
                            retry = retry_count,
                            "Targeted reanalysis returned no new findings"
                        );
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            retry = retry_count,
                            "Targeted reanalysis failed, proceeding with current synthesis"
                        );
                        break;
                    }
                }
            }

            if synthesizer.meets_requirements(&synth_result, min_confidence) {
                tracing::info!(
                    overall_confidence = format!("{:.1}%", synth_result.confidence.overall * 100.0),
                    confirmed_findings = synth_result.validation.confirmed_findings.len(),
                    retries = retry_count,
                    "Analysis synthesis meets confidence requirements"
                );
            } else {
                tracing::warn!(
                    overall_confidence = format!("{:.1}%", synth_result.confidence.overall * 100.0),
                    min_required = format!("{:.1}%", min_confidence * 100.0),
                    gaps = synth_result.validation.gaps.len(),
                    retries_exhausted = retry_count,
                    "Analysis synthesis below threshold after retries - proceeding anyway"
                );
            }

            // AST Enrichment: validate and enhance synthesis with ground-truth facts
            let ast_facts =
                AstEnricher::extract_facts(&self.project_root, file_registry.all_files()).await;

            if !ast_facts.parsed_files.is_empty() {
                synthesizer.enhance_with_ast(&mut synth_result, &ast_facts);
            }

            (Some(current_analysis), Some(synth_result))
        } else {
            (None, None)
        };

        // Populate ClaudegenContext with deep analysis and synthesis
        if let Some(ref deep) = deep_analysis {
            ctx.set_deep_analysis(deep.clone());
        }
        if let Some(ref synth) = synthesis {
            ctx.set_synthesis(synth.clone());
        }

        // Log synthesis insights if available
        if let Some(ref synth) = synthesis
            && !synth.modules.is_empty()
        {
            tracing::debug!(
                merged_modules = synth.modules.len(),
                "Merged module analysis available"
            );
        }

        // Phase 3: Convention Inference
        let conventions = self.infer_conventions(&detection).await?;
        ctx.set_conventions(conventions.clone());
        tracing::info!(
            architecture = %conventions.architecture.pattern_name,
            patterns = conventions.patterns.len(),
            "Conventions inferred"
        );

        // Phase 4: Constraint Extraction (enhanced with synthesis data)
        let constraints = self
            .extract_constraints(&detection, &conventions, synthesis.as_ref())
            .await?;
        ctx.set_constraints(constraints.clone());
        tracing::info!(
            anti_patterns = constraints.anti_patterns.len(),
            hidden_deps = constraints.hidden_dependencies.len(),
            workflows = constraints.complex_workflows.len(),
            gotchas = constraints.gotchas.len(),
            "Constraints extracted"
        );

        // Phase 5: Output Planning (enhanced with synthesis data)
        let output_plan = output_router::OutputRouter::plan_with_synthesis(
            &detection,
            monorepo.as_ref(),
            &conventions,
            &constraints,
            synthesis.as_ref(),
        )?;
        tracing::info!(
            strategy = ?output_plan.strategy,
            rule_groups = output_plan.rules_plan.rule_groups.len(),
            skills = output_plan.skills_plan.planned_skills.len(),
            agents = output_plan.agents_plan.planned_agents.len(),
            "Output planned"
        );

        // Phase 5.5: Enrichment - Bridge synthesis findings to generation
        let enrichment_engine = EnrichmentEngine::new(self.config.quality().minimum_quality);
        let enriched_plan =
            enrichment_engine.enrich(output_plan.clone(), synthesis.as_ref(), &constraints);
        tracing::info!(
            total_constraints = enriched_plan.coverage.total_constraints,
            covered = enriched_plan.coverage.covered_constraints,
            coverage_ratio = format!("{:.1}%", enriched_plan.coverage.coverage_ratio * 100.0),
            suggested_artifacts = enriched_plan.suggested_artifacts.len(),
            "Enrichment complete"
        );

        // Log coverage warning if below threshold
        if !enrichment_engine.meets_coverage_threshold(&enriched_plan.coverage) {
            tracing::warn!(
                coverage = format!("{:.1}%", enriched_plan.coverage.coverage_ratio * 100.0),
                uncovered = enriched_plan.coverage.uncovered.len(),
                "Constraint coverage below threshold - some constraints may not appear in output"
            );
        }

        // Phase 6: Draft Generation
        let project_name = self
            .project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let claude_md = ClaudeMdGenerator::generate_with_enrichment(
            &output_plan,
            &detection,
            &conventions,
            &constraints,
            &project_name,
            Some(&enriched_plan),
            synthesis.as_ref(),
        )?;

        let rules = PathRulesGenerator::generate(
            &output_plan,
            monorepo.as_ref(),
            &conventions,
            &constraints,
        )?;

        let skills = self
            .generate_skills_with_enrichment(
                &enriched_plan,
                &constraints,
                &file_registry,
                &conventions,
                synthesis.as_ref(),
            )
            .await?;
        let agents = self
            .generate_agents_with_enrichment(&enriched_plan, &detection, monorepo.as_ref())
            .await?;

        tracing::info!(
            skills = skills.len(),
            agents = agents.len(),
            rules = rules.len(),
            "Draft generation complete"
        );

        // Phase 7: Quality-Based Refinement Loop with Cross-Session Learning
        let mut refinement_engine = RefinementEngine::new_async(
            self.project_root.clone(),
            Arc::clone(&self.provider),
            self.config.clone(),
            file_registry.clone(),
        )
        .await?;

        let refinement_result = refinement_engine
            .refine(skills, agents, rules, &claude_md, &output_plan)
            .await?;

        tracing::info!(
            iterations = refinement_result.iterations,
            converged = refinement_result.converged,
            quality = refinement_result.final_quality,
            "Refinement complete"
        );

        let skills = refinement_result.skills;
        let agents = refinement_result.agents;
        let rules = refinement_result.rules;

        // Phase 8: Final Validation (simplified - actual validation via LLM Judge)
        let tier_result =
            TierFilterResult::check(&skills, &agents, &rules, &claude_md.to_markdown());

        let consistency_result =
            ConsistencyResult::check(detection.is_monorepo, &skills, &agents, &rules);

        let file_registry = self.get_file_registry().await?;
        let cross_validation_result =
            CrossValidationResult::check(&skills, &agents, &rules, &claude_md, &file_registry);

        let quality_score = refinement_result.final_quality;

        tracing::info!(
            quality_score = quality_score,
            tier_passed = tier_result.passed,
            consistency_passed = consistency_result.passed,
            cv_passed = cross_validation_result.passed,
            "Final validation complete"
        );

        let plugin = Plugin {
            manifest: PluginManifest::new(format!("{}-plugin", to_kebab_case(&project_name)))
                .with_version("1.0.0")
                .with_description(format!("Claude Code plugin for {}", project_name)),
            skills: skills.clone(),
            agents: agents.clone(),
            rules: rules.clone(),
        };

        // Log accumulated context stats
        let ctx_stats = ctx.stats();
        tracing::info!(
            tier3_items = ctx_stats.tier3_count,
            abstractions = ctx_stats.abstraction_count,
            conventions = ctx_stats.convention_count,
            iterations = ctx_stats.iteration_count,
            "ClaudegenContext populated"
        );

        Ok(AdaptivePipelineOutput {
            claude_md,
            plugin,
            rules,
            detection,
            deep_analysis,
            synthesis,
            output_plan,
            enriched_plan: Some(enriched_plan),
            tier_filter_result: tier_result,
            consistency_result,
            cross_validation_result,
            quality_score,
            refinement_iterations: refinement_result.iterations,
            refinement_converged: refinement_result.converged,
            context: ctx,
        })
    }

    async fn detect_project(&self) -> Result<ProjectDetection> {
        project_detection::detect(&self.project_root).await
    }

    async fn analyze_monorepo(&self, detection: &ProjectDetection) -> Result<MonorepoAnalysis> {
        monorepo_analyzer::analyze(&self.project_root, detection).await
    }

    async fn infer_conventions(&self, detection: &ProjectDetection) -> Result<InferredConventions> {
        let max_samples = self.config.few_shot.max_examples;
        convention_inference::infer(
            &self.project_root,
            detection,
            self.provider.clone(),
            max_samples,
        )
        .await
    }

    async fn extract_constraints(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<ExtractedConstraints> {
        let extractor = constraint_extraction::ConstraintExtractor::new(
            &self.project_root,
            self.provider.clone(),
        );
        extractor
            .extract_with_synthesis(detection, conventions, synthesis)
            .await
    }

    async fn run_deep_analysis(
        &self,
        detection: &ProjectDetection,
    ) -> Result<Option<DeepAnalysisResult>> {
        if !self.config.deep_analysis().enabled {
            return Ok(None);
        }

        // Use MultiAgentAnalyzer if enabled
        if self.config.multi_agent().enabled {
            return self.run_multi_agent_analysis(detection).await;
        }

        let analyzer = DeepAnalyzer::new(
            &self.project_root,
            Arc::clone(&self.provider),
            self.config.analysis.clone(),
            self.config.deep_analysis.clone(),
        );

        let result = analyzer.analyze(detection).await?;

        if result.patterns.is_empty() && result.constraints.is_empty() && result.insights.is_empty()
        {
            return Ok(None);
        }

        Ok(Some(result))
    }

    /// Run targeted reanalysis focusing on weak areas identified by synthesis
    async fn run_targeted_reanalysis(
        &self,
        detection: &ProjectDetection,
        targets: &super::analysis::synthesis::ReanalysisTargets,
    ) -> Result<Option<DeepAnalysisResult>> {
        use crate::config::AnalysisSpecialty;

        // Build a focused multi-agent config based on reanalysis targets
        let mut focused_config = self.config.multi_agent().clone();

        let mut enabled_specialists = Vec::new();
        if targets.reanalyze_structure {
            enabled_specialists.push(AnalysisSpecialty::Structure);
        }
        if targets.reanalyze_patterns {
            enabled_specialists.push(AnalysisSpecialty::Pattern);
        }
        if targets.reanalyze_constraints {
            enabled_specialists.push(AnalysisSpecialty::Constraint);
        }

        if enabled_specialists.is_empty() {
            return Ok(None);
        }

        focused_config.enabled_specialists = enabled_specialists;

        let file_registry = self.get_file_registry().await?;

        let file_list: Vec<String> = file_registry.all_files()
            .take(self.config.analysis.depth.max_files().min(200)) // Read more files for reanalysis
            .cloned()
            .collect();

        let mut file_contents = std::collections::HashMap::new();
        for file in file_list.iter().take(100) {
            // Read more files
            let path = self.project_root.join(file.as_str());
            if let Ok(content) = tokio::fs::read_to_string(&path).await
                && content.len() < 50000
            {
                file_contents.insert(file.clone(), content);
            }
        }

        let context = AnalysisContext {
            project_root: self.project_root.to_string_lossy().to_string(),
            file_list,
            file_contents,
            project_type: detection.primary_type.as_str().to_string(),
            languages: detection
                .languages
                .iter()
                .map(|l| l.language.clone())
                .collect(),
        };

        let multi_analyzer = MultiAgentAnalyzer::new(Arc::clone(&self.provider))
            .with_timeout(self.config.multi_agent.specialist_timeout_secs);

        let result = multi_analyzer.analyze(context).await?;
        let deep_result = multi_analyzer.to_deep_analysis_result(result);

        if deep_result.patterns.is_empty()
            && deep_result.constraints.is_empty()
            && deep_result.insights.is_empty()
            && deep_result.structure.core_modules.is_empty()
        {
            return Ok(None);
        }

        Ok(Some(deep_result))
    }

    /// Merge two analysis results, preferring new findings where they exist
    fn merge_analysis_results(
        &self,
        existing: DeepAnalysisResult,
        new: DeepAnalysisResult,
    ) -> DeepAnalysisResult {
        use std::collections::HashSet;

        // Collect existing names first (cloning to avoid borrow issues)
        let existing_pattern_names: HashSet<String> =
            existing.patterns.iter().map(|p| p.name.clone()).collect();
        let existing_constraint_titles: HashSet<String> = existing
            .constraints
            .iter()
            .map(|c| c.title.clone())
            .collect();
        let existing_module_names: HashSet<String> = existing
            .structure
            .core_modules
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let existing_entry_paths: HashSet<String> = existing
            .structure
            .entry_points
            .iter()
            .map(|e| e.path.clone())
            .collect();
        let existing_insight_files: HashSet<String> =
            existing.insights.iter().map(|i| i.file.clone()).collect();
        let existing_abstraction_names: HashSet<String> = existing
            .key_abstractions
            .iter()
            .map(|a| a.name.clone())
            .collect();

        // Now move and merge
        let mut merged_patterns = existing.patterns;
        for pattern in new.patterns {
            if !existing_pattern_names.contains(&pattern.name) {
                merged_patterns.push(pattern);
            }
        }

        let mut merged_constraints = existing.constraints;
        for constraint in new.constraints {
            if !existing_constraint_titles.contains(&constraint.title) {
                merged_constraints.push(constraint);
            }
        }

        let mut merged_modules = existing.structure.core_modules;
        for module in new.structure.core_modules {
            if !existing_module_names.contains(&module.name) {
                merged_modules.push(module);
            }
        }

        let mut merged_entry_points = existing.structure.entry_points;
        for entry in new.structure.entry_points {
            if !existing_entry_paths.contains(&entry.path) {
                merged_entry_points.push(entry);
            }
        }

        let mut merged_insights = existing.insights;
        for insight in new.insights {
            if !existing_insight_files.contains(&insight.file) {
                merged_insights.push(insight);
            }
        }

        let mut merged_abstractions = existing.key_abstractions;
        for abstraction in new.key_abstractions {
            if !existing_abstraction_names.contains(&abstraction.name) {
                merged_abstractions.push(abstraction);
            }
        }

        DeepAnalysisResult {
            structure: super::analysis::deep_analyzer::SemanticStructure {
                entry_points: merged_entry_points,
                core_modules: merged_modules,
                layer_boundaries: existing.structure.layer_boundaries,
                config_locations: existing.structure.config_locations,
            },
            patterns: merged_patterns,
            constraints: merged_constraints,
            dependencies: existing.dependencies,
            insights: merged_insights,
            key_abstractions: merged_abstractions,
            analysis_quality: super::analysis::deep_analyzer::AnalysisQuality {
                files_analyzed: existing.analysis_quality.files_analyzed
                    + new.analysis_quality.files_analyzed,
                lines_analyzed: existing.analysis_quality.lines_analyzed
                    + new.analysis_quality.lines_analyzed,
                coverage_ratio: (existing.analysis_quality.coverage_ratio
                    + new.analysis_quality.coverage_ratio)
                    / 2.0,
                evidence_count: existing.analysis_quality.evidence_count
                    + new.analysis_quality.evidence_count,
                validated_refs: existing.analysis_quality.validated_refs
                    + new.analysis_quality.validated_refs,
                filtered_hallucinations: existing.analysis_quality.filtered_hallucinations
                    + new.analysis_quality.filtered_hallucinations,
                confidence_score: (existing.analysis_quality.confidence_score
                    + new.analysis_quality.confidence_score)
                    / 2.0,
            },
        }
    }

    async fn run_multi_agent_analysis(
        &self,
        detection: &ProjectDetection,
    ) -> Result<Option<DeepAnalysisResult>> {
        let file_registry = self.get_file_registry().await?;

        // Build analysis context
        let file_list: Vec<String> = file_registry
            .all_files()
            .take(self.config.analysis.depth.max_files().min(100))
            .cloned()
            .collect();

        let mut file_contents = std::collections::HashMap::new();
        for file in file_list.iter().take(50) {
            let path = self.project_root.join(file.as_str());
            if let Ok(content) = tokio::fs::read_to_string(&path).await
                && content.len() < 50000
            {
                // Skip very large files
                file_contents.insert(file.clone(), content);
            }
        }

        let context = AnalysisContext {
            project_root: self.project_root.to_string_lossy().to_string(),
            file_list,
            file_contents,
            project_type: detection.primary_type.as_str().to_string(),
            languages: detection
                .languages
                .iter()
                .map(|l| l.language.clone())
                .collect(),
        };

        let multi_analyzer = MultiAgentAnalyzer::new(Arc::clone(&self.provider))
            .with_timeout(self.config.multi_agent.specialist_timeout_secs);

        let result = multi_analyzer.analyze(context).await?;

        // Convert to DeepAnalysisResult
        let deep_result = multi_analyzer.to_deep_analysis_result(result);

        if deep_result.patterns.is_empty()
            && deep_result.constraints.is_empty()
            && deep_result.insights.is_empty()
        {
            return Ok(None);
        }

        Ok(Some(deep_result))
    }

    /// Generate skills with enriched constraints from synthesis
    async fn generate_skills_with_enrichment(
        &self,
        enriched_plan: &EnrichedPlan,
        constraints: &ExtractedConstraints,
        file_registry: &VerifiedFileRegistry,
        _conventions: &InferredConventions,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<Vec<Skill>> {
        let mut skills = Vec::new();

        for planned in &enriched_plan.plan.skills_plan.planned_skills {
            // Get enriched constraints for this skill
            let skill_constraints = enriched_plan
                .skill_constraints
                .get(&planned.name)
                .cloned()
                .unwrap_or_default();

            let workflow = constraints
                .complex_workflows
                .iter()
                .find(|w| to_kebab_case(&w.name) == planned.name);

            let mut body = if let Some(w) = workflow {
                let mut body = format!("## {}\n\n", w.name);
                body.push_str(&format!("{}\n\n", w.description));
                body.push_str("### Steps\n");
                for step in &w.steps {
                    let valid_files: Vec<_> = step
                        .files_involved
                        .iter()
                        .filter(|f| file_registry.contains(f) || file_registry.directory_exists(f))
                        .collect();

                    if let Some(file_ref) = valid_files.first() {
                        body.push_str(&format!(
                            "{}. {} (see @{})\n",
                            step.order, step.action, file_ref
                        ));
                    } else {
                        body.push_str(&format!("{}. {}\n", step.order, step.action));
                    }
                }
                if !w.gotchas.is_empty() {
                    body.push_str("\n### Gotchas\n");
                    for gotcha in &w.gotchas {
                        body.push_str(&format!("- {}\n", gotcha));
                    }
                }
                body
            } else {
                format!("## {}\n\n{}", planned.name, planned.trigger)
            };

            // Inject enriched constraints into skill body
            if !skill_constraints.is_empty() {
                body.push_str("\n### Critical Constraints\n");
                for constraint in &skill_constraints {
                    body.push_str(&format!("- {}\n", constraint.format_for_skill()));
                }
            }

            // Add module context from synthesis if available
            if let Some(synth) = synthesis {
                let relevant_modules: Vec<_> = synth
                    .modules
                    .iter()
                    .filter(|m| {
                        let name_lower = planned.name.to_lowercase();
                        m.name.to_lowercase().contains(&name_lower)
                            || name_lower.contains(&m.name.to_lowercase())
                    })
                    .take(2)
                    .collect();

                if !relevant_modules.is_empty() {
                    body.push_str("\n### Related Modules\n");
                    for module in relevant_modules {
                        body.push_str(&format!("- @{}: {}\n", module.path, module.responsibility));
                    }
                }
            }

            let skill = Skill::new(&planned.name, &planned.trigger, body).with_user_invocable(true);

            skills.push(skill);
        }

        Ok(skills)
    }

    /// Generate agents with enriched internal knowledge from synthesis
    async fn generate_agents_with_enrichment(
        &self,
        enriched_plan: &EnrichedPlan,
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
    ) -> Result<Vec<Agent>> {
        let mut agents = Vec::new();

        for planned in &enriched_plan.plan.agents_plan.planned_agents {
            // Get enriched knowledge for this agent
            let knowledge = enriched_plan.agent_knowledge.get(&planned.name).cloned();

            let agent = self
                .build_agent_with_knowledge(
                    &planned.name,
                    &planned.role,
                    detection,
                    monorepo,
                    knowledge,
                )
                .await?;
            agents.push(agent);
        }

        Ok(agents)
    }

    /// Build agent with injected internal knowledge
    async fn build_agent_with_knowledge(
        &self,
        name: &str,
        role: &str,
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        knowledge: Option<super::enrichment::AgentInternalKnowledge>,
    ) -> Result<Agent> {
        let (model, tools) = self.determine_agent_config(name, role);

        // Generate prompt with injected internal knowledge
        let prompt = if let Some(ref k) = knowledge
            && k.is_substantial()
        {
            // Use enriched knowledge directly
            self.build_enriched_agent_prompt(role, detection, monorepo, k)
        } else {
            // Fallback to LLM generation
            self.generate_agent_prompt_with_llm(role, detection, monorepo)
                .await?
        };

        let mut agent = Agent::new(name, role, prompt).with_model(model);

        if !tools.is_empty() {
            agent = agent.with_tools(tools);
        }

        Ok(agent)
    }

    /// Build agent prompt with enriched internal knowledge
    fn build_enriched_agent_prompt(
        &self,
        role: &str,
        detection: &ProjectDetection,
        _monorepo: Option<&MonorepoAnalysis>,
        knowledge: &super::enrichment::AgentInternalKnowledge,
    ) -> String {
        let project_type = detection.primary_type.as_str();
        let langs: Vec<_> = detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();

        let mut prompt = format!(
            "## Description\n\
            {role} specialist for {project_type} ({langs}) with deep internal project knowledge.\n\n",
            role = role,
            project_type = project_type,
            langs = langs.join(", "),
        );

        // Add the enriched internal knowledge section
        prompt.push_str(&knowledge.format_as_prompt_section());

        prompt
    }

    fn determine_agent_config(&self, name: &str, role: &str) -> (AgentModel, Vec<String>) {
        let agent_config = &self.config.output.agents;

        // Check per-agent overrides first (exact name match)
        if let Some(override_cfg) = agent_config.overrides.get(name) {
            let model = override_cfg
                .model
                .as_ref()
                .map(|m| m.to_agent_model())
                .unwrap_or_else(|| agent_config.default_model.to_agent_model());

            let tools = override_cfg
                .tools
                .clone()
                .unwrap_or_else(|| self.get_tools_for_role(&role.to_lowercase(), agent_config));

            return (model, tools);
        }

        let role_lower = role.to_lowercase();

        // Check configurable role mappings
        for mapping in &agent_config.role_mappings {
            if mapping.patterns.iter().any(|p| role_lower.contains(p)) {
                let tools = self.get_tools_for_role(&role_lower, agent_config);
                return (mapping.model.to_agent_model(), tools);
            }
        }

        // Fallback to default
        (
            agent_config.default_model.to_agent_model(),
            agent_config.tools.default.clone(),
        )
    }

    fn get_tools_for_role(
        &self,
        role_lower: &str,
        agent_config: &crate::config::OutputAgentConfig,
    ) -> Vec<String> {
        if role_lower.contains("debug") || role_lower.contains("troubleshoot") {
            agent_config.tools.debug.clone()
        } else if role_lower.contains("architect") || role_lower.contains("design") {
            agent_config.tools.architect.clone()
        } else if role_lower.contains("coordinator") || role_lower.contains("cross") {
            agent_config.tools.coordinator.clone()
        } else {
            agent_config.tools.default.clone()
        }
    }

    async fn generate_agent_prompt_with_llm(
        &self,
        role: &str,
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
    ) -> Result<String> {
        let project_type = detection.primary_type.as_str();
        let langs: Vec<_> = detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();

        let key_paths = self.get_key_paths(detection).await;

        let prompt = format!(
            "Generate a domain-expert agent prompt for a \"{role}\" specialist.\n\n\
Project: {project_type} in {langs}\n\
Key paths: {key_paths}\n\
{monorepo_info}\n\n\
CRITICAL REQUIREMENTS (ALL must be satisfied):\n\
1. MUST include an \"Internal Knowledge\" section with project-specific hidden constraints\n\
2. MUST have at least 2 file references with line numbers (e.g., @src/pipeline/mod.rs:42)\n\
3. MUST NOT be a generic role name (code-reviewer, test-writer, bug-fixer, etc.)\n\
4. MUST describe specific workflows/sequences unique to this project\n\
5. MUST include gotchas or order-dependent operations\n\n\
REQUIRED STRUCTURE:\n\
## Description\n\
One line explaining the agent's domain expertise in THIS project.\n\n\
## Internal Knowledge\n\
- Hidden constraints specific to this project\n\
- Order-dependent workflows\n\
- Gotchas that new developers would not know\n\n\
## Key References\n\
- @path/to/file.rs:line - Description of what this reference teaches\n\n\
Return as JSON: {{\"prompt\": \"...\"}}\n",
            role = role,
            project_type = project_type,
            langs = langs.join(", "),
            key_paths = key_paths.join(", "),
            monorepo_info = if let Some(mono) = monorepo {
                format!("Monorepo with {} subprojects", mono.subprojects.len())
            } else {
                String::new()
            }
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The generated agent prompt"
                }
            },
            "required": ["prompt"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(prompt_text) = response.content.get("prompt").and_then(|v| v.as_str()) {
                    Ok(prompt_text.trim().to_string())
                } else {
                    Ok(Self::fallback_agent_prompt(role, detection, &key_paths))
                }
            }
            Err(_) => Ok(Self::fallback_agent_prompt(role, detection, &key_paths)),
        }
    }

    async fn get_key_paths(&self, detection: &ProjectDetection) -> Vec<String> {
        let mut paths = Vec::new();

        if let Ok(refs) =
            ReferenceExtractor::extract_key_references(&self.project_root, detection.primary_type)
                .await
        {
            for r in refs.into_iter().take(6) {
                paths.push(r.to_string_ref());
            }
        }

        if paths.is_empty() {
            let candidates = match detection.primary_type {
                crate::config::ProjectType::Cli => {
                    vec!["src/main.rs", "src/cli/", "src/commands/"]
                }
                crate::config::ProjectType::Library => vec!["src/lib.rs", "src/"],
                crate::config::ProjectType::Backend => {
                    vec!["src/api/", "src/domain/", "services/"]
                }
                crate::config::ProjectType::Frontend => {
                    vec!["src/components/", "src/pages/", "app/"]
                }
                crate::config::ProjectType::Monorepo => {
                    vec!["packages/", "apps/", "services/"]
                }
                crate::config::ProjectType::Agent => {
                    vec!["src/tools/", "src/prompts/", "src/agents/"]
                }
                _ => vec!["src/"],
            };

            for candidate in candidates {
                let path = self.project_root.join(candidate);
                if path.exists() {
                    paths.push(format!("@{}", candidate));
                }
            }
        }

        if paths.is_empty() {
            paths.push("@src/".to_string());
        }

        paths.into_iter().take(6).collect()
    }

    fn fallback_agent_prompt(
        role: &str,
        detection: &ProjectDetection,
        key_paths: &[String],
    ) -> String {
        let project_type = detection.primary_type.as_str();
        let langs: Vec<_> = detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();

        format!(
            "## Description\n\
{role} specialist for {project_type} ({langs}) with internal project knowledge.\n\n\
## Internal Knowledge\n\
- This agent has project-specific constraints that must be discovered through analysis\n\
- Consult @CLAUDE.md for architecture patterns and anti-patterns\n\n\
## Key References\n\
{paths}\n\
- @CLAUDE.md - Project conventions and architecture\n\
- @.claude/rules/ - Path-specific rules",
            role = role,
            project_type = project_type,
            langs = langs.join(", "),
            paths = key_paths.join("\n")
        )
    }

    pub async fn write_output(&self, output: &AdaptivePipelineOutput) -> Result<()> {
        // Write CLAUDE.md atomically
        atomic_write(
            &self.project_root.join("CLAUDE.md"),
            output.claude_md.to_markdown().as_bytes(),
        )
        .await?;

        // Create plugin directory structure
        let plugin_dir = output.plugin.plugin_dir(&self.project_root);
        let claude_plugin_dir = plugin_dir.join(".claude-plugin");
        let skills_dir = plugin_dir.join("skills");
        let agents_dir = plugin_dir.join("agents");

        // Clean old directories
        for dir in [&skills_dir, &agents_dir] {
            if dir.exists() {
                fs::remove_dir_all(dir).await?;
            }
        }

        fs::create_dir_all(&claude_plugin_dir).await?;
        fs::create_dir_all(&skills_dir).await?;
        fs::create_dir_all(&agents_dir).await?;

        // Write plugin manifest atomically
        let manifest_json = output.plugin.manifest.to_json().map_err(|e| {
            crate::types::ClaudegenError::Config(format!(
                "Failed to serialize plugin manifest: {e}"
            ))
        })?;
        atomic_write(
            &claude_plugin_dir.join("plugin.json"),
            manifest_json.as_bytes(),
        )
        .await?;

        // Write skills
        for skill in &output.plugin.skills {
            let skill_dir = skills_dir.join(&skill.name);
            fs::create_dir_all(&skill_dir).await?;
            atomic_write(&skill_dir.join("SKILL.md"), skill.to_markdown().as_bytes()).await?;
        }

        // Write agents
        for agent in &output.plugin.agents {
            atomic_write(
                &agents_dir.join(format!("{}.md", agent.name)),
                agent.to_markdown().as_bytes(),
            )
            .await?;
        }

        // Write rules to .claude/rules/
        // Clean stale rules first to prevent invalid constraints from previous runs
        let rules_dir = self.project_root.join(".claude").join("rules");
        if rules_dir.exists() {
            // Remove all existing .md files to prevent stale rules
            if let Ok(mut entries) = fs::read_dir(&rules_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "md")
                        && let Err(e) = fs::remove_file(&path).await
                    {
                        tracing::debug!(path = %path.display(), error = %e, "Failed to remove stale rule");
                    }
                }
            }
        }

        if !output.rules.is_empty() || output.output_plan.strategy.requires_path_rules() {
            fs::create_dir_all(&rules_dir).await?;

            for rule in &output.rules {
                atomic_write(
                    &rules_dir.join(format!("{}.md", rule.name)),
                    rule.to_markdown().as_bytes(),
                )
                .await?;
            }
        }

        Ok(())
    }

    pub fn project_root(&self) -> &PathBuf {
        &self.project_root
    }

    /// Write only plugin artifacts (skills, agents, manifest).
    /// Used when output_dir differs from project_root.
    pub async fn write_plugin_only(&self, output: &AdaptivePipelineOutput) -> Result<()> {
        // Create plugin directory structure
        let plugin_dir = output.plugin.plugin_dir(&self.project_root);
        let claude_plugin_dir = plugin_dir.join(".claude-plugin");
        let skills_dir = plugin_dir.join("skills");
        let agents_dir = plugin_dir.join("agents");

        // Clean old directories
        for dir in [&skills_dir, &agents_dir] {
            if dir.exists() {
                fs::remove_dir_all(dir).await?;
            }
        }

        fs::create_dir_all(&claude_plugin_dir).await?;
        fs::create_dir_all(&skills_dir).await?;
        fs::create_dir_all(&agents_dir).await?;

        // Write plugin manifest atomically
        let manifest_json = output.plugin.manifest.to_json().map_err(|e| {
            crate::types::ClaudegenError::Config(format!(
                "Failed to serialize plugin manifest: {e}"
            ))
        })?;
        atomic_write(
            &claude_plugin_dir.join("plugin.json"),
            manifest_json.as_bytes(),
        )
        .await?;

        // Write skills
        for skill in &output.plugin.skills {
            let skill_dir = skills_dir.join(&skill.name);
            fs::create_dir_all(&skill_dir).await?;
            atomic_write(&skill_dir.join("SKILL.md"), skill.to_markdown().as_bytes()).await?;
        }

        // Write agents
        for agent in &output.plugin.agents {
            atomic_write(
                &agents_dir.join(format!("{}.md", agent.name)),
                agent.to_markdown().as_bytes(),
            )
            .await?;
        }

        Ok(())
    }
}

fn to_kebab_case(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_whitespace() || c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<String>()
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Atomically write content to a file.
///
/// Writes to a temporary file in the same directory, syncs to disk,
/// then renames to the target path. This prevents partial writes on crash.
async fn atomic_write(path: &std::path::Path, content: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    use uuid::Uuid;

    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let temp_name = format!(".tmp_{}", Uuid::new_v4());
    let temp_path = parent.join(&temp_name);

    // Write to temp file
    let mut file: tokio::fs::File = fs::File::create(&temp_path).await?;
    file.write_all(content).await?;
    file.sync_all().await?;
    drop(file);

    // Atomic rename (same filesystem guarantees atomicity on POSIX)
    if let Err(e) = fs::rename(&temp_path, path).await {
        // Clean up temp file on failure
        let _ = fs::remove_file(&temp_path).await;
        return Err(crate::types::ClaudegenError::Io(e));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("Hello World"), "hello-world");
        assert_eq!(to_kebab_case("API_Client"), "api-client");
        assert_eq!(to_kebab_case("  spaced  "), "spaced");
    }
}
