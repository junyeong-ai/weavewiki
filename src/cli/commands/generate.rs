//! Generate Command - Creates Claude Code plugin
//!
//! Uses QualityLoop with Adaptive Pipeline for project-type agnostic generation.

use std::path::PathBuf;

use tokio::runtime::Runtime;

use crate::ai::budget::create_shared_budget;
use crate::ai::metrics::create_shared_metrics;
use crate::ai::provider::{ProviderConfig, ProviderSet, create_provider_set};
use crate::config::{Config, ConfigLoader};
use crate::pipeline::adaptive::AdaptivePipelineOutput;
use crate::pipeline::{QualityLoop, QualityLoopResult};
use crate::types::Result;

#[derive(Default)]
pub struct GenerateOptions {
    pub output: Option<PathBuf>,
    pub resume: bool,
    pub dry_run: bool,
    pub config_path: Option<PathBuf>,
}

pub fn run(options: GenerateOptions) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(run_async(options))
}

async fn run_async(options: GenerateOptions) -> Result<()> {
    let project_root = std::env::current_dir()?;

    println!("claudegen - Claude Code Plugin Generator\n");
    println!("Using Quality Loop with Adaptive Pipeline");

    // Load config from project root for consistency
    let config = ConfigLoader::load_for_project(&project_root, options.config_path.as_deref())?;

    if options.dry_run {
        println!("\nDry run mode:\n");
        println!("  Project: {}", project_root.display());
        println!("  Depth: {:?}", config.analysis.depth);
        println!("  Quality floor: {}", config.convergence.quality_floor);
        println!("  Target quality: {}", config.convergence.target_quality);
        println!("  Quality loop: {}", config.quality_loop().enabled);
        println!("  Multi-agent: {}", config.multi_agent().enabled);
        return Ok(());
    }

    // Create budget and metrics for tracking
    let session_id = uuid::Uuid::new_v4().to_string();
    let budget = create_shared_budget(config.budget.total_tokens);
    let metrics = create_shared_metrics(&session_id);

    // Create providers with tracking enabled
    let providers = create_providers(&config)
        .await?
        .with_tracking(budget.clone(), metrics.clone());

    let mut quality_loop = QualityLoop::new(project_root.clone(), providers, config.clone())
        .budget(budget.clone())
        .metrics(metrics.clone());

    if let Some(output_dir) = options.output {
        quality_loop = quality_loop.output_dir(output_dir);
    }

    if options.resume {
        quality_loop = quality_loop.resume(true);
    }

    println!("\nStarting pipeline...\n");

    match quality_loop.run().await {
        Ok(result) => {
            quality_loop.write_output(&result).await?;
            print_quality_loop_result(&result);
            print_summary(&result.output);
            print_metrics(&metrics, &budget);
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
    println!(
        "  - Final confidence: {:.1}%",
        result.final_confidence * 100.0
    );

    if !result.gaps_discovered.is_empty() {
        println!("  - Gaps discovered: {}", result.gaps_discovered.len());
        for gap in result.gaps_discovered.iter().take(3) {
            println!(
                "    • [{}] {} (iteration {})",
                gap.area, gap.description, gap.iteration_found
            );
        }
    }

    // Deep Review Results
    if result.deep_review_attempts > 0 {
        let status = if result.deep_review_passed {
            "✓"
        } else {
            "✗"
        };
        println!(
            "\n🔍 Deep Review: {} ({} attempts)",
            status, result.deep_review_attempts
        );
        if !result.deep_review_passed {
            println!("  ⚠️  Two-pass verification did not pass");
        }
    }
}

fn print_metrics(
    metrics: &crate::ai::metrics::SharedMetrics,
    budget: &crate::ai::budget::SharedBudget,
) {
    let summary = metrics.summary();
    let budget_stats = budget.stats();

    println!("\n📈 Token Usage:");
    println!(
        "  - Total: {} (input: {}, output: {})",
        summary.total_tokens, summary.input_tokens, summary.output_tokens
    );
    println!("  - API calls: {}", summary.api_calls);
    println!("  - Avg latency: {:.0}ms", summary.avg_latency_ms);
    println!("  - Estimated cost: ${:.4}", summary.total_cost_usd);

    println!("\n💰 Budget:");
    println!(
        "  - Used: {}/{} ({:.1}%)",
        budget_stats.consumed,
        budget_stats.total_budget,
        budget_stats.utilization * 100.0
    );
    if budget_stats.is_warning {
        println!("  ⚠️  Budget warning threshold exceeded");
    }
}

/// Build tiered ProviderSet from config for phase-based model routing
async fn create_providers(config: &Config) -> Result<ProviderSet> {
    let llm = &config.llm;

    let base_config = ProviderConfig {
        provider: llm.provider.clone(),
        model: Some(llm.default_model.clone()),
        timeout_secs: llm.timeout_secs,
        temperature: llm.temperature,
        api_key: None, // Let provider read from env
        api_base: None,
        max_tokens: llm.max_tokens,
        extended_context: llm.context.use_extended_context,
    };

    create_provider_set(&base_config, llm, &config.circuit_breaker).await
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
            "\n⚠️  Tier 1 content filtered: {} items",
            output.tier_filter_result.tier1_count
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
    let total_refs =
        cv.evidence_traceability.valid_references + cv.evidence_traceability.invalid_references;
    if total_refs > 0 {
        println!(
            "  - Evidence Traceability: {}/{} references valid ({:.0}%)",
            cv.evidence_traceability.valid_references,
            total_refs,
            cv.evidence_traceability.coverage_score * 100.0
        );
    }

    if !cv.plan_consistency.missing_coverage.is_empty() {
        println!(
            "  - Plan Consistency: {} items missing from plan",
            cv.plan_consistency.missing_coverage.len()
        );
    }

    if cv.evidence_traceability.invalid_references > 0 {
        println!(
            "\n⚠️  Invalid file references: {}",
            cv.evidence_traceability.invalid_references
        );
    }
}
