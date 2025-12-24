use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::runtime::Runtime;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Parse analysis mode from string
fn parse_analysis_mode(s: &str) -> Result<weavewiki::config::AnalysisMode, String> {
    match s.to_lowercase().as_str() {
        "fast" => Ok(weavewiki::config::AnalysisMode::Fast),
        "standard" => Ok(weavewiki::config::AnalysisMode::Standard),
        "deep" => Ok(weavewiki::config::AnalysisMode::Deep),
        _ => Err(format!(
            "Invalid mode '{}'. Valid values: fast, standard, deep",
            s
        )),
    }
}

/// Parse project scale from string
fn parse_project_scale(s: &str) -> Result<weavewiki::config::ProjectScale, String> {
    match s.to_lowercase().as_str() {
        "small" => Ok(weavewiki::config::ProjectScale::Small),
        "medium" => Ok(weavewiki::config::ProjectScale::Medium),
        "large" => Ok(weavewiki::config::ProjectScale::Large),
        "enterprise" => Ok(weavewiki::config::ProjectScale::Enterprise),
        _ => Err(format!(
            "Invalid scale '{}'. Valid values: small, medium, large, enterprise",
            s
        )),
    }
}

#[derive(Parser)]
#[command(name = "weavewiki")]
#[command(
    version,
    about = "AI-driven wiki documentation generator for codebases"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, short, default_value = ".weavewiki/config.yaml")]
    config: PathBuf,

    #[arg(long)]
    verbose: bool,

    #[arg(long, short)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize WeaveWiki in the current directory
    Init {
        #[arg(long, short, help = "Overwrite existing initialization")]
        force: bool,
    },

    /// Build knowledge graph from source code
    Build {
        #[arg(long, help = "Run full build from scratch (clear existing graph)")]
        full: bool,
        #[arg(long, help = "Path to build")]
        path: Option<PathBuf>,
    },

    /// Generate AI-driven wiki documentation
    Generate {
        #[arg(long, short, help = "Output directory for wiki")]
        output: Option<PathBuf>,
        #[arg(long, help = "LLM provider (claude-code, openai, ollama)")]
        provider: Option<String>,
        #[arg(long, help = "Model to use")]
        model: Option<String>,
        #[arg(long, help = "Resume from previous session")]
        resume: bool,
        #[arg(long, help = "Show current progress status")]
        status: bool,
        #[arg(long, help = "Auto-commit generated documentation to git")]
        commit: bool,

        // Pipeline options
        #[arg(long, value_parser = parse_analysis_mode, help = "Analysis mode: fast, standard, deep (default: standard)")]
        mode: Option<weavewiki::config::AnalysisMode>,
        #[arg(long, value_parser = parse_project_scale, help = "Scale override: small, medium, large, enterprise")]
        scale: Option<weavewiki::config::ProjectScale>,
        #[arg(long, help = "Quality target override (0.0-1.0)")]
        quality_target: Option<f32>,
        #[arg(long, help = "Max refinement turns override")]
        max_turns: Option<u8>,
        #[arg(long = "dry-run", help = "Show configuration only, don't run")]
        dry_run: bool,
    },

    /// Query the knowledge graph (structural)
    Query {
        #[arg(help = "Node ID or path to query")]
        query: String,
        #[arg(
            short = 'd',
            long,
            default_value = "10",
            help = "Maximum depth for dependencies"
        )]
        depth: u32,
        #[arg(
            short = 'f',
            long,
            default_value = "text",
            help = "Output format: text, json"
        )]
        format: String,
    },

    /// Validate knowledge base against source code
    Validate {
        #[arg(help = "Path to validate")]
        path: Option<PathBuf>,
        #[arg(
            long,
            default_value = ".weavewiki/validation-report.json",
            help = "Report output path"
        )]
        report: PathBuf,
        #[arg(long, default_value = "warning", help = "Minimum severity to report")]
        severity: String,
    },

    /// Show project status
    Status {
        #[arg(
            short = 'f',
            long,
            default_value = "text",
            help = "Output format: text, json"
        )]
        format: String,
        #[arg(short = 'd', long, help = "Show detailed information")]
        detailed: bool,
    },

    /// Clean up WeaveWiki data
    Clean {
        #[arg(long, help = "Remove all WeaveWiki data")]
        all: bool,
        #[arg(long, help = "Only clear the wiki cache")]
        cache: bool,
        #[arg(long, help = "Only clear checkpoints")]
        checkpoints: bool,
        #[arg(long, help = "Only clear incomplete sessions")]
        sessions: bool,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration (merged from all sources)
    Show {
        #[arg(short = 'g', long, help = "Show global config file only")]
        global: bool,
        #[arg(
            short = 'f',
            long,
            default_value = "text",
            help = "Output format: text, json, yaml"
        )]
        format: String,
    },
    /// Show configuration file paths
    Path,
    /// Edit configuration file with $EDITOR
    Edit {
        #[arg(long, short, help = "Edit global config")]
        global: bool,
    },
    /// Initialize configuration
    Init {
        #[arg(long, short, help = "Initialize global config")]
        global: bool,
        #[arg(long, help = "Overwrite existing config")]
        force: bool,
    },
}

/// Set up panic handler for graceful error reporting
fn setup_panic_handler() {
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // Extract panic message
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        // Log the panic
        eprintln!("\n\x1b[1;31m━━━ PANIC ━━━\x1b[0m");
        eprintln!("\x1b[31mWeaveWiki encountered an unexpected error:\x1b[0m");
        eprintln!("  {}", message);

        if let Some(location) = panic_info.location() {
            eprintln!(
                "\x1b[90mLocation: {}:{}:{}\x1b[0m",
                location.file(),
                location.line(),
                location.column()
            );
        }

        eprintln!("\n\x1b[33mPlease report this issue at:\x1b[0m");
        eprintln!("  https://github.com/user/weavewiki/issues");
        eprintln!();

        // Call default hook for backtrace (if RUST_BACKTRACE=1)
        default_hook(panic_info);
    }));
}

fn main() -> ExitCode {
    // Install panic handler first
    setup_panic_handler();

    // Run the actual CLI
    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\x1b[31mError:\x1b[0m {}", e);
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        "debug"
    } else if cli.quiet {
        "error"
    } else {
        "info"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Init { force } => {
            weavewiki::cli::commands::init::run(force)?;
        }
        Commands::Build { full, path } => {
            weavewiki::cli::commands::analyze::run(full, path, false)?;
        }
        Commands::Generate {
            output,
            provider,
            model,
            resume,
            status,
            commit,
            mode: analysis_mode,
            scale,
            quality_target,
            max_turns,
            dry_run,
        } => {
            use weavewiki::cli::commands::wiki::{MultiAgentOptions, WikiMode, WikiRunOptions};

            let wiki_mode = if status {
                WikiMode::Status
            } else if resume {
                WikiMode::Resume
            } else {
                WikiMode::Generate
            };

            weavewiki::cli::commands::wiki::run_with_options(WikiRunOptions {
                output,
                provider,
                model,
                mode: wiki_mode,
                commit,
                multi_agent: MultiAgentOptions {
                    mode: analysis_mode,
                    scale,
                    quality_target,
                    max_turns,
                    verbose: cli.verbose,
                    dry_run,
                },
            })?;
        }
        Commands::Query {
            query,
            depth,
            format,
        } => {
            weavewiki::cli::commands::query::run(&query, depth, &format)?;
        }
        Commands::Validate {
            path,
            report,
            severity,
        } => {
            weavewiki::cli::commands::validate::run(path, &report, &severity)?;
        }
        Commands::Status { format, detailed } => {
            weavewiki::cli::commands::status::run(&format, detailed)?;
        }
        Commands::Clean {
            all,
            cache,
            checkpoints,
            sessions,
        } => {
            let rt = Runtime::new()?;
            rt.block_on(weavewiki::cli::commands::clean::run(
                all,
                cache,
                checkpoints,
                sessions,
            ))?;
        }
        Commands::Config { action } => match action {
            ConfigAction::Show { global, format } => {
                weavewiki::cli::commands::config::show(global, &format)?;
            }
            ConfigAction::Path => {
                weavewiki::cli::commands::config::path()?;
            }
            ConfigAction::Edit { global } => {
                weavewiki::cli::commands::config::edit(global)?;
            }
            ConfigAction::Init { global, force } => {
                if global {
                    weavewiki::cli::commands::config::init_global(force)?;
                } else {
                    weavewiki::cli::commands::config::init_project()?;
                }
            }
        },
    }

    Ok(())
}
