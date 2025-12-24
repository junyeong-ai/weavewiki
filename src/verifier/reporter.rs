use std::fs;
use std::path::Path;

use crate::types::{
    IssueSeverity, Result, ValidationError, ValidationErrorKind, VerificationReport, WeaveError,
};

pub struct Reporter;

impl Reporter {
    pub fn generate_json<P: AsRef<Path>>(
        report: &VerificationReport,
        output_path: P,
    ) -> Result<()> {
        let json = serde_json::to_string_pretty(report).map_err(|e| {
            WeaveError::Validation(ValidationError::new(
                ValidationErrorKind::Format,
                format!("Failed to serialize report: {}", e),
            ))
        })?;

        fs::write(output_path, json)?;

        Ok(())
    }

    pub fn print_summary(report: &VerificationReport) {
        println!("Verification Report");
        println!("══════════════════════════════════════");
        println!();
        println!("Claims: {}", report.total_claims);
        println!("  Verified: {} ✓", report.verified);
        println!("  Stale: {} ⚠", report.stale);
        println!("  Invalid: {} ✗", report.invalid);
        println!();

        if report.issues.is_empty() {
            println!("No issues found.");
        } else {
            println!("Issues ({}):", report.issues.len());
            println!();

            for issue in &report.issues {
                let icon = match issue.severity {
                    IssueSeverity::Error => "✗",
                    IssueSeverity::Warning => "⚠",
                    IssueSeverity::Info => "ℹ",
                };

                println!(
                    "{} [{}] {}",
                    icon,
                    format!("{:?}", issue.severity).to_uppercase(),
                    issue.message
                );

                if let Some(ref suggestion) = issue.suggestion {
                    println!("  → {}", suggestion);
                }

                if issue.auto_fixable {
                    println!("  (auto-fixable with --fix)");
                }

                println!();
            }
        }

        println!("══════════════════════════════════════");

        if report.has_errors() {
            println!("Result: FAILED ({} errors)", report.error_count());
        } else if report.warning_count() > 0 {
            println!("Result: PASSED with warnings ({})", report.warning_count());
        } else {
            println!("Result: PASSED ✓");
        }
    }

    pub fn print_filtered(report: &VerificationReport, min_severity: IssueSeverity) {
        let filtered: Vec<_> = report
            .issues
            .iter()
            .filter(|i| i.severity <= min_severity)
            .collect();

        if filtered.is_empty() {
            println!("No issues at severity {:?} or higher.", min_severity);
            return;
        }

        println!("Issues ({}):", filtered.len());
        println!();

        for issue in filtered {
            let icon = match issue.severity {
                IssueSeverity::Error => "✗",
                IssueSeverity::Warning => "⚠",
                IssueSeverity::Info => "ℹ",
            };

            println!(
                "{} [{}] {}",
                icon,
                format!("{:?}", issue.severity).to_uppercase(),
                issue.message
            );

            if let Some(ref suggestion) = issue.suggestion {
                println!("  → {}", suggestion);
            }

            println!();
        }
    }
}
