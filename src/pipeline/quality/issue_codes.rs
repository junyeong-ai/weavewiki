use serde::{Deserialize, Serialize};

use crate::config::RefinementStrategyType;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IssueCode {
    Known(KnownIssueCode),
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KnownIssueCode {
    WeakEvidence,
    MissingReferences,
    InvalidFileReference,

    LowActionability,
    VagueGuidance,
    MissingExamples,

    MissingModule,
    PartialModuleCoverage,
    MissingSections,

    TooShort,
    TooGeneric,
    Shallow,
    Redundant,

    LowVerificationRatio,
}

impl IssueCode {
    pub fn applicable_strategies(&self) -> Vec<RefinementStrategyType> {
        match self {
            Self::Known(code) => code.applicable_strategies(),
            Self::Unknown(_) => vec![
                RefinementStrategyType::Semantic,
                RefinementStrategyType::Evidence,
            ],
        }
    }
}

impl KnownIssueCode {
    pub fn applicable_strategies(&self) -> Vec<RefinementStrategyType> {
        match self {
            // Evidence-first: issues where adding/fixing references is the primary fix
            Self::WeakEvidence
            | Self::MissingReferences
            | Self::InvalidFileReference
            | Self::LowVerificationRatio => {
                vec![RefinementStrategyType::Evidence, RefinementStrategyType::Semantic]
            }

            // Semantic-first: issues requiring content restructuring or enrichment
            Self::LowActionability
            | Self::VagueGuidance
            | Self::MissingExamples
            | Self::TooGeneric
            | Self::MissingModule
            | Self::PartialModuleCoverage
            | Self::MissingSections
            | Self::TooShort
            | Self::Shallow => {
                vec![RefinementStrategyType::Semantic, RefinementStrategyType::Evidence]
            }

            // Semantic-only: reducing content, no evidence needed
            Self::Redundant => {
                vec![RefinementStrategyType::Semantic]
            }
        }
    }
}

impl std::fmt::Display for IssueCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Known(code) => write!(f, "{}", code.as_str()),
            Self::Unknown(s) => write!(f, "{s}"),
        }
    }
}

impl KnownIssueCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WeakEvidence => "WEAK_EVIDENCE",
            Self::MissingReferences => "MISSING_REFERENCES",
            Self::InvalidFileReference => "INVALID_FILE_REFERENCE",
            Self::LowActionability => "LOW_ACTIONABILITY",
            Self::VagueGuidance => "VAGUE_GUIDANCE",
            Self::MissingExamples => "MISSING_EXAMPLES",
            Self::MissingModule => "MISSING_MODULE",
            Self::PartialModuleCoverage => "PARTIAL_MODULE_COVERAGE",
            Self::MissingSections => "MISSING_SECTIONS",
            Self::TooShort => "TOO_SHORT",
            Self::TooGeneric => "TOO_GENERIC",
            Self::Shallow => "SHALLOW",
            Self::Redundant => "REDUNDANT",
            Self::LowVerificationRatio => "LOW_VERIFICATION_RATIO",
        }
    }
}

impl Default for IssueCode {
    fn default() -> Self {
        Self::Unknown("UNSPECIFIED".to_string())
    }
}

impl From<&str> for IssueCode {
    fn from(s: &str) -> Self {
        let upper = s.to_uppercase().replace('-', "_");
        match upper.as_str() {
            "WEAK_EVIDENCE" => Self::Known(KnownIssueCode::WeakEvidence),
            "MISSING_REFERENCES" => Self::Known(KnownIssueCode::MissingReferences),
            "INVALID_FILE_REFERENCE" => Self::Known(KnownIssueCode::InvalidFileReference),
            "LOW_ACTIONABILITY" => Self::Known(KnownIssueCode::LowActionability),
            "VAGUE_GUIDANCE" => Self::Known(KnownIssueCode::VagueGuidance),
            "MISSING_EXAMPLES" => Self::Known(KnownIssueCode::MissingExamples),
            "MISSING_MODULE" => Self::Known(KnownIssueCode::MissingModule),
            "PARTIAL_MODULE_COVERAGE" => Self::Known(KnownIssueCode::PartialModuleCoverage),
            "MISSING_SECTIONS" => Self::Known(KnownIssueCode::MissingSections),
            "TOO_SHORT" => Self::Known(KnownIssueCode::TooShort),
            "TOO_GENERIC" => Self::Known(KnownIssueCode::TooGeneric),
            "SHALLOW" => Self::Known(KnownIssueCode::Shallow),
            "REDUNDANT" => Self::Known(KnownIssueCode::Redundant),
            "LOW_VERIFICATION_RATIO" => Self::Known(KnownIssueCode::LowVerificationRatio),
            _ => Self::Unknown(s.to_string()),
        }
    }
}

impl From<String> for IssueCode {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_issue_code_parsing() {
        assert_eq!(
            IssueCode::from("WEAK_EVIDENCE"),
            IssueCode::Known(KnownIssueCode::WeakEvidence)
        );
        assert_eq!(
            IssueCode::from("weak_evidence"),
            IssueCode::Known(KnownIssueCode::WeakEvidence)
        );
        assert_eq!(
            IssueCode::from("weak-evidence"),
            IssueCode::Known(KnownIssueCode::WeakEvidence)
        );
    }

    #[test]
    fn test_unknown_issue_code() {
        let code = IssueCode::from("CUSTOM_ISSUE");
        assert!(matches!(code, IssueCode::Unknown(_)));
    }

    #[test]
    fn test_applicable_strategies() {
        let code = IssueCode::Known(KnownIssueCode::WeakEvidence);
        let strategies = code.applicable_strategies();
        assert!(strategies.contains(&RefinementStrategyType::Evidence));
    }

    #[test]
    fn test_too_generic_strategies() {
        let code = IssueCode::Known(KnownIssueCode::TooGeneric);
        let strategies = code.applicable_strategies();
        assert_eq!(strategies[0], RefinementStrategyType::Semantic);
        assert!(strategies.contains(&RefinementStrategyType::Evidence));
    }

    #[test]
    fn test_serde_roundtrip() {
        let code = IssueCode::Known(KnownIssueCode::MissingReferences);
        let json = serde_json::to_string(&code).unwrap();
        let parsed: IssueCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, parsed);
    }

    #[test]
    fn test_display_roundtrip() {
        let codes = [
            KnownIssueCode::WeakEvidence,
            KnownIssueCode::MissingReferences,
            KnownIssueCode::InvalidFileReference,
            KnownIssueCode::LowVerificationRatio,
            KnownIssueCode::TooGeneric,
            KnownIssueCode::Shallow,
            KnownIssueCode::Redundant,
        ];
        for code in codes {
            let issue = IssueCode::Known(code);
            let displayed = issue.to_string();
            let parsed = IssueCode::from(displayed.as_str());
            assert_eq!(issue, parsed, "Roundtrip failed for {code:?}");
        }
    }
}
