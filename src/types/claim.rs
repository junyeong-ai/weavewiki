use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::InformationTier;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub claim_type: ClaimType,
    pub subject_id: String,
    pub statement: String,
    pub evidence: ClaimEvidence,
    pub tier: InformationTier,
    pub confidence: f32,
    pub verification: VerificationStatus,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
}

impl Claim {
    pub fn new(
        id: impl Into<String>,
        claim_type: ClaimType,
        subject_id: impl Into<String>,
        statement: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            claim_type,
            subject_id: subject_id.into(),
            statement: statement.into(),
            evidence: ClaimEvidence::default(),
            tier: InformationTier::Fact,
            confidence: 1.0,
            verification: VerificationStatus::Pending,
            created_at: Utc::now(),
            verified_at: None,
        }
    }

    pub fn verify(&mut self, status: VerificationStatus) {
        self.verification = status;
        self.verified_at = Some(Utc::now());
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    FunctionSignature,
    ClassStructure,
    ModuleExports,
    FileExists,
    DependencyRelation,
    TypeDefinition,
    ApiEndpoint,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerificationStatus {
    Pending,
    Verified,
    Stale,
    Invalid,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaimEvidence {
    pub file: String,
    pub line: Option<u32>,
    pub snippet: Option<String>,
    pub hash: Option<String>,
}

impl ClaimEvidence {
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line: None,
            snippet: None,
            hash: None,
        }
    }

    pub fn with_line(mut self, line: u32) -> Self {
        self.line = Some(line);
        self
    }

    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }

    pub fn with_hash(mut self, hash: impl Into<String>) -> Self {
        self.hash = Some(hash.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationIssue {
    pub id: String,
    pub severity: IssueSeverity,
    pub claim_id: String,
    pub message: String,
    pub suggestion: Option<String>,
    pub auto_fixable: bool,
}

impl VerificationIssue {
    pub fn new(
        claim_id: impl Into<String>,
        severity: IssueSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            severity,
            claim_id: claim_id.into(),
            message: message.into(),
            suggestion: None,
            auto_fixable: false,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn auto_fixable(mut self) -> Self {
        self.auto_fixable = true;
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationReport {
    pub generated_at: DateTime<Utc>,
    pub total_claims: u32,
    pub verified: u32,
    pub stale: u32,
    pub invalid: u32,
    pub issues: Vec<VerificationIssue>,
}

impl VerificationReport {
    pub fn new() -> Self {
        Self {
            generated_at: Utc::now(),
            ..Default::default()
        }
    }

    pub fn add_issue(&mut self, issue: VerificationIssue) {
        self.issues.push(issue);
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }
}
