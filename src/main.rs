use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::runtime::Runtime;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "claudegen")]
#[command(version, about = "Claude Code plugin generator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, short, default_value = ".claudegen/config.yaml")]
    config: PathBuf,

    #[arg(long)]
    verbose: bool,

    #[arg(long, short)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize claudegen in the current directory
    Init {
        #[arg(long, short, help = "Overwrite existing initialization")]
        force: bool,
    },

    /// Analyze codebase and build knowledge graph
    Analyze {
        #[arg(long, help = "Run full analysis from scratch")]
        full: bool,
        #[arg(long, help = "Path to analyze")]
        path: Option<PathBuf>,
    },

    /// Generate Claude Code plugin (CLAUDE.md, skills, agents, rules)
    Generate {
        #[arg(long, short, help = "Output directory")]
        output: Option<PathBuf>,
        #[arg(long, help = "Resume from previous session")]
        resume: bool,
        #[arg(long = "dry-run", help = "Show configuration only")]
        dry_run: bool,
    },

    /// Validate generated plugin output
    Validate {
        #[arg(help = "Path to validate")]
        path: Option<PathBuf>,
        #[arg(long, help = "Strict validation mode")]
        strict: bool,
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
    },

    /// Clean up claudegen data
    Clean {
        #[arg(long, help = "Remove all claudegen data")]
        all: bool,
        #[arg(long, help = "Only clear checkpoints")]
        checkpoints: bool,
    },
}

fn setup_panic_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        eprintln!("\n\x1b[1;31m━━━ PANIC ━━━\x1b[0m");
        eprintln!("\x1b[31mclaudegen encountered an unexpected error:\x1b[0m");
        eprintln!("  {message}");

        if let Some(location) = panic_info.location() {
            eprintln!(
                "\x1b[90mLocation: {}:{}:{}\x1b[0m",
                location.file(),
                location.line(),
                location.column()
            );
        }

        eprintln!("\n\x1b[33mPlease report this issue at:\x1b[0m");
        eprintln!("  https://github.com/junyeong-ai/claudegen/issues");
        eprintln!();

        default_hook(panic_info);
    }));
}

fn main() -> ExitCode {
    setup_panic_handler();

    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\x1b[31mError:\x1b[0m {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

fn run_cli() -> claudegen::types::Result<()> {
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
            claudegen::cli::commands::init::run(force)?;
        }
        Commands::Analyze { full, path } => {
            claudegen::cli::commands::analyze::run(full, path, false)?;
        }
        Commands::Generate {
            output,
            resume,
            dry_run,
        } => {
            use claudegen::cli::commands::generate::{GenerateOptions, run};
            run(GenerateOptions {
                output,
                resume,
                dry_run,
            })?;
        }
        Commands::Validate { path, strict } => {
            claudegen::cli::commands::validate::run(
                path,
                &PathBuf::from("validation-report.json"),
                if strict { "error" } else { "warning" },
            )?;
        }
        Commands::Status { format } => {
            claudegen::cli::commands::status::run(&format, false)?;
        }
        Commands::Clean { all, checkpoints } => {
            let rt = Runtime::new()?;
            rt.block_on(claudegen::cli::commands::clean::run(
                all,
                false,
                checkpoints,
                false,
            ))?;
        }
    }

    Ok(())
}
