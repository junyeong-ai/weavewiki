//! Validate Command
//!
//! Validates knowledge graph claims against actual source code.

use std::path::{Component, Path, PathBuf};

use crate::cli::util::require_graph_db_path;
use crate::storage::Database;
use crate::types::{
    Claim, ClaimEvidence, ClaimType, ClaudegenError, InformationTier, Result, Severity,
};
use crate::verifier::{Reporter, VerificationEngine};

/// Validate that a path does not contain path traversal sequences.
/// Returns sanitized path or error if traversal detected.
fn sanitize_report_path(path: &Path) -> Result<PathBuf> {
    let mut sanitized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => {} // Skip "."
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ClaudegenError::Config(format!(
                    "Invalid report path: '{}' contains path traversal or absolute components",
                    path.display()
                )));
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        return Err(ClaudegenError::Config(
            "Invalid report path: path is empty after sanitization".to_string(),
        ));
    }

    Ok(sanitized)
}

pub fn run(path: Option<PathBuf>, report_path: &Path, severity: &str) -> Result<()> {
    let db_path = require_graph_db_path()?;
    let root =
        path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    println!("Validating knowledge base...");
    println!("  Root: {}", root.display());

    let db = Database::open(&db_path)?;
    let engine = VerificationEngine::new(&root);

    let claims = load_claims_from_graph(&db)?;

    if claims.is_empty() {
        println!("No claims found in knowledge graph.");
        return Ok(());
    }

    println!("  Claims to verify: {}", claims.len());
    println!();

    let mut report = engine.verify_all(&claims)?;

    let tracked_files: Vec<String> = claims.iter().map(|c| c.evidence.file.clone()).collect();
    let stale_issues = engine.detect_stale_files(&tracked_files)?;
    for issue in stale_issues {
        report.add_issue(issue);
    }

    let min_severity = match severity.to_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" | "all" => Severity::Info,
        other => {
            tracing::warn!(value = other, "Unknown severity, using 'info'");
            Severity::Info
        }
    };

    if severity != "all" {
        Reporter::print_filtered(&report, min_severity);
    } else {
        Reporter::print_summary(&report);
    }

    let claudegen_dir = Path::new(".claudegen");
    if report_path.to_string_lossy() != "validation-report.json" || !report.issues.is_empty() {
        match sanitize_report_path(report_path) {
            Ok(safe_report_path) => {
                let output_path = claudegen_dir.join(safe_report_path);
                if let Some(parent) = output_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent) {
                        tracing::warn!(path = %parent.display(), error = %e, "Failed to create report directory");
                    }
                match Reporter::generate_json(&report, &output_path) {
                    Ok(()) => {
                        println!();
                        println!("Report saved to: {}", output_path.display());
                    }
                    Err(e) => {
                        tracing::warn!(path = %output_path.display(), error = %e, "Failed to save report");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Invalid report path, skipping report save");
            }
        }
    }

    if report.has_errors() {
        return Err(ClaudegenError::Verification(
            "Validation found errors. Check the report for details.".to_string(),
        ));
    }

    Ok(())
}

fn load_claims_from_graph(db: &Database) -> Result<Vec<Claim>> {
    let conn = db.connection()?;
    let mut stmt =
        conn.prepare("SELECT id, node_type, path, name, metadata, evidence FROM nodes LIMIT 1000")?;

    let claims: Vec<Claim> = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let node_type: String = row.get(1)?;
            let path: String = row.get(2)?;
            let name: String = row.get(3)?;

            let claim_type = match node_type.as_str() {
                "function" => ClaimType::FunctionSignature,
                "class" => ClaimType::ClassStructure,
                "file" => ClaimType::FileExists,
                "module" => ClaimType::ModuleExports,
                "api" => ClaimType::ApiEndpoint,
                _ => ClaimType::FileExists,
            };

            Ok(Claim {
                id: format!("claim:{id}"),
                claim_type,
                subject_id: id,
                statement: name,
                evidence: ClaimEvidence::new(path),
                tier: InformationTier::Fact,
                confidence: 1.0,
                verification: crate::types::VerificationStatus::Pending,
                created_at: chrono::Utc::now(),
                verified_at: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(claims)
}
