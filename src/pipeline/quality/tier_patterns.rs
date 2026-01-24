//! Tier Pattern Detection
//!
//! Centralized patterns for content tier classification.
//! Used by both LlmJudge and DeepReviewEngine.

use std::sync::LazyLock;

use regex::Regex;

/// Tier 1 patterns - Generic knowledge (REJECT)
/// Content matching these patterns provides no value beyond basic knowledge.
pub static TIER1_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // Build commands (Claude already knows these)
        r"(?i)cargo\s+(build|run|test|check)",
        r"(?i)npm\s+(install|run|test|build)",
        r"(?i)yarn\s+(install|add|build)",
        r"(?i)pnpm\s+(install|add)",
        r"(?i)go\s+(build|run|test)",
        r"(?i)make\s+(build|test|clean)",
        // Generic coding advice
        r"(?i)\buse\s+(async|await)\b",
        r"(?i)use\s+the\s+\?\s+operator",
        r"(?i)\bhandle\s+errors?\b",
        r"(?i)handle\s+errors\s+properly",
        r"(?i)\bfollow\s+best\s+practices?\b",
        r"(?i)\buse\s+proper\s+naming\b",
        r"(?i)\bwrite\s+clean\s+code\b",
        r"(?i)\badd\s+comments?\b",
        r"(?i)add\s+comments\s+to\s+explain",
        r"(?i)\buse\s+meaningful\s+names?\b",
        r"(?i)\bprefer\s+composition\b",
        r"(?i)\bavoid\s+global\s+state\b",
        r"(?i)\buse\s+dependency\s+injection\b",
        r"(?i)prefer\s+const\s+over\s+let",
        r"(?i)use\s+strict\s+mode",
        // Tool suggestions
        r"(?i)use\s+git\s+for\s+version\s+control",
        r"(?i)run\s+tests\s+before\s+commit",
        r"(?i)use\s+a\s+linter",
        r"(?i)format\s+your\s+code",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Tier 3 patterns - Hidden constraints (ESSENTIAL)
/// Content matching these patterns indicates project-specific critical knowledge.
pub static TIER3_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"@[\w/]+\.(rs|ts|py|go|java|kt|rb|swift|c|cpp|h):\d+",
        r"(?i)\bMUST\b",
        r"(?i)\bCRITICAL\b",
        r"(?i)\bNEVER\b",
        r"(?i)\bALWAYS\b",
        r"Arc::<.*>::clone|Arc::clone",
        r"OnceCell|LazyLock",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Count Tier 1 pattern matches in content
#[inline]
pub fn count_tier1_matches(content: &str) -> usize {
    TIER1_PATTERNS
        .iter()
        .filter(|p| p.is_match(content))
        .count()
}

/// Count Tier 3 pattern matches in content
#[inline]
pub fn count_tier3_matches(content: &str) -> usize {
    TIER3_PATTERNS
        .iter()
        .filter(|p| p.is_match(content))
        .count()
}

/// Check if content appears to be Tier 1 (generic)
pub fn is_tier1_content(content: &str) -> bool {
    let tier1_matches = count_tier1_matches(content);
    let content_len = content.len();

    // Tier 1 if many generic patterns and short content
    (tier1_matches >= 2 && content_len < 500) || tier1_matches >= 3
}

/// Check if content appears to be Tier 3 (constraint)
pub fn is_tier3_content(content: &str) -> bool {
    count_tier3_matches(content) >= 2
}

/// Find Tier 1 matches with line numbers
pub fn find_tier1_matches(content: &str) -> Vec<(usize, String)> {
    let mut matches = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        for pattern in TIER1_PATTERNS.iter() {
            if pattern.is_match(line) {
                matches.push((line_num + 1, line.trim().to_string()));
                break;
            }
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier1_patterns() {
        let tier1_cases = [
            "cargo build",
            "npm install",
            "use async/await",
            "follow best practices",
            "write clean code",
            "use meaningful names",
        ];

        for case in tier1_cases {
            assert!(
                count_tier1_matches(case) > 0,
                "Should match Tier 1: {}",
                case
            );
        }
    }

    #[test]
    fn test_tier3_patterns() {
        let tier3_cases = [
            "@src/main.rs:42",
            "You MUST use Arc::clone",
            "CRITICAL: NEVER skip this step",
        ];

        for case in tier3_cases {
            assert!(
                count_tier3_matches(case) > 0,
                "Should match Tier 3: {}",
                case
            );
        }
    }

    #[test]
    fn test_project_specific_not_tier1() {
        let project_specific = [
            "Use @src/main.rs:42 for entry point",
            "Provider MUST be Arc-shared for rate limiting",
        ];

        for case in project_specific {
            assert!(
                !is_tier1_content(case),
                "Project-specific should not be Tier 1: {}",
                case
            );
        }
    }

    #[test]
    fn test_is_tier1_content() {
        let short_generic = "Use async/await and handle errors properly";
        assert!(is_tier1_content(short_generic));

        let long_specific = format!(
            "This project uses @src/config.rs:100 for configuration. MUST respect Arc sharing. {}",
            "x".repeat(500)
        );
        assert!(!is_tier1_content(&long_specific));
    }
}
