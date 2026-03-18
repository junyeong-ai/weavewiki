//! Validity state for content verification

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Binary validity state for generated content.
///
/// - Valid: All file references exist and are accessible
/// - Hallucinated: Contains references to non-existent files/lines
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidityState {
    #[default]
    Valid,
    Hallucinated,
}

impl ValidityState {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    pub fn is_hallucinated(&self) -> bool {
        matches!(self, Self::Hallucinated)
    }
}

impl std::fmt::Display for ValidityState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "valid"),
            Self::Hallucinated => write!(f, "hallucinated"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validity_state() {
        assert!(ValidityState::Valid.is_valid());
        assert!(!ValidityState::Valid.is_hallucinated());
        assert!(!ValidityState::Hallucinated.is_valid());
        assert!(ValidityState::Hallucinated.is_hallucinated());
    }

    #[test]
    fn test_default() {
        assert_eq!(ValidityState::default(), ValidityState::Valid);
    }
}
