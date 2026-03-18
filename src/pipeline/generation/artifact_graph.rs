//! Artifact Cross-Linking Graph
//!
//! Builds bidirectional relationships between generated artifacts (Skills, Agents, Rules)
//! and injects cross-references back into each artifact for discoverability.
//!
//! Relationship types:
//! - Agent <-> Skill: Agent's `skills:` frontmatter lists relevant skills
//! - Skill <-> Rule: Skill body gets "See also" references to governing rules
//! - Agent <-> Rule: Agent prompt gets references to relevant rules
//! - Rule <-> Module: Rules with `paths` globs connect to module agents

use std::collections::{HashMap, HashSet};

use crate::types::agent::Agent;
use crate::types::module_map::DetectedModule;
use crate::types::rule::Rule;
use crate::types::skill::Skill;

/// Common programming terms that cause false positive matches when used as segments.
const STOPWORDS: &[&str] = &[
    "code", "data", "test", "error", "type", "file", "name", "user", "list", "item",
    "view", "form", "page", "base", "core", "main", "util", "spec", "impl", "func",
    "info", "meta", "init",
];

fn is_stopword(s: &str) -> bool {
    STOPWORDS.contains(&s)
}

/// A relationship between two artifacts
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactRelation {
    pub kind: RelationKind,
    pub source: ArtifactId,
    pub target: ArtifactId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelationKind {
    /// Agent uses a skill (agent.skills includes skill name)
    AgentUsesSkill,
    /// Rule governs files that a module agent covers
    RuleGovernsModule,
    /// Skill references a rule's domain
    SkillReferencesRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArtifactId {
    Skill(String),
    Agent(String),
    Rule(String),
}

impl std::fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill(name) => write!(f, "skill:{name}"),
            Self::Agent(name) => write!(f, "agent:{name}"),
            Self::Rule(name) => write!(f, "rule:{name}"),
        }
    }
}

/// Graph of relationships between generated artifacts
pub struct ArtifactGraph {
    relations: Vec<ArtifactRelation>,
    /// Forward index: artifact -> artifacts it relates to
    forward: HashMap<String, Vec<(RelationKind, String)>>,
}

impl ArtifactGraph {
    /// Build the cross-linking graph from generated artifacts.
    ///
    /// Discovers relationships based on:
    /// - Name segment matching (e.g., "auth-specialist" agent <-> "auth-test" skill)
    /// - Rule path globs matching module paths
    /// - Skill body content mentioning rule domains
    pub fn build(
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        modules: &[DetectedModule],
    ) -> Self {
        let mut relations = Vec::new();
        let mut forward: HashMap<String, Vec<(RelationKind, String)>> = HashMap::new();

        // Agent <-> Skill: match by module name segments
        for agent in agents {
            for skill in skills {
                if segments_overlap(&agent.name, &skill.name) {
                    let rel = ArtifactRelation {
                        kind: RelationKind::AgentUsesSkill,
                        source: ArtifactId::Agent(agent.name.clone()),
                        target: ArtifactId::Skill(skill.name.clone()),
                    };
                    forward
                        .entry(agent.name.clone())
                        .or_default()
                        .push((RelationKind::AgentUsesSkill, skill.name.clone()));
                    relations.push(rel);
                }
            }
        }

        // Rule <-> Module Agent: match rule paths to module paths
        let module_agents: HashMap<&str, Vec<&str>> = build_module_agent_map(agents, modules);
        for rule in rules {
            if let Some(ref paths) = rule.paths {
                for (module_id, agent_names) in &module_agents {
                    if path_matches_module(paths, module_id, modules) {
                        for agent_name in agent_names {
                            let rel = ArtifactRelation {
                                kind: RelationKind::RuleGovernsModule,
                                source: ArtifactId::Rule(rule.name.clone()),
                                target: ArtifactId::Agent(agent_name.to_string()),
                            };
                            forward
                                .entry(rule.name.clone())
                                .or_default()
                                .push((RelationKind::RuleGovernsModule, agent_name.to_string()));
                            relations.push(rel);
                        }
                    }
                }
            }
        }

        // Skill <-> Rule: match by content references
        for skill in skills {
            let body_lower = skill.body.to_lowercase();
            for rule in rules {
                let rule_lower = rule.name.to_lowercase();
                if skill_references_rule(&body_lower, &rule_lower) {
                    let rel = ArtifactRelation {
                        kind: RelationKind::SkillReferencesRule,
                        source: ArtifactId::Skill(skill.name.clone()),
                        target: ArtifactId::Rule(rule.name.clone()),
                    };
                    forward
                        .entry(skill.name.clone())
                        .or_default()
                        .push((RelationKind::SkillReferencesRule, rule.name.clone()));
                    relations.push(rel);
                }
            }
        }

        Self { relations, forward }
    }

    /// Apply cross-links back into the artifacts.
    ///
    /// - Agents: populates `skills` field with related skill names
    /// - Skills: appends "See also" section with related rules
    /// - Returns the modified artifacts
    pub fn apply(
        &self,
        mut skills: Vec<Skill>,
        mut agents: Vec<Agent>,
        rules: &[Rule],
    ) -> (Vec<Skill>, Vec<Agent>) {
        // Apply agent -> skill links
        for agent in &mut agents {
            let skill_names: Vec<String> = self
                .forward
                .get(&agent.name)
                .map(|rels| {
                    rels.iter()
                        .filter(|(kind, _)| matches!(kind, RelationKind::AgentUsesSkill))
                        .map(|(_, name)| name.clone())
                        .collect()
                })
                .unwrap_or_default();

            if !skill_names.is_empty() {
                // Merge with existing skills, avoiding duplicates
                let existing: HashSet<String> = agent
                    .skills
                    .as_ref()
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();

                let mut merged: Vec<String> = agent.skills.take().unwrap_or_default();
                for name in skill_names {
                    if !existing.contains(&name) {
                        merged.push(name);
                    }
                }
                if !merged.is_empty() {
                    agent.skills = Some(merged);
                }
            }
        }

        // Apply skill -> rule "See also" links
        for skill in &mut skills {
            let rule_names: Vec<String> = self
                .forward
                .get(&skill.name)
                .map(|rels| {
                    rels.iter()
                        .filter(|(kind, _)| matches!(kind, RelationKind::SkillReferencesRule))
                        .map(|(_, name)| name.clone())
                        .collect()
                })
                .unwrap_or_default();

            if !rule_names.is_empty() {
                // Only add "See also" if the skill doesn't already have one
                if !skill.body.contains("See also:") {
                    let refs: Vec<String> = rule_names
                        .iter()
                        .filter_map(|name| {
                            rules
                                .iter()
                                .find(|r| &r.name == name)
                                .map(|_| format!("`.claude/rules/{}.md`", name))
                        })
                        .collect();

                    if !refs.is_empty() {
                        skill.body.push_str(&format!(
                            "\n\n---\n\nSee also: {}",
                            refs.join(", ")
                        ));
                    }
                }
            }
        }

        (skills, agents)
    }

    /// Number of discovered relationships
    pub fn relation_count(&self) -> usize {
        self.relations.len()
    }
}

/// Check if two kebab-case names share a meaningful segment.
///
/// "auth-specialist" and "auth-test" share "auth".
/// Ignores short segments (< 4 chars) and stopwords to avoid false matches.
fn segments_overlap(a: &str, b: &str) -> bool {
    let a_segments: HashSet<&str> = a
        .split('-')
        .filter(|s| s.len() >= 4 && !is_stopword(s))
        .collect();
    let b_segments: HashSet<&str> = b
        .split('-')
        .filter(|s| s.len() >= 4 && !is_stopword(s))
        .collect();
    !a_segments.is_disjoint(&b_segments)
}

/// Check if a skill body references a rule by name or segments.
///
/// Requires either:
/// - Full rule name appears in body, OR
/// - At least 2 non-stopword segments match simultaneously
fn skill_references_rule(body_lower: &str, rule_lower: &str) -> bool {
    // Full name match
    if body_lower.contains(rule_lower) {
        return true;
    }

    // Segment matching: require 2+ non-stopword segments
    let segments: Vec<&str> = rule_lower
        .split('-')
        .filter(|s| s.len() >= 4 && !is_stopword(s))
        .collect();

    let matching_count = segments
        .iter()
        .filter(|seg| body_lower.contains(*seg))
        .count();

    matching_count >= 2 && !segments.is_empty()
}

/// Build a map from module_id -> agent names that cover that module.
fn build_module_agent_map<'a>(
    agents: &'a [Agent],
    modules: &'a [DetectedModule],
) -> HashMap<&'a str, Vec<&'a str>> {
    let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
    for module in modules {
        for agent in agents {
            if segments_overlap(&agent.name, &module.module_id) {
                map.entry(module.module_id.as_str())
                    .or_default()
                    .push(agent.name.as_str());
            }
        }
    }
    map
}

/// Check if any rule path glob could match files within a module's paths.
fn path_matches_module(rule_paths: &[String], module_id: &str, modules: &[DetectedModule]) -> bool {
    let module = modules.iter().find(|m| m.module_id == module_id);
    let module = match module {
        Some(m) => m,
        None => return false,
    };

    for rule_path in rule_paths {
        let rule_lower = rule_path.to_lowercase();
        for module_path in &module.paths {
            let mod_lower = module_path.to_lowercase();
            // Rule path starts with module path (rule is within module)
            if rule_lower.starts_with(&mod_lower) {
                return true;
            }
            // Module path starts with rule prefix, with boundary check
            let prefix = rule_lower.trim_end_matches("**").trim_end_matches('/');
            if mod_lower.starts_with(prefix)
                && (mod_lower.len() == prefix.len()
                    || mod_lower.as_bytes().get(prefix.len()) == Some(&b'/'))
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segments_overlap() {
        assert!(segments_overlap("auth-specialist", "auth-testing"));
        assert!(segments_overlap("database-admin", "database-migration"));
        assert!(!segments_overlap("auth-specialist", "payment-handler"));
        // Short segments ignored (< 4 chars)
        assert!(!segments_overlap("a-test", "a-other"));
        assert!(!segments_overlap("api-handler", "api-client"));
    }

    #[test]
    fn test_segments_overlap_stopwords() {
        // Stopwords should not cause matches
        assert!(!segments_overlap("code-review", "code-quality"));
        assert!(!segments_overlap("data-validation", "data-migration"));
        assert!(!segments_overlap("test-runner", "test-helper"));
        assert!(!segments_overlap("user-management", "user-auth"));
        assert!(!segments_overlap("file-handler", "file-parser"));
    }

    #[test]
    fn test_skill_references_rule_full_name() {
        // Full name match works
        assert!(skill_references_rule("review authentication patterns", "authentication"));
        assert!(skill_references_rule("check data-validation rules", "data-validation"));
    }

    #[test]
    fn test_skill_references_rule_no_single_segment() {
        // Single segment should NOT match body text
        assert!(!skill_references_rule(
            "this skill handles authentication and security",
            "authentication-review"
        ));
    }

    #[test]
    fn test_skill_references_rule_two_segments() {
        // Two non-stopword segments matching should work
        assert!(skill_references_rule(
            "handles authentication and reviews security patterns",
            "authentication-review"
        ));
    }

    #[test]
    fn test_skill_references_rule_stopword_segments() {
        // Stopword segments should be filtered out
        assert!(!skill_references_rule(
            "this mentions data somewhere",
            "data-validation"
        ));
    }

    #[test]
    fn test_build_graph_agent_skill_link() {
        let skills = vec![Skill::new("auth-testing", "Test auth", "# Auth testing")];
        let agents = vec![Agent::new("auth-specialist", "Auth expert", "You handle auth")];
        let rules = vec![];
        let modules = vec![];

        let graph = ArtifactGraph::build(&skills, &agents, &rules, &modules);
        assert_eq!(graph.relation_count(), 1);

        let (_, agents) = graph.apply(skills, agents, &rules);
        assert_eq!(
            agents[0].skills.as_ref().unwrap(),
            &vec!["auth-testing".to_string()]
        );
    }

    #[test]
    fn test_build_graph_no_false_matches() {
        let skills = vec![Skill::new("build-check", "Build checker", "# Build")];
        let agents = vec![Agent::new("auth-specialist", "Auth expert", "You handle auth")];

        let graph = ArtifactGraph::build(&skills, &agents, &[], &[]);
        assert_eq!(graph.relation_count(), 0);
    }

    #[test]
    fn test_build_graph_no_stopword_false_matches() {
        // "data" is a stopword - should NOT match
        let skills = vec![Skill::new("data-processor", "Process data", "# Data processing")];
        let agents = vec![Agent::new("data-validator", "Validate data", "You validate")];

        let graph = ArtifactGraph::build(&skills, &agents, &[], &[]);
        assert_eq!(graph.relation_count(), 0, "Stopword 'data' should not cause match");
    }

    #[test]
    fn test_apply_skill_see_also() {
        let skills = vec![Skill::new(
            "auth-review",
            "Review auth code",
            "# Auth Review\n\nReview authentication patterns.",
        )];
        let agents = vec![];
        let rules = vec![Rule::module(
            "auth",
            vec!["src/auth/**".into()],
            vec!["# Auth rules".into()],
        )];
        let modules = vec![];

        let graph = ArtifactGraph::build(&skills, &agents, &rules, &modules);
        let (skills, _) = graph.apply(skills, agents, &rules);

        assert!(
            skills[0].body.contains("See also:"),
            "Skill body should contain See also reference"
        );
        assert!(skills[0].body.contains(".claude/rules/auth.md"));
    }

    #[test]
    fn test_apply_preserves_existing_skills() {
        let skills = vec![Skill::new("auth-testing", "Test auth", "# Auth testing")];
        let mut agent = Agent::new("auth-specialist", "Auth expert", "You handle auth");
        agent.skills = Some(vec!["existing-skill".to_string()]);
        let agents = vec![agent];

        let graph = ArtifactGraph::build(&skills, &agents, &[], &[]);
        let (_, agents) = graph.apply(skills, agents, &[]);

        let agent_skills = agents[0].skills.as_ref().unwrap();
        assert!(agent_skills.contains(&"existing-skill".to_string()));
        assert!(agent_skills.contains(&"auth-testing".to_string()));
    }

    #[test]
    fn test_rule_governs_module() {
        let skills = vec![];
        let agents = vec![Agent::new("auth-specialist", "Auth expert", "Auth prompt")];
        let rules = vec![Rule::module(
            "auth-security",
            vec!["src/auth/**".into()],
            vec!["# Security".into()],
        )];
        let modules = vec![DetectedModule::new("auth", "Authentication")
            .paths(vec!["src/auth/".into()])];

        let graph = ArtifactGraph::build(&skills, &agents, &rules, &modules);
        assert!(graph.relation_count() > 0, "Rule should govern auth module");
    }

    #[test]
    fn test_no_duplicate_see_also() {
        let skill = Skill::new(
            "auth-review",
            "Review auth",
            "# Auth Review\n\nSee also: existing refs",
        );
        let rules = vec![Rule::module(
            "auth",
            vec!["src/auth/**".into()],
            vec!["# Auth".into()],
        )];

        let graph = ArtifactGraph::build(&[skill.clone()], &[], &rules, &[]);
        let (skills, _) = graph.apply(vec![skill], vec![], &rules);

        // Should not add a second "See also"
        let count = skills[0].body.matches("See also:").count();
        assert_eq!(count, 1, "Should not duplicate See also sections");
    }

    #[test]
    fn test_path_matches_module_boundary() {
        let modules = vec![
            DetectedModule::new("auth", "Authentication")
                .paths(vec!["src/auth/".into()]),
            DetectedModule::new("auth-legacy", "Legacy auth")
                .paths(vec!["src/auth-legacy/".into()]),
        ];

        // "src/auth/**" should match "src/auth/" but NOT "src/auth-legacy/"
        let rule_paths = vec!["src/auth/**".into()];
        assert!(path_matches_module(&rule_paths, "auth", &modules));
        assert!(
            !path_matches_module(&rule_paths, "auth-legacy", &modules),
            "src/auth/** should not match src/auth-legacy/"
        );
    }

    #[test]
    fn test_path_matches_module_exact() {
        let modules = vec![DetectedModule::new("api", "API layer")
            .paths(vec!["src/api/".into()])];

        let rule_paths = vec!["src/api/**".into()];
        assert!(path_matches_module(&rule_paths, "api", &modules));

        // Exact prefix match with path separator
        let rule_paths_exact = vec!["src/api/".into()];
        assert!(path_matches_module(&rule_paths_exact, "api", &modules));
    }

    #[test]
    fn test_skill_body_no_single_common_segment_match() {
        // Rule "data-validation" should NOT match a skill just because it mentions "data"
        let skills = vec![Skill::new(
            "review-code",
            "Review code",
            "# Code Review\n\nReview data models and validation logic.",
        )];
        let rules = vec![Rule::module(
            "data-validation",
            vec!["src/validation/**".into()],
            vec!["# Validation".into()],
        )];

        let graph = ArtifactGraph::build(&skills, &[], &rules, &[]);

        // "data" and "validation" are both stopwords, so no segment match.
        // "data-validation" as full name might match body text if present.
        // In this case, body contains "data" and "validation" as separate words,
        // but the full string "data-validation" is not in the body.
        let skill_rule_rels: Vec<_> = graph
            .relations
            .iter()
            .filter(|r| matches!(r.kind, RelationKind::SkillReferencesRule))
            .collect();
        assert_eq!(
            skill_rule_rels.len(),
            0,
            "Stopword segments should not cause false matches"
        );
    }
}
