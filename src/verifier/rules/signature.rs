use std::path::Path;

use crate::types::{
    Claim, ClaimType, IssueSeverity, Result, VerificationIssue, VerificationStatus,
};

pub struct SignatureRule;

impl SignatureRule {
    pub fn verify(
        claim: &Claim,
        current_content: &str,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        if claim.claim_type != ClaimType::FunctionSignature {
            return Ok((VerificationStatus::Pending, None));
        }

        let expected_signature = &claim.statement;
        let file_path = &claim.evidence.file;

        if !Path::new(file_path).exists() {
            return Ok((
                VerificationStatus::Invalid,
                Some(
                    VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Error,
                        format!("File no longer exists: {}", file_path),
                    )
                    .with_suggestion("Remove this claim or update file path"),
                ),
            ));
        }

        if Self::signature_exists(current_content, expected_signature) {
            Ok((VerificationStatus::Verified, None))
        } else {
            let similar = Self::find_similar_signature(current_content, expected_signature);
            let suggestion = similar.map(|s| format!("Found similar: {}", s));

            Ok((
                VerificationStatus::Stale,
                Some(
                    VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Warning,
                        format!("Function signature changed: {}", expected_signature),
                    )
                    .with_suggestion(
                        suggestion
                            .unwrap_or_else(|| "Update claim with current signature".to_string()),
                    ),
                ),
            ))
        }
    }

    fn signature_exists(content: &str, signature: &str) -> bool {
        let normalized_sig = Self::normalize_signature(signature);
        let normalized_content = Self::normalize_content(content);
        normalized_content.contains(&normalized_sig)
    }

    fn normalize_signature(sig: &str) -> String {
        sig.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn normalize_content(content: &str) -> String {
        content
            .lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn find_similar_signature(content: &str, original: &str) -> Option<String> {
        let fn_name = Self::extract_function_name(original)?;

        for line in content.lines() {
            let trimmed = line.trim();
            if (trimmed.contains("function") || trimmed.contains("fn ") || trimmed.contains("def "))
                && trimmed.contains(&fn_name)
            {
                return Some(trimmed.to_string());
            }
        }

        None
    }

    fn extract_function_name(signature: &str) -> Option<String> {
        let patterns = [
            ("function ", "("),
            ("fn ", "("),
            ("def ", "("),
            ("async function ", "("),
            ("async fn ", "("),
            ("async def ", "("),
        ];

        for (prefix, suffix) in patterns {
            if let Some(start) = signature.find(prefix) {
                let after_prefix = &signature[start + prefix.len()..];
                if let Some(end) = after_prefix.find(suffix) {
                    let name = after_prefix[..end].trim();
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }

        None
    }
}
