//! Pattern Library & Project Constitution
//!
//! Comprehensive pattern detection system for code analysis.
//!
//! ## Design Principles
//! - Multi-strategy detection: keywords, structural analysis, naming conventions
//! - 25+ pattern types across architectural, creational, behavioral, and Rust-specific categories
//! - Confidence scoring based on evidence strength
//! - Extensible pattern registry
//!
//! ## Pattern Categories
//! - Creational: Singleton, Factory, Builder, Prototype
//! - Structural: Adapter, Decorator, Facade, Proxy, Composite
//! - Behavioral: Observer, Strategy, Command, State, Chain of Responsibility
//! - Architectural: Repository, CQRS, Event Sourcing, Clean Architecture
//! - Rust-specific: Newtype, TypeState, Builder with Consume

use std::collections::HashMap;
use tracing::info;

use super::bottom_up::FileInsight;

/// Detected code pattern with confidence scoring
#[derive(Debug, Clone)]
pub struct CodePattern {
    pub name: String,
    pub description: String,
    pub usage_count: usize,
    pub example_files: Vec<String>,
    pub category: PatternCategory,
    pub confidence: f32,
}

/// Pattern categories aligned with standard design pattern classifications
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PatternCategory {
    Creational,
    Structural,
    Behavioral,
    Architectural,
    DataFlow,
    ErrorHandling,
    Testing,
    Configuration,
    RustSpecific,
    Other,
}

/// Project constitution (coding standards)
#[derive(Debug, Clone, Default)]
pub struct ProjectConstitution {
    pub naming_conventions: Vec<Convention>,
    pub file_organization: Vec<Convention>,
    pub code_style: Vec<Convention>,
    pub documentation: Vec<Convention>,
}

/// A single convention/rule
#[derive(Debug, Clone)]
pub struct Convention {
    pub rule: String,
    pub evidence: Vec<String>,
    pub confidence: f32,
}

/// Pattern definition for the registry
struct PatternDef {
    name: &'static str,
    description: &'static str,
    category: PatternCategory,
    /// Keywords to match in insights (case-insensitive)
    keywords: &'static [&'static str],
    /// File path patterns to match
    path_patterns: &'static [&'static str],
    /// Minimum occurrences to report
    min_occurrences: usize,
}

/// Comprehensive pattern registry
const PATTERN_REGISTRY: &[PatternDef] = &[
    // ===== Creational Patterns =====
    PatternDef {
        name: "Singleton",
        description: "Ensures a class has only one instance with global access point",
        category: PatternCategory::Creational,
        keywords: &[
            "singleton",
            "single instance",
            "global instance",
            "once_cell",
            "lazy_static",
        ],
        path_patterns: &[],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Factory",
        description: "Creates objects without specifying exact class to instantiate",
        category: PatternCategory::Creational,
        keywords: &["factory", "create_", "make_", "build_", "object creation"],
        path_patterns: &["factory", "_factory"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Builder",
        description: "Constructs complex objects step by step with fluent interface",
        category: PatternCategory::Creational,
        keywords: &["builder", "fluent", "with_", "set_", "method chaining"],
        path_patterns: &["builder", "_builder"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Prototype",
        description: "Creates new objects by cloning existing instances",
        category: PatternCategory::Creational,
        keywords: &["prototype", "clone", "deep copy", "copy from"],
        path_patterns: &[],
        min_occurrences: 2,
    },
    // ===== Structural Patterns =====
    PatternDef {
        name: "Adapter",
        description: "Converts interface of a class into another expected interface",
        category: PatternCategory::Structural,
        keywords: &[
            "adapter",
            "wrapper",
            "compatibility layer",
            "convert interface",
        ],
        path_patterns: &["adapter", "_adapter"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Decorator",
        description: "Adds behavior to objects dynamically without affecting others",
        category: PatternCategory::Structural,
        keywords: &[
            "decorator",
            "wrapper",
            "add behavior",
            "extend functionality",
        ],
        path_patterns: &["decorator", "_decorator"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Facade",
        description: "Provides simplified interface to complex subsystem",
        category: PatternCategory::Structural,
        keywords: &["facade", "simplified interface", "unified api", "subsystem"],
        path_patterns: &["facade", "_facade"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Proxy",
        description: "Controls access to another object through surrogate",
        category: PatternCategory::Structural,
        keywords: &["proxy", "lazy loading", "access control", "surrogate"],
        path_patterns: &["proxy", "_proxy"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Composite",
        description: "Composes objects into tree structures for part-whole hierarchies",
        category: PatternCategory::Structural,
        keywords: &["composite", "tree structure", "hierarchy", "part-whole"],
        path_patterns: &["composite"],
        min_occurrences: 1,
    },
    // ===== Behavioral Patterns =====
    PatternDef {
        name: "Observer",
        description: "Defines subscription mechanism for state change notifications",
        category: PatternCategory::Behavioral,
        keywords: &[
            "observer",
            "subscribe",
            "notify",
            "listener",
            "event handler",
            "callback",
            "on_",
        ],
        path_patterns: &["observer", "listener", "subscriber"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Strategy",
        description: "Defines family of interchangeable algorithms",
        category: PatternCategory::Behavioral,
        keywords: &[
            "strategy",
            "algorithm",
            "interchangeable",
            "polymorphism",
            "policy",
        ],
        path_patterns: &["strategy", "_strategy"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Command",
        description: "Encapsulates requests as objects for parameterization",
        category: PatternCategory::Behavioral,
        keywords: &["command", "execute", "undo", "redo", "action queue"],
        path_patterns: &["command", "_command", "_cmd"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "State Machine",
        description: "Allows object to alter behavior when internal state changes",
        category: PatternCategory::Behavioral,
        keywords: &[
            "state machine",
            "state pattern",
            "transition",
            "fsm",
            "state enum",
            "current_state",
        ],
        path_patterns: &["state", "_state", "fsm"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Chain of Responsibility",
        description: "Passes requests along chain of handlers",
        category: PatternCategory::Behavioral,
        keywords: &[
            "chain",
            "handler chain",
            "middleware",
            "next handler",
            "pipeline",
        ],
        path_patterns: &["chain", "handler"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Iterator",
        description: "Provides sequential access to elements without exposing structure",
        category: PatternCategory::Behavioral,
        keywords: &["iterator", "iter", "next", "has_next", "cursor"],
        path_patterns: &["iterator", "_iter"],
        min_occurrences: 2,
    },
    PatternDef {
        name: "Visitor",
        description: "Separates algorithm from object structure it operates on",
        category: PatternCategory::Behavioral,
        keywords: &["visitor", "accept", "visit_", "traverse"],
        path_patterns: &["visitor"],
        min_occurrences: 1,
    },
    // ===== Architectural Patterns =====
    PatternDef {
        name: "Repository",
        description: "Abstracts data access with collection-like interface",
        category: PatternCategory::Architectural,
        keywords: &[
            "repository",
            "data access",
            "persistence",
            "crud",
            "find_by",
            "save",
            "get_all",
        ],
        path_patterns: &["repository", "repo", "_repo"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Service Layer",
        description: "Defines application's boundary with layer of services",
        category: PatternCategory::Architectural,
        keywords: &["service", "business logic", "use case", "application layer"],
        path_patterns: &["service", "_service"],
        min_occurrences: 2,
    },
    PatternDef {
        name: "Event-Driven",
        description: "Uses events for communication between decoupled components",
        category: PatternCategory::Architectural,
        keywords: &[
            "event",
            "emit",
            "publish",
            "dispatch",
            "event bus",
            "message queue",
        ],
        path_patterns: &["event", "_event"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "CQRS",
        description: "Separates read and write operations for different models",
        category: PatternCategory::Architectural,
        keywords: &[
            "cqrs",
            "command query",
            "read model",
            "write model",
            "query handler",
        ],
        path_patterns: &["command", "query"],
        min_occurrences: 2,
    },
    PatternDef {
        name: "Dependency Injection",
        description: "Provides dependencies to objects rather than having them construct",
        category: PatternCategory::Architectural,
        keywords: &[
            "dependency injection",
            "inject",
            "ioc",
            "container",
            "provide",
        ],
        path_patterns: &["provider", "inject"],
        min_occurrences: 1,
    },
    // ===== Data Flow Patterns =====
    PatternDef {
        name: "Pipeline/Middleware",
        description: "Processes data through series of transformations",
        category: PatternCategory::DataFlow,
        keywords: &[
            "pipeline",
            "middleware",
            "transform",
            "stage",
            "pass through",
        ],
        path_patterns: &["pipeline", "middleware"],
        min_occurrences: 1,
    },
    PatternDef {
        name: "Pub/Sub",
        description: "Decouples publishers from subscribers via message broker",
        category: PatternCategory::DataFlow,
        keywords: &[
            "pub/sub",
            "publish",
            "subscribe",
            "topic",
            "channel",
            "broadcast",
        ],
        path_patterns: &["pubsub", "subscriber", "publisher"],
        min_occurrences: 1,
    },
    // ===== Error Handling Patterns =====
    PatternDef {
        name: "Result/Error Handling",
        description: "Explicit error handling using Result types",
        category: PatternCategory::ErrorHandling,
        keywords: &[
            "result",
            "error handling",
            "try",
            "catch",
            "recover",
            "fallback",
        ],
        path_patterns: &["error", "_error"],
        min_occurrences: 2,
    },
    // ===== Rust-Specific Patterns =====
    PatternDef {
        name: "Newtype",
        description: "Wraps primitive type for type safety without runtime cost",
        category: PatternCategory::RustSpecific,
        keywords: &["newtype", "wrapper type", "type alias", "strong typing"],
        path_patterns: &[],
        min_occurrences: 2,
    },
    PatternDef {
        name: "TypeState",
        description: "Uses type system to encode state transitions at compile time",
        category: PatternCategory::RustSpecific,
        keywords: &["typestate", "phantom", "marker type", "compile-time state"],
        path_patterns: &[],
        min_occurrences: 1,
    },
    PatternDef {
        name: "RAII",
        description: "Resource management tied to object lifetime via Drop",
        category: PatternCategory::RustSpecific,
        keywords: &["raii", "drop", "cleanup", "resource management", "guard"],
        path_patterns: &["guard", "_guard"],
        min_occurrences: 1,
    },
];

/// Pattern and constitution extractor
pub struct PatternExtractor;

impl PatternExtractor {
    /// Extract patterns from file insights using multi-strategy detection
    pub fn extract_patterns(insights: &[FileInsight]) -> Vec<CodePattern> {
        let mut pattern_evidence: HashMap<&str, Vec<(String, f32)>> = HashMap::new();

        for insight in insights {
            let path_lower = insight.file_path.to_lowercase();

            for pattern in PATTERN_REGISTRY {
                let mut confidence = 0.0f32;
                let mut matched = false;

                // Strategy 1: Keyword matching in content (high confidence)
                let content_lower = insight.content.to_lowercase();
                for keyword in pattern.keywords {
                    if content_lower.contains(keyword) {
                        confidence += 0.4;
                        matched = true;
                        break;
                    }
                }

                // Strategy 2: File path pattern matching (medium confidence)
                for path_pattern in pattern.path_patterns {
                    if path_lower.contains(path_pattern) {
                        confidence += 0.3;
                        matched = true;
                        break;
                    }
                }

                // Strategy 3: Purpose matching (high confidence)
                let purpose_lower = insight.purpose.to_lowercase();
                for keyword in pattern.keywords {
                    if purpose_lower.contains(keyword) {
                        confidence += 0.3;
                        matched = true;
                        break;
                    }
                }

                if matched {
                    let entry = pattern_evidence.entry(pattern.name).or_default();
                    // Don't add duplicate files
                    if !entry.iter().any(|(f, _)| f == &insight.file_path) {
                        entry.push((insight.file_path.clone(), confidence.min(1.0)));
                    }
                }
            }
        }

        // Convert to patterns with confidence scoring
        let mut patterns = Vec::new();
        for pattern_def in PATTERN_REGISTRY {
            if let Some(evidence) = pattern_evidence.get(pattern_def.name)
                && evidence.len() >= pattern_def.min_occurrences
            {
                let avg_confidence =
                    evidence.iter().map(|(_, c)| c).sum::<f32>() / evidence.len() as f32;
                patterns.push(CodePattern {
                    name: pattern_def.name.to_string(),
                    description: pattern_def.description.to_string(),
                    usage_count: evidence.len(),
                    example_files: evidence.iter().take(3).map(|(f, _)| f.clone()).collect(),
                    category: pattern_def.category.clone(),
                    confidence: avg_confidence,
                });
            }
        }

        patterns.sort_by(|a, b| {
            // Sort by usage count, then by confidence
            b.usage_count.cmp(&a.usage_count).then(
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });

        info!("Extracted {} code patterns", patterns.len());
        patterns
    }

    /// Infer project constitution from file insights
    pub fn infer_constitution(insights: &[FileInsight]) -> ProjectConstitution {
        let mut constitution = ProjectConstitution::default();

        // Analyze file paths for naming conventions
        let mut snake_case = 0;
        let mut camel_case = 0;
        let mut kebab_case = 0;

        for insight in insights {
            let filename = insight
                .file_path
                .rsplit('/')
                .next()
                .unwrap_or(&insight.file_path);

            if filename.contains('_') && !filename.contains('-') {
                snake_case += 1;
            } else if filename.contains('-') {
                kebab_case += 1;
            } else if filename.chars().any(|c| c.is_uppercase()) {
                camel_case += 1;
            }
        }

        // Add naming convention based on most common
        let total = snake_case + camel_case + kebab_case;
        if total > 0 {
            if snake_case >= camel_case && snake_case >= kebab_case {
                constitution.naming_conventions.push(Convention {
                    rule: "Use snake_case for file names".to_string(),
                    evidence: vec![format!("{} files use snake_case", snake_case)],
                    confidence: snake_case as f32 / total as f32,
                });
            } else if camel_case >= snake_case && camel_case >= kebab_case {
                constitution.naming_conventions.push(Convention {
                    rule: "Use camelCase/PascalCase for file names".to_string(),
                    evidence: vec![format!("{} files use camelCase", camel_case)],
                    confidence: camel_case as f32 / total as f32,
                });
            } else {
                constitution.naming_conventions.push(Convention {
                    rule: "Use kebab-case for file names".to_string(),
                    evidence: vec![format!("{} files use kebab-case", kebab_case)],
                    confidence: kebab_case as f32 / total as f32,
                });
            }
        }

        // Analyze file organization
        let mut module_patterns: HashMap<String, usize> = HashMap::new();
        for insight in insights {
            if let Some(parent) = insight.file_path.rsplit('/').nth(1) {
                *module_patterns.entry(parent.to_string()).or_insert(0) += 1;
            }
        }

        // Common directory conventions
        let conventions = [
            ("src", "Source code in src/ directory"),
            ("lib", "Library code in lib/ directory"),
            ("tests", "Tests in tests/ directory"),
            ("docs", "Documentation in docs/ directory"),
            ("bin", "Executables in bin/ directory"),
            ("pkg", "Packages in pkg/ directory"),
        ];

        for (dir, desc) in conventions {
            if let Some(&count) = module_patterns.get(dir)
                && count >= 2
            {
                constitution.file_organization.push(Convention {
                    rule: desc.to_string(),
                    evidence: vec![format!("{} files in {}/ directory", count, dir)],
                    confidence: 0.9,
                });
            }
        }

        // Code style from content
        let mut style_hints: HashMap<String, usize> = HashMap::new();
        for insight in insights {
            let lower = insight.content.to_lowercase();
            if lower.contains("error") {
                *style_hints
                    .entry("Error handling is critical".to_string())
                    .or_insert(0) += 1;
            }
            if lower.contains("async") || lower.contains("concurrent") {
                *style_hints
                    .entry("Async patterns are used extensively".to_string())
                    .or_insert(0) += 1;
            }
            if lower.contains("immutable") {
                *style_hints
                    .entry("Prefer immutable data structures".to_string())
                    .or_insert(0) += 1;
            }
        }

        for (hint, count) in style_hints {
            if count >= 2 {
                constitution.code_style.push(Convention {
                    rule: hint,
                    evidence: vec![format!("Mentioned in {} files", count)],
                    confidence: (count as f32 / insights.len() as f32).min(1.0),
                });
            }
        }

        constitution
    }

    /// Generate constitution as markdown
    pub fn generate_constitution_md(constitution: &ProjectConstitution) -> String {
        let mut output = String::new();

        output.push_str("# Project Constitution\n\n");
        output.push_str("Coding standards and conventions inferred from the codebase.\n\n");

        if !constitution.naming_conventions.is_empty() {
            output.push_str("## Naming Conventions\n\n");
            for conv in &constitution.naming_conventions {
                output.push_str(&format!(
                    "- **{}** (confidence: {:.0}%)\n",
                    conv.rule,
                    conv.confidence * 100.0
                ));
            }
            output.push('\n');
        }

        if !constitution.file_organization.is_empty() {
            output.push_str("## File Organization\n\n");
            for conv in &constitution.file_organization {
                output.push_str(&format!("- {}\n", conv.rule));
            }
            output.push('\n');
        }

        if !constitution.code_style.is_empty() {
            output.push_str("## Code Style\n\n");
            for conv in &constitution.code_style {
                output.push_str(&format!("- {}\n", conv.rule));
            }
            output.push('\n');
        }

        output
    }

    /// Generate pattern library as markdown with confidence scoring
    pub fn generate_patterns_md(patterns: &[CodePattern]) -> String {
        let mut output = String::new();

        output.push_str("# Pattern Library\n\n");
        output
            .push_str("Code patterns detected in the codebase using multi-strategy analysis.\n\n");

        if patterns.is_empty() {
            output.push_str("No significant patterns detected.\n");
            return output;
        }

        // Group patterns by category for better organization
        let mut by_category: HashMap<PatternCategory, Vec<&CodePattern>> = HashMap::new();
        for pattern in patterns {
            by_category
                .entry(pattern.category.clone())
                .or_default()
                .push(pattern);
        }

        // Define category order
        let category_order = [
            PatternCategory::Creational,
            PatternCategory::Structural,
            PatternCategory::Behavioral,
            PatternCategory::Architectural,
            PatternCategory::DataFlow,
            PatternCategory::ErrorHandling,
            PatternCategory::RustSpecific,
            PatternCategory::Testing,
            PatternCategory::Configuration,
            PatternCategory::Other,
        ];

        for category in category_order.iter() {
            if let Some(category_patterns) = by_category.get(category) {
                output.push_str(&format!("## {:?} Patterns\n\n", category));

                for pattern in category_patterns {
                    output.push_str(&format!("### {}\n\n", pattern.name));
                    output.push_str(&format!("{}\n\n", pattern.description));
                    output.push_str(&format!(
                        "- **Usage**: {} occurrence{}\n",
                        pattern.usage_count,
                        if pattern.usage_count > 1 { "s" } else { "" }
                    ));
                    output.push_str(&format!(
                        "- **Confidence**: {:.0}%\n",
                        pattern.confidence * 100.0
                    ));
                    if !pattern.example_files.is_empty() {
                        output.push_str("- **Examples**:\n");
                        for file in &pattern.example_files {
                            output.push_str(&format!("  - `{}`\n", file));
                        }
                    }
                    output.push('\n');
                }
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::super::bottom_up::Importance;
    use super::*;

    fn make_insight(path: &str, insights: Vec<&str>) -> FileInsight {
        FileInsight {
            file_path: path.to_string(),
            language: Some("rust".to_string()),
            line_count: 100,
            purpose: String::new(),
            importance: Importance::Medium,
            tier: super::super::bottom_up::ProcessingTier::Standard,
            content: insights.join("\n"),
            diagram: None,
            related_files: vec![],
            token_count: 0,
            research_iterations_json: None,
            research_aspects_json: None,
        }
    }

    #[test]
    fn test_extract_patterns() {
        let insights = vec![
            make_insight("src/factory.rs", vec!["Uses factory pattern"]),
            make_insight("src/builder.rs", vec!["Uses factory pattern"]),
            make_insight("src/other.rs", vec!["Uses singleton"]),
        ];

        let patterns = PatternExtractor::extract_patterns(&insights);

        // Factory should be detected (2 files)
        assert!(patterns.iter().any(|p| p.name == "Factory"));
    }

    #[test]
    fn test_infer_constitution() {
        let insights = vec![
            make_insight("src/user_service.rs", vec![]),
            make_insight("src/order_service.rs", vec![]),
            make_insight("src/payment_service.rs", vec![]),
        ];

        let constitution = PatternExtractor::infer_constitution(&insights);

        // Should detect snake_case convention
        assert!(!constitution.naming_conventions.is_empty());
    }

    #[test]
    fn test_generate_md() {
        let patterns = vec![CodePattern {
            name: "Factory".to_string(),
            description: "Object creation".to_string(),
            usage_count: 3,
            example_files: vec!["a.rs".to_string()],
            category: PatternCategory::Creational,
            confidence: 0.8,
        }];

        let md = PatternExtractor::generate_patterns_md(&patterns);
        assert!(md.contains("# Pattern Library"));
        assert!(md.contains("### Factory"));
        assert!(md.contains("**Confidence**: 80%"));
    }
}
