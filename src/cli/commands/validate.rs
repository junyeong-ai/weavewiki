//! Validate Command
//!
//! Validates knowledge graph claims against actual source code.

use std::path::{Path, PathBuf};

use crate::cli::util::require_graph_db_path;
use crate::storage::Database;
use crate::types::{
    Claim, ClaimEvidence, ClaimType, InformationTier, IssueSeverity, Result, WeaveError,
};
use crate::verifier::{Reporter, VerificationEngine};

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
        "error" => IssueSeverity::Error,
        "warning" => IssueSeverity::Warning,
        _ => IssueSeverity::Info,
    };

    if severity != "all" {
        Reporter::print_filtered(&report, min_severity);
    } else {
        Reporter::print_summary(&report);
    }

    let weavewiki_dir = Path::new(".weavewiki");
    if report_path.to_string_lossy() != "validation-report.json" || !report.issues.is_empty() {
        let output_path = weavewiki_dir.join(report_path);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Reporter::generate_json(&report, &output_path)?;
        println!();
        println!("Report saved to: {}", output_path.display());
    }

    if report.has_errors() {
        return Err(WeaveError::Verification(
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
                id: format!("claim:{}", id),
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
