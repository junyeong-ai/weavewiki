//! Generate Command - Creates Claude Code plugin
//!
//! Uses QualityLoop with Adaptive Pipeline for project-type agnostic generation:
//! - Outer Loop: Quality verification with analysis re-run on gaps
//! - Phase 1: Project Detection (auto-detect CLI/Library/Backend/Frontend/Monorepo)
//! - Phase 2: Monorepo Analysis (workspace structure)
//! - Phase 3: Multi-Agent Deep Analysis (parallel specialist agents)
//! - Phase 4: Convention Inference (few-shot based)
//! - Phase 5: Constraint Extraction (Tier 3 value)
//! - Phase 6: Output Planning (strategy selection)
//! - Phase 7: Insight-Driven Generation (LLM-based content decisions)
//! - Phase 8: Quality-Based Refinement (semantic quality loop)
//! - Phase 9: Final Validation (tier filtering, evidence validation)

use std::path::PathBuf;
use std::sync::Arc;

use tokio::runtime::Runtime;

use crate::config::{Config, ConfigLoader};
use crate::pipeline::adaptive::AdaptivePipelineOutput;
use crate::pipeline::{QualityLoop, QualityLoopResult};
use crate::types::Result;

#[cfg(feature = "claude-agent")]
use crate::ai::ClaudeAgentProvider;

#[derive(Default)]
pub struct GenerateOptions {
    pub output: Option<PathBuf>,
    pub resume: bool,
    pub dry_run: bool,
}


pub fn run(options: GenerateOptions) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(run_async(options))
}

async fn run_async(options: GenerateOptions) -> Result<()> {
    let project_root = std::env::current_dir()?;

    println!("claudegen - Claude Code Plugin Generator\n");
    println!("Using Quality Loop with Adaptive Pipeline");

    let config = ConfigLoader::load()?;

    if options.dry_run {
        println!("\nDry run mode - configuration:\n");
        println!("Project: {}", project_root.display());
        println!("Analysis depth: {:?}", config.analysis.depth);
        println!("Preset: {:?}", config.preset);
        println!("Quality loop enabled: {}", config.quality_loop().enabled);
        println!("Multi-agent enabled: {}", config.multi_agent().enabled);
        println!("\nQuality Loop Pipeline:");
        println!("  Outer Loop: Quality verification with re-analysis on gaps");
        println!("  1. Project Detection - Auto-detect project type");
        println!("  2. Monorepo Analysis - Workspace structure");
        println!("  3. Multi-Agent Analysis - Parallel specialist agents");
        println!("  4. Convention Inference - Few-shot based");
        println!("  5. Constraint Extraction - Hidden dependencies");
        println!("  6. Output Planning - Strategy selection");
        println!("  7. Insight-Driven Generation - LLM content decisions");
        println!("  8. Quality Refinement - Semantic quality loop");
        println!("  9. Final Validation - Tier filtering, evidence check");
        return Ok(());
    }

    let provider = create_provider(&config).await?;

    let quality_loop = QualityLoop::new(project_root.clone(), provider, config);

    println!("\nStarting Quality Loop Pipeline...\n");
    println!("  Outer Loop: Quality verification with analysis re-run on gaps");
    println!("  Inner Pipeline:");
    println!("    - Project Detection");
    println!("    - Monorepo Analysis");
    println!("    - Multi-Agent Deep Analysis");
    println!("    - Convention Inference");
    println!("    - Constraint Extraction");
    println!("    - Output Planning");
    println!("    - Insight-Driven Generation");
    println!("    - Quality Refinement");
    println!("    - Final Validation");
    println!();

    match quality_loop.run().await {
        Ok(result) => {
            quality_loop.write_output(&result).await?;
            print_quality_loop_result(&result);
            print_summary(&result.output);
            Ok(())
        }
        Err(e) => {
            eprintln!("\nGeneration failed: {e}");
            Err(e)
        }
    }
}

fn print_quality_loop_result(result: &QualityLoopResult) {
    println!("\n🔄 Quality Loop Summary:");
    println!("  - Outer iterations: {}", result.outer_iterations);
    println!("  - Analysis re-runs: {}", result.analysis_rerun_count);
    println!("  - Final confidence: {:.1}%", result.final_confidence * 100.0);

    if !result.gaps_discovered.is_empty() {
        println!("  - Gaps discovered: {}", result.gaps_discovered.len());
        for gap in result.gaps_discovered.iter().take(3) {
            println!("    • [{}] {} (iteration {})", gap.area, gap.description, gap.iteration_found);
        }
    }

    // Deep Review Results
    if result.deep_review_attempts > 0 {
        let status = if result.deep_review_passed { "✓" } else { "✗" };
        println!("\n🔍 Deep Review: {} ({} attempts)", status, result.deep_review_attempts);
        if !result.deep_review_passed {
            println!("  ⚠️  Two-pass verification did not pass");
        }
    }
}

#[cfg(feature = "claude-agent")]
async fn create_provider(config: &Config) -> Result<Arc<dyn crate::ai::LlmProvider>> {
    let model = &config.llm.default_model;
    let provider = ClaudeAgentProvider::from_env(model).await?;
    Ok(Arc::new(provider))
}

#[cfg(not(feature = "claude-agent"))]
async fn create_provider(_config: &Config) -> Result<Arc<dyn crate::ai::LlmProvider>> {
    Err(crate::types::ClaudegenError::Config(
        "No LLM provider available. Enable 'claude-agent' feature.".into(),
    ))
}

/// Print summary of generation results to stdout.
/// This function belongs in the CLI layer as it handles user-facing output.
fn print_summary(output: &AdaptivePipelineOutput) {
    let plugin_name = &output.plugin.manifest.name;

    println!("\n✓ Generation complete!");
    println!("\n📊 Detection Results:");
    println!("  - Project Type: {:?}", output.detection.primary_type);
    println!("  - Is Monorepo: {}", output.detection.is_monorepo);
    println!(
        "  - Languages: {}",
        output
            .detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!("\n📋 Output Strategy: {:?}", output.output_plan.strategy);

    println!("\n📁 Generated files:");
    println!("  - CLAUDE.md");

    if !output.plugin.skills.is_empty() {
        println!(
            "  - {}/skills/ ({} skills)",
            plugin_name,
            output.plugin.skills.len()
        );
    }

    if !output.plugin.agents.is_empty() {
        println!(
            "  - {}/agents/ ({} agents)",
            plugin_name,
            output.plugin.agents.len()
        );
    }

    if !output.rules.is_empty() {
        println!("  - .claude/rules/ ({} rules)", output.rules.len());
    }

    if !output.tier_filter_result.passed {
        println!(
            "\n⚠️  Tier 1 content filtered: {} items removed",
            output.tier_filter_result.tier1_violations.len()
        );
    }

    if !output.consistency_result.passed {
        println!(
            "\n⚠️  Consistency warnings: {} issues",
            output.consistency_result.issues.len()
        );
    }

    // Refinement Loop Results
    let status = if output.refinement_converged {
        "converged"
    } else {
        "max iterations"
    };
    println!(
        "\n🔄 Refinement: {} iterations ({})",
        output.refinement_iterations, status
    );

    // Quality Score and Cross-Validation
    println!("\n📊 Quality Score: {:.0}%", output.quality_score * 100.0);

    let cv = &output.cross_validation_result;
    if cv.evidence_traceability.total_references > 0 {
        println!(
            "  - Evidence Traceability: {}/{} references valid ({:.0}%)",
            cv.evidence_traceability.valid_references,
            cv.evidence_traceability.total_references,
            cv.evidence_traceability.coverage_score * 100.0
        );
    }

    if !cv.plan_consistency.missing_items.is_empty() {
        println!(
            "  - Plan Consistency: {} items missing from plan",
            cv.plan_consistency.missing_items.len()
        );
    }

    if !cv.passed {
        println!(
            "\n⚠️  Cross-validation warnings: {} issues",
            cv.issues.len()
        );
    }
}
