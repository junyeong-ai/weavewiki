//! Wiki Generation Command
//!
//! AI-driven documentation generation with complete coverage for
//! large codebases and monorepos.
//!
//! Commands:
//! - generate: Full documentation generation
//! - generate --resume: Resume from previous session
//! - generate --status: Show current progress

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tokio::runtime::Runtime;
use tracing::{info, warn};

use crate::ai::preflight::PreflightCheck;
use crate::ai::provider::{ChainConfig, ProviderChainBuilder, ProviderConfig, create_provider};
use crate::config::{AnalysisMode, ProjectScale};
use crate::config::{Config, ConfigLoader};
use crate::storage::{Database, SharedDatabase};
use crate::types::{Result, WeaveError};
use crate::wiki::exhaustive::{MultiAgentConfig, MultiAgentPipeline, SessionStatus};

/// Wiki command mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WikiMode {
    /// Full generation (default)
    #[default]
    Generate,
    /// Resume from previous session
    Resume,
    /// Show progress status
    Status,
}

/// Multi-agent pipeline options
#[derive(Debug, Clone, Default)]
pub struct MultiAgentOptions {
    /// Analysis mode: fast, standard, deep
    pub mode: Option<AnalysisMode>,
    /// Scale override: small, medium, large, enterprise
    pub scale: Option<ProjectScale>,
    /// Quality target override (0.0-1.0)
    pub quality_target: Option<f32>,
    /// Max refinement turns override
    pub max_turns: Option<u8>,
    /// Show verbose output
    pub verbose: bool,
    /// Dry run (show config only)
    pub dry_run: bool,
}

/// Wiki run options (consolidated parameters)
#[derive(Debug, Clone, Default)]
pub struct WikiRunOptions {
    /// Output directory
    pub output: Option<PathBuf>,
    /// LLM provider override
    pub provider: Option<String>,
    /// Model override
    pub model: Option<String>,
    /// Wiki generation mode
    pub mode: WikiMode,
    /// Auto-commit after generation
    pub commit: bool,
    /// Multi-agent specific options
    pub multi_agent: MultiAgentOptions,
}

/// Run wiki generation with options
pub fn run_with_options(options: WikiRunOptions) -> Result<()> {
    let WikiRunOptions {
        output,
        provider,
        model,
        mode,
        commit,
        multi_agent,
    } = options;

    let weavewiki_dir = PathBuf::from(".weavewiki");

    if !weavewiki_dir.exists() {
        return Err(WeaveError::NotInitialized);
    }

    let _config = ConfigLoader::load()?;

    let db_path = weavewiki_dir.join("graph/graph.db");
    if !db_path.exists() {
        return Err(WeaveError::NotInitialized);
    }

    let db: SharedDatabase = Arc::new(Database::open(&db_path)?);
    db.initialize()?;

    let output_dir = output.unwrap_or_else(|| weavewiki_dir.join("wiki"));

    let result = match mode {
        WikiMode::Status => run_status(&db),
        WikiMode::Resume => run_resume(db.clone(), &output_dir, provider, model),
        WikiMode::Generate => run_generate(db.clone(), &output_dir, provider, model, multi_agent),
    };

    // Auto-commit if enabled and generation succeeded
    if result.is_ok() && commit && matches!(mode, WikiMode::Generate | WikiMode::Resume) {
        git_auto_commit(&output_dir)?;
    }

    result
}

/// Get canonical project path for session queries
fn get_canonical_project_path() -> Result<String> {
    let cwd = std::env::current_dir().map_err(WeaveError::Io)?;
    let canonical = cwd
        .canonicalize()
        .unwrap_or(cwd)
        .to_string_lossy()
        .to_string();
    Ok(canonical)
}

/// Session info from database
struct SessionInfo {
    id: String,
    status: SessionStatus,
    current_phase: u8,
    total_files: usize,
    files_analyzed: usize,
    quality_score: f32,
    analysis_mode: String,
    detected_scale: String,
}

/// Get latest session for project
fn get_latest_session(db: &Database, project_path: &str) -> Result<Option<SessionInfo>> {
    let conn = db.connection()?;
    let result: std::result::Result<SessionInfo, _> = conn.query_row(
        r#"SELECT id, status, current_phase, total_files, files_analyzed,
                  quality_score, analysis_mode, detected_scale
           FROM doc_sessions
           WHERE project_path = ?1
           ORDER BY started_at DESC
           LIMIT 1"#,
        [project_path],
        |row| {
            Ok(SessionInfo {
                id: row.get(0)?,
                status: SessionStatus::parse(row.get::<_, String>(1)?.as_str()),
                current_phase: row.get::<_, i32>(2)? as u8,
                total_files: row.get::<_, i32>(3)? as usize,
                files_analyzed: row.get::<_, i32>(4)? as usize,
                quality_score: row.get(5)?,
                analysis_mode: row.get(6)?,
                detected_scale: row.get(7)?,
            })
        },
    );

    match result {
        Ok(session) => Ok(Some(session)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Check for resumable session
fn check_resumable_session(db: &Database, project_path: &str) -> Result<Option<String>> {
    let conn = db.connection()?;
    let result: std::result::Result<(String, String), _> = conn.query_row(
        r#"SELECT id, status FROM doc_sessions
           WHERE project_path = ?1
             AND status IN ('running', 'paused')
           ORDER BY started_at DESC
           LIMIT 1"#,
        [project_path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok((id, _)) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Show current progress status
fn run_status(db: &Database) -> Result<()> {
    let project_path = get_canonical_project_path()?;

    match get_latest_session(db, &project_path)? {
        Some(session) => {
            println!("\n📊 Wiki Documentation Status\n");
            println!("  Session:      {}", session.id);
            println!("  Phase:        {}/6", session.current_phase);
            println!("  Status:       {:?}", session.status);
            println!("  Mode:         {}", session.analysis_mode);
            println!("  Scale:        {}", session.detected_scale);
            println!();
            println!(
                "  Files analyzed:    {}/{}",
                session.files_analyzed, session.total_files
            );
            println!("  Quality score:     {:.1}%", session.quality_score * 100.0);

            if session.status == SessionStatus::Completed {
                println!("\n  ✅ Documentation complete!");
            } else if session.status == SessionStatus::Failed {
                println!("\n  ❌ Generation failed. Check logs for details.");
            } else {
                println!("\n  💡 Run 'weavewiki generate --resume' to continue");
            }
        }
        None => {
            println!("\n  No wiki generation in progress.");
            println!("  Run 'weavewiki generate' to start.");
        }
    }

    Ok(())
}

/// Resume from previous session
fn run_resume(
    db: SharedDatabase,
    output_dir: &Path,
    provider: Option<String>,
    model: Option<String>,
) -> Result<()> {
    println!("\n⏩ Resuming wiki generation...\n");

    fs::create_dir_all(output_dir)?;

    let config = ConfigLoader::load()?;
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_path = get_canonical_project_path()?;

    // Build provider with fallback chain support
    let provider_config = ProviderConfig {
        provider: provider.unwrap_or_else(|| config.llm.provider.clone()),
        model: model.or_else(|| Some(config.llm.model.clone())),
        timeout_secs: config.llm.timeout_secs,
        temperature: config.llm.temperature,
        ..Default::default()
    };
    let llm_provider = create_provider_chain(&config, &provider_config)?;
    info!("Using LLM provider: {}", llm_provider.name());

    // Get latest session
    let session = get_latest_session(&db, &project_path)?
        .ok_or_else(|| WeaveError::Session("No previous session found".to_string()))?;

    println!("  Resuming session: {}", session.id);
    println!("  Current phase:    {}/6", session.current_phase);
    println!("  Mode:             {}", session.analysis_mode);

    // Create pipeline from existing session
    let pipeline = MultiAgentPipeline::resume_session(
        db,
        session.id.clone(),
        llm_provider,
        &project_root,
        output_dir,
    );

    // Load checkpoint and resume
    let checkpoint = pipeline.load_checkpoint()?;
    let result = if let Some(cp) = checkpoint {
        println!(
            "  Checkpoint:       Phase {} with {} files",
            cp.last_completed_phase,
            cp.files.len()
        );

        let rt = Runtime::new().map_err(|e| WeaveError::Session(e.to_string()))?;
        rt.block_on(pipeline.resume(cp))?
    } else {
        // No checkpoint data - start fresh but reuse session
        println!("  ⚠️  No checkpoint found, starting fresh");

        let rt = Runtime::new().map_err(|e| WeaveError::Session(e.to_string()))?;
        rt.block_on(pipeline.run())?
    };

    let md_count = count_md_files(output_dir);
    print_multi_agent_result(&result, output_dir, md_count);
    Ok(())
}

/// Validate CLI options for multi-agent pipeline
fn validate_options(options: &MultiAgentOptions) -> Result<()> {
    // Validate quality_target range
    if let Some(qt) = options.quality_target
        && !(0.0..=1.0).contains(&qt)
    {
        return Err(WeaveError::Config(format!(
            "quality_target must be between 0.0 and 1.0 (got {})",
            qt
        )));
    }

    // Validate max_turns range
    if let Some(mt) = options.max_turns
        && (mt == 0 || mt > 20)
    {
        return Err(WeaveError::Config(format!(
            "max_turns must be between 1 and 20 (got {})",
            mt
        )));
    }

    Ok(())
}

/// Generate documentation
fn run_generate(
    db: SharedDatabase,
    output_dir: &Path,
    provider: Option<String>,
    model: Option<String>,
    options: MultiAgentOptions,
) -> Result<()> {
    // Validate options before proceeding
    validate_options(&options)?;

    let analysis_mode = options.mode.unwrap_or(AnalysisMode::Standard);
    println!(
        "\n🚀 Starting Multi-Agent Documentation Pipeline (mode={})...\n",
        analysis_mode
    );

    fs::create_dir_all(output_dir)?;

    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_path = get_canonical_project_path()?;
    let config = ConfigLoader::load()?;

    // Check for resumable session
    if let Some(session_id) = check_resumable_session(&db, &project_path)? {
        println!("  💡 Found resumable session: {}", session_id);
        println!("     Use --resume to continue from last checkpoint\n");
    }

    // Build provider with fallback chain support
    let provider_config = ProviderConfig {
        provider: provider.unwrap_or_else(|| config.llm.provider.clone()),
        model: model.or_else(|| Some(config.llm.model.clone())),
        timeout_secs: config.llm.timeout_secs,
        temperature: config.llm.temperature,
        ..Default::default()
    };
    let llm_provider = create_provider_chain(&config, &provider_config)?;
    info!("Using LLM provider: {}", llm_provider.name());

    // Build multi-agent config
    let ma_config = MultiAgentConfig {
        mode: analysis_mode,
        scale_override: options.scale,
        quality_target_override: options.quality_target,
        max_turns_override: options.max_turns,
        show_progress: true,
        verbose: options.verbose,
        dry_run: options.dry_run,
    };

    // Create runtime for async operations
    let rt = Runtime::new().map_err(|e| WeaveError::Session(e.to_string()))?;

    // Run preflight checks before creating pipeline
    rt.block_on(run_preflight_checks(
        &db,
        llm_provider.as_ref(),
        &project_root,
        &config,
    ))?;

    // Create pipeline
    let pipeline =
        MultiAgentPipeline::new(db, llm_provider, &project_root, output_dir).with_config(ma_config);

    // Auto-detect scale
    let detected_scale = pipeline.detect_scale();
    let actual_scale = options.scale.unwrap_or(detected_scale);
    println!("  Scale: {} (detected: {})", actual_scale, detected_scale);

    if options.dry_run {
        println!("\n  [Dry Run] Configuration:");
        println!("    Mode:           {}", analysis_mode);
        println!("    Scale:          {}", actual_scale);
        println!(
            "    Quality Target: {:.0}%",
            options.quality_target.unwrap_or(0.8) * 100.0
        );
        println!("    Max Turns:      {}", options.max_turns.unwrap_or(3));
        println!("    Output:         {}", output_dir.display());
        return Ok(());
    }

    // Run pipeline
    let result = rt.block_on(pipeline.run_with_recovery())?;

    // Show output stats
    let md_count = count_md_files(output_dir);
    print_multi_agent_result(&result, output_dir, md_count);
    Ok(())
}

/// Print multi-agent pipeline result
fn print_multi_agent_result(
    result: &crate::wiki::exhaustive::MultiAgentResult,
    output_dir: &Path,
    md_file_count: usize,
) {
    let status_icon = if result.target_met { "✅" } else { "⚠️" };
    println!("\n{} Multi-Agent Pipeline Complete!\n", status_icon);
    println!("  Quality Score:    {:.1}%", result.quality_score * 100.0);
    println!("  Quality Target:   {:.1}%", result.quality_target * 100.0);
    println!(
        "  Target Met:       {}",
        if result.target_met { "Yes" } else { "No" }
    );
    println!("  Refinement Turns: {}", result.refinement_turns);
    println!("  Files Analyzed:   {}", result.files_analyzed);
    println!("  Pages Generated:  {}", result.pages_generated);
    println!("  Markdown Files:   {}", md_file_count);
    println!("  Duration:         {}s", result.duration_secs);
    println!();
    println!("  Output: {}", output_dir.display());
}

/// Create provider chain with fallback support
fn create_provider_chain(
    config: &Config,
    primary_config: &ProviderConfig,
) -> Result<crate::ai::provider::SharedProvider> {
    // Create primary provider
    let primary = create_provider(primary_config)?;

    // Check if fallback is configured
    if config.llm.fallback_provider.is_some() || config.llm.fallback_model.is_some() {
        let fallback_config = ProviderConfig {
            provider: config
                .llm
                .fallback_provider
                .clone()
                .unwrap_or_else(|| primary_config.provider.clone()),
            model: config
                .llm
                .fallback_model
                .clone()
                .or_else(|| primary_config.model.clone()),
            timeout_secs: primary_config.timeout_secs,
            temperature: primary_config.temperature,
            ..Default::default()
        };

        // Only add fallback if it's different from primary
        if fallback_config.provider != primary_config.provider
            || fallback_config.model != primary_config.model
        {
            let fallback = create_provider(&fallback_config)?;
            info!(
                "Provider chain: {} → {} (fallback)",
                primary.name(),
                fallback.name()
            );

            let chain = ProviderChainBuilder::new()
                .add_shared(primary)
                .add_shared(fallback)
                .with_config(ChainConfig {
                    max_total_attempts: 6,
                    cost_optimize: true,
                    ..Default::default()
                })
                .build();

            return Ok(Arc::new(chain));
        }
    }

    // No fallback configured, return single provider
    Ok(primary)
}

/// Run preflight validation checks
async fn run_preflight_checks(
    _db: &Database,
    provider: &dyn crate::ai::provider::LlmProvider,
    project_root: &Path,
    _config: &Config,
) -> Result<()> {
    let preflight = PreflightCheck::new();

    println!("  🔍 Running preflight checks...");

    let mut result = crate::ai::preflight::PreflightResult::new();

    // Check database
    let db_path = project_root.join(".weavewiki/graph/graph.db");
    preflight.check_database(&db_path, &mut result);

    // Check provider health
    match provider.health_check().await {
        Ok(true) => {
            println!("  ✓ Provider '{}' is healthy", provider.name());
        }
        Ok(false) | Err(_) => {
            warn!(
                "  ⚠ Provider '{}' health check inconclusive",
                provider.name()
            );
        }
    }

    if !result.passed {
        for error in &result.errors {
            println!("  ✗ {}", error);
        }
        return Err(WeaveError::Config(format!(
            "Preflight checks failed: {}",
            result.errors.join(", ")
        )));
    }

    for warning in &result.warnings {
        println!("  ⚠ {}", warning);
    }

    println!("  ✓ Preflight checks passed\n");
    Ok(())
}

fn count_md_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                count += count_md_files(&path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                count += 1;
            }
        }
    }
    count
}

/// Git auto-commit for generated documentation
fn git_auto_commit(output_dir: &Path) -> Result<()> {
    println!("\n📝 Auto-committing documentation changes...\n");

    // Check if we're in a git repository
    let git_check = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output();

    let is_git_repo = git_check
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !is_git_repo {
        warn!("Not a git repository, skipping auto-commit");
        println!("  ⚠ Not a git repository, skipping auto-commit");
        return Ok(());
    }

    // Safety Check 1: Check if current branch is main/master
    let branch_result = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    if let Ok(output) = branch_result
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch == "main" || branch == "master" {
            warn!(
                "Auto-commit blocked: cannot commit to {} branch without --force",
                branch
            );
            println!("  ⚠ Auto-commit blocked: currently on '{}' branch", branch);
            println!(
                "     To commit to {}, use a dedicated feature branch",
                branch
            );
            println!("     or commit manually after review");
            return Ok(());
        }
    }

    // Safety Check 2: Check for other unstaged changes (outside output_dir)
    let status_result = Command::new("git").args(["status", "--porcelain"]).output();

    if let Ok(output) = status_result
        && output.status.success()
    {
        let status_output = String::from_utf8_lossy(&output.stdout);
        let output_dir_str = output_dir.to_string_lossy();

        // Check if there are changes outside of output_dir
        let has_other_changes = status_output.lines().any(|line| {
            if line.len() < 3 {
                return false;
            }
            let file_path = &line[3..]; // Skip status indicators (e.g., "M ", "?? ")
            !file_path.starts_with(output_dir_str.as_ref())
        });

        if has_other_changes {
            warn!("Auto-commit blocked: detected changes outside output directory");
            println!("  ⚠ Auto-commit blocked: detected changes outside wiki directory");
            println!("     The following files have uncommitted changes:");
            for line in status_output.lines() {
                if line.len() >= 3 {
                    let file_path = &line[3..];
                    if !file_path.starts_with(output_dir_str.as_ref()) {
                        println!("       {}", line);
                    }
                }
            }
            println!("     Please commit or stash these changes first,");
            println!("     or commit wiki documentation manually");
            return Ok(());
        }
    }

    // Stage wiki output directory
    let add_result = Command::new("git")
        .args(["add", &output_dir.to_string_lossy()])
        .output()
        .map_err(WeaveError::Io)?;

    if !add_result.status.success() {
        let stderr = String::from_utf8_lossy(&add_result.stderr);
        warn!("git add failed: {}", stderr);
        println!("  ⚠ Failed to stage changes: {}", stderr);
        return Ok(());
    }

    // Check if there are changes to commit
    let diff_result = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .output()
        .map_err(WeaveError::Io)?;

    if diff_result.status.success() {
        println!("  ℹ No changes to commit");
        return Ok(());
    }

    // Safety Check 3: User confirmation
    println!("  The following documentation changes will be committed:");
    let diff_stat = Command::new("git")
        .args(["diff", "--cached", "--stat"])
        .output();

    if let Ok(output) = diff_stat
        && output.status.success()
    {
        let stat_output = String::from_utf8_lossy(&output.stdout);
        for line in stat_output.lines() {
            println!("     {}", line);
        }
    }

    println!();
    println!("  Proceed with auto-commit? [Y/n]: ");

    // Read user input
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => {
            let response = input.trim().to_lowercase();
            if !response.is_empty() && response != "y" && response != "yes" {
                println!("  ℹ Auto-commit cancelled by user");
                return Ok(());
            }
        }
        Err(e) => {
            warn!("Failed to read user input: {}", e);
            println!("  ⚠ Failed to read confirmation, skipping auto-commit");
            return Ok(());
        }
    }

    // Get current timestamp for commit message
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

    // Create commit message
    let commit_message = format!(
        "docs(wiki): auto-generated documentation update\n\n\
         Generated by WeaveWiki at {}\n\n\
         This commit contains AI-generated documentation based on\n\
         source code analysis.",
        timestamp
    );

    // Commit changes
    let commit_result = Command::new("git")
        .args(["commit", "-m", &commit_message])
        .output()
        .map_err(WeaveError::Io)?;

    if commit_result.status.success() {
        let hash_result = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output();

        let hash = hash_result
            .ok()
            .filter(|r| r.status.success())
            .map(|r| String::from_utf8_lossy(&r.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        println!("  ✓ Committed documentation changes ({})", hash);
        info!("Auto-committed wiki documentation: {}", hash);
    } else {
        let stderr = String::from_utf8_lossy(&commit_result.stderr);
        warn!("git commit failed: {}", stderr);
        println!("  ⚠ Failed to commit: {}", stderr);
    }

    Ok(())
}
