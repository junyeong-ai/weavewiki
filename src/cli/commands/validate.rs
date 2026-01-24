//! Validate Command
//!
//! Validates generated Claude Code plugin artifacts:
//! - CLAUDE.md content quality
//! - Skills, Agents, Rules structure and references
//! - Cross-artifact consistency
//! - Evidence traceability

use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml_bw as serde_yaml;

use crate::cli::util::ProjectState;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::validation::{ConsistencyResult, CrossValidationResult, TierFilterResult};
use crate::types::{
    Agent, ClaudegenError, DevelopmentCommand, DiagnosticLevel, ProjectMemory, Result, Rule, Skill,
};

/// Minimum acceptable Tier3 (constraint) content ratio
const MIN_TIER3_RATIO: f32 = 0.3;

/// Detect if the project is a monorepo by checking for workspace configuration files
/// Aligned with project_detection.rs detect_workspace() for consistency
fn detect_monorepo(root: &Path) -> bool {
    // File-based workspace indicators
    let file_indicators = [
        "pnpm-workspace.yaml",
        "turbo.json",
        "lerna.json",
        "nx.json",
        "go.work",
    ];
    if file_indicators
        .iter()
        .any(|f| root.join(f).exists())
    {
        return true;
    }

    // Content-based workspace indicators
    let content_checks: &[(&str, &str)] = &[
        ("Cargo.toml", "[workspace]"),
        ("package.json", "\"workspaces\""),
        ("settings.gradle", "include"),
        ("settings.gradle.kts", "include"),
        ("pom.xml", "<modules>"),
    ];
    content_checks.iter().any(|(file, pattern)| {
        fs::read_to_string(root.join(file))
            .map(|content| content.contains(pattern))
            .unwrap_or(false)
    })
}

pub async fn run(
    path: Option<PathBuf>,
    severity: &str,
    config_path: Option<&Path>,
) -> Result<()> {
    let root =
        path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Note: config_path reserved for future config-driven validation
    let _ = config_path;

    println!("Validating generated plugin output...");
    println!("  Root: {}", root.display());

    // Build file registry for reference validation
    let file_registry = VerifiedFileRegistry::build(&root).await?;
    println!("  Files indexed: {}", file_registry.file_count());
    println!();

    // Load generated artifacts
    let (claude_md, skills, agents, rules) = load_artifacts(&root)?;

    let has_artifacts = claude_md.is_some() || !skills.is_empty() || !agents.is_empty() || !rules.is_empty();
    if !has_artifacts {
        println!("No generated artifacts found.");
        println!("  Expected locations:");
        println!("    - CLAUDE.md (root)");
        println!("    - {{project}}-plugin/skills/{{skill}}/SKILL.md");
        println!("    - {{project}}-plugin/agents/*.md");
        println!("    - .claude/rules/*.md");
        return Ok(());
    }

    // Run validations
    let claude_md_content = claude_md.as_ref().map(|m| m.to_markdown()).unwrap_or_default();
    let tier_result = TierFilterResult::check(&skills, &agents, &rules, &claude_md_content);
    let is_monorepo = detect_monorepo(&root);
    let consistency = ConsistencyResult::check(is_monorepo, &skills, &agents, &rules);
    let cross_validation = claude_md
        .as_ref()
        .map(|md| CrossValidationResult::check(&skills, &agents, &rules, md, &file_registry))
        .unwrap_or_default();

    // Collect and report issues
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut info: Vec<String> = Vec::new();

    // Artifact-level validation (meta: names, descriptions, tools, paths)
    for skill in &skills {
        for issue in skill.validate() {
            let msg = format!("skill/{}: {}", skill.name, issue.message);
            match issue.severity {
                DiagnosticLevel::Error => errors.push(msg),
                DiagnosticLevel::Warning => warnings.push(msg),
                DiagnosticLevel::Info => info.push(msg),
            }
        }
    }

    for agent in &agents {
        for issue in agent.validate() {
            let msg = format!("agent/{}: {}", agent.name, issue.message);
            match issue.severity {
                DiagnosticLevel::Error => errors.push(msg),
                DiagnosticLevel::Warning => warnings.push(msg),
                DiagnosticLevel::Info => info.push(msg),
            }
        }
    }

    for rule in &rules {
        for issue in rule.validate() {
            let msg = format!("rule/{}: {}", rule.name, issue.message);
            match issue.severity {
                DiagnosticLevel::Error => errors.push(msg),
                DiagnosticLevel::Warning => warnings.push(msg),
                DiagnosticLevel::Info => info.push(msg),
            }
        }
    }

    // Tier filter issues
    if !tier_result.passed {
        errors.push(format!(
            "Tier1 (generic) content detected: {} items",
            tier_result.tier1_count
        ));
    }
    if tier_result.tier3_ratio < MIN_TIER3_RATIO && (skills.len() + agents.len() + rules.len()) > 0 {
        warnings.push(format!(
            "Low Tier3 (constraint) ratio: {:.0}% (target: {:.0}%+)",
            tier_result.tier3_ratio * 100.0,
            MIN_TIER3_RATIO * 100.0
        ));
    }

    // Consistency issues
    for issue in &consistency.issues {
        errors.push(issue.clone());
    }

    // Cross-validation issues
    if cross_validation.evidence_traceability.invalid_references > 0 {
        errors.push(format!(
            "Invalid file references: {}",
            cross_validation.evidence_traceability.invalid_references
        ));
    }

    if !cross_validation.plan_consistency.passed {
        for missing in &cross_validation.plan_consistency.missing_coverage {
            warnings.push(format!("Missing coverage: {missing}"));
        }
    }

    // Artifact count info
    info.push(format!(
        "Artifacts found: {} skills, {} agents, {} rules, CLAUDE.md: {}",
        skills.len(),
        agents.len(),
        rules.len(),
        if claude_md.is_some() { "✓" } else { "✗" }
    ));
    info.push(format!(
        "Content tiers: Tier1={}, Tier2={}, Tier3={} ({:.0}%)",
        tier_result.tier1_count,
        tier_result.tier2_count,
        tier_result.tier3_count,
        tier_result.tier3_ratio * 100.0
    ));

    // Filter by severity
    let min_level = match severity.to_lowercase().as_str() {
        "error" => DiagnosticLevel::Error,
        "warning" => DiagnosticLevel::Warning,
        _ => DiagnosticLevel::Info,
    };

    // Print results
    if min_level <= DiagnosticLevel::Info {
        for msg in &info {
            println!("  ℹ {msg}");
        }
    }

    if min_level <= DiagnosticLevel::Warning {
        for msg in &warnings {
            println!("  ⚠ {msg}");
        }
    }

    for msg in &errors {
        println!("  ✗ {msg}");
    }

    println!();

    // Summary
    if errors.is_empty() && warnings.is_empty() {
        println!("✓ Validation passed");
    } else {
        println!(
            "Validation complete: {} error(s), {} warning(s)",
            errors.len(),
            warnings.len()
        );
    }

    if !errors.is_empty() {
        return Err(ClaudegenError::Verification(format!(
            "{} validation error(s) found",
            errors.len()
        )));
    }

    Ok(())
}

/// Loaded artifacts from disk
type LoadedArtifacts = (Option<ProjectMemory>, Vec<Skill>, Vec<Agent>, Vec<Rule>);

fn load_artifacts(root: &Path) -> Result<LoadedArtifacts> {
    let claude_md = load_claude_md(root)?;
    let rules = load_markdown_artifacts(root, ".claude/rules", parse_rule)?;

    // Try plugin directory first (*-plugin/), fall back to legacy .claude/ paths
    let (skills, agents) = if let Some(plugin_dir) = find_plugin_dir(root) {
        let skills = load_plugin_skills(&plugin_dir)?;
        let agents = load_markdown_artifacts(&plugin_dir, "agents", parse_agent)?;
        (skills, agents)
    } else {
        // Legacy fallback
        let skills = load_markdown_artifacts(root, ".claude/skills", parse_skill)?;
        let agents = load_markdown_artifacts(root, ".claude/agents", parse_agent)?;
        (skills, agents)
    };

    Ok((claude_md, skills, agents, rules))
}

/// Find plugin directory by checking state.toml first, then *-plugin/ convention
fn find_plugin_dir(root: &Path) -> Option<PathBuf> {
    // 1. Check state.toml for last_output_dir (using root, not CWD)
    if let Ok(state) = ProjectState::load_from(root)
        && let Some(ref output_dir) = state.last_output_dir
    {
        // output_dir could be either:
        // - The plugin directory itself ({base}/{project}-plugin)
        // - The base directory ({base}) containing a *-plugin subdirectory
        if is_valid_plugin_dir(output_dir) {
            return Some(output_dir.clone());
        }

        // Search for *-plugin subdirectory within output_dir
        if let Some(plugin_dir) = search_plugin_subdir(output_dir) {
            return Some(plugin_dir);
        }
    }

    // 2. Search for *-plugin directories at project root
    if let Some(plugin_dir) = search_plugin_subdir(root) {
        return Some(plugin_dir);
    }

    None
}

/// Search for *-plugin subdirectory within a directory
fn search_plugin_subdir(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() && is_valid_plugin_dir(&path) {
            return Some(path);
        }
    }

    None
}

fn is_valid_plugin_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.ends_with("-plugin") {
        return false;
    }
    // Valid if has manifest or skills/agents subdirectories
    path.join(".claude-plugin/plugin.json").exists()
        || path.join("skills").exists()
        || path.join("agents").exists()
}

/// Load skills from plugin directory structure: {plugin}/skills/{skill}/SKILL.md
fn load_plugin_skills(plugin_dir: &Path) -> Result<Vec<Skill>> {
    let skills_dir = plugin_dir.join("skills");
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in fs::read_dir(&skills_dir)?.flatten() {
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }

        let skill_file = skill_dir.join("SKILL.md");
        if skill_file.exists()
            && let Ok(content) = fs::read_to_string(&skill_file)
            && let Some(skill) = parse_skill(&skill_file, &content)
        {
            skills.push(skill);
        }
    }
    Ok(skills)
}

fn load_claude_md(root: &Path) -> Result<Option<ProjectMemory>> {
    let path = root.join("CLAUDE.md");
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        Ok(Some(parse_claude_md(&content)))
    } else {
        Ok(None)
    }
}

fn load_markdown_artifacts<T, F>(root: &Path, subdir: &str, parser: F) -> Result<Vec<T>>
where
    F: Fn(&Path, &str) -> Option<T>,
{
    let dir = root.join(subdir);
    let mut items = Vec::new();

    if !dir.exists() {
        return Ok(items);
    }

    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        let is_markdown = path.extension().map(|e| e == "md").unwrap_or(false);

        if is_markdown
            && let Ok(content) = fs::read_to_string(&path)
            && let Some(item) = parser(&path, &content)
        {
            items.push(item);
        }
    }

    Ok(items)
}

fn parse_claude_md(content: &str) -> ProjectMemory {
    let mut memory = ProjectMemory::default();
    let mut current_section = String::new();
    let mut section_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if line.starts_with("# ") {
            memory.overview = line.trim_start_matches("# ").to_string();
        } else if line.starts_with("## ") {
            // Save previous section
            if !current_section.is_empty() && !section_lines.is_empty() {
                save_section(&mut memory, &current_section, &section_lines);
            }
            current_section = line.trim_start_matches("## ").to_string();
            section_lines.clear();
        } else if !line.trim().is_empty() && !current_section.is_empty() {
            section_lines.push(line.to_string());
        }
    }

    // Save last section
    if !current_section.is_empty() && !section_lines.is_empty() {
        save_section(&mut memory, &current_section, &section_lines);
    }

    memory
}

fn save_section(memory: &mut ProjectMemory, section: &str, lines: &[String]) {
    let joined = lines.join("\n");
    match section.to_lowercase().as_str() {
        s if s.contains("architecture") => memory.architecture = Some(joined),
        s if s.contains("command") => {
            // Parse command lines like "- **name**: `cmd` - description"
            for line in lines {
                if let Some(cmd) = parse_command_line(line) {
                    memory.commands.push(cmd);
                }
            }
        }
        s if s.contains("standard") || s.contains("convention") => {
            memory.standards = lines.to_vec();
        }
        _ => {}
    }
}

fn parse_command_line(line: &str) -> Option<DevelopmentCommand> {
    // Parse "- **name**: `cmd` - description" or simpler formats
    let line = line.trim_start_matches('-').trim();

    // Simple format: just extract name and command
    if line.contains('`') {
        let parts: Vec<&str> = line.splitn(2, '`').collect();
        if parts.len() >= 2 {
            let name = parts[0]
                .trim()
                .trim_start_matches("**")
                .trim_end_matches("**")
                .trim_end_matches(':')
                .trim()
                .to_string();
            let rest = parts[1];
            let cmd_end = rest.find('`').unwrap_or(rest.len());
            let command = rest[..cmd_end].to_string();
            let description = if cmd_end < rest.len() {
                let desc = rest[cmd_end + 1..].trim().trim_start_matches('-').trim();
                if desc.is_empty() {
                    None
                } else {
                    Some(desc.to_string())
                }
            } else {
                None
            };

            if !name.is_empty() && !command.is_empty() {
                return Some(DevelopmentCommand {
                    name,
                    command,
                    description,
                });
            }
        }
    }

    None
}

/// Parse tools field that may be comma-separated string or YAML array
/// Claude Code outputs comma-separated: "tools: Read, Write, Edit"
/// But YAML arrays are also valid: "tools:\n  - Read\n  - Write"
fn parse_tools_field(value: &serde_yaml::Value) -> Option<Vec<String>> {
    // Try as string first (comma-separated)
    if let Some(s) = value.as_str() {
        let tools: Vec<String> = s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if !tools.is_empty() {
            return Some(tools);
        }
    }

    // Fall back to YAML array
    value.as_sequence().map(|seq| {
        seq.iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect()
    })
}

fn parse_skill(path: &Path, content: &str) -> Option<Skill> {
    // Try YAML frontmatter first (generated format)
    if let Some((frontmatter, body)) = parse_frontmatter(content) {
        let name = frontmatter
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| extract_skill_name_from_path(path));
        let description = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();

        let mut skill = Skill::new(&name, &description, &body);

        // Extract allowed-tools (comma-separated or array)
        if let Some(tools) = frontmatter.get("allowed-tools").and_then(parse_tools_field) {
            skill.allowed_tools = Some(tools);
        }

        // Extract context mode
        if let Some(context_str) = frontmatter.get("context").and_then(|v| v.as_str())
            && let Ok(context) = context_str.parse()
        {
            skill.context = Some(context);
        }

        // Extract agent name (requires context: fork)
        if let Some(agent) = frontmatter.get("agent").and_then(|v| v.as_str()) {
            skill.agent = Some(agent.to_string());
        }

        // Extract model
        if let Some(model) = frontmatter.get("model").and_then(|v| v.as_str()) {
            skill.model = Some(model.to_string());
        }

        // Extract user-invocable
        if let Some(user_invocable) = frontmatter.get("user-invocable").and_then(|v| v.as_bool()) {
            skill.user_invocable = Some(user_invocable);
        }

        // Extract argument-hint
        if let Some(hint) = frontmatter.get("argument-hint").and_then(|v| v.as_str()) {
            skill.argument_hint = Some(hint.to_string());
        }

        // Extract disable-model-invocation
        if let Some(disable) = frontmatter
            .get("disable-model-invocation")
            .and_then(|v| v.as_bool())
        {
            skill.disable_model_invocation = Some(disable);
        }

        return Some(skill);
    }

    // Fallback: simple text parsing for non-frontmatter files
    let name = extract_skill_name_from_path(path);
    let body = extract_fallback_body(content);
    Some(Skill::new(&name, &name, &body))
}

fn extract_fallback_body(content: &str) -> String {
    content
        .lines()
        .skip_while(|l| l.starts_with('#') || l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_skill_name_from_path(path: &Path) -> String {
    let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if filename.eq_ignore_ascii_case("skill") {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    } else {
        filename.to_string()
    }
}

fn parse_agent(path: &Path, content: &str) -> Option<Agent> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Try YAML frontmatter first (generated format)
    if let Some((frontmatter, body)) = parse_frontmatter(content) {
        let fm_name = frontmatter
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.clone());
        let description = frontmatter
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(&fm_name)
            .to_string();

        let mut agent = Agent::new(&fm_name, &description, &body);

        // Extract skills from frontmatter (YAML array only)
        if let Some(skills) = frontmatter.get("skills").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
        }) {
            agent.skills = Some(skills);
        }

        // Extract tools (comma-separated or array)
        if let Some(tools) = frontmatter.get("tools").and_then(parse_tools_field) {
            agent.tools = Some(tools);
        }

        // Extract disallowedTools (comma-separated or array)
        if let Some(disallowed) = frontmatter.get("disallowedTools").and_then(parse_tools_field) {
            agent.disallowed_tools = Some(disallowed);
        }

        // Extract permissionMode (FromStr is infallible)
        if let Some(mode_str) = frontmatter.get("permissionMode").and_then(|v| v.as_str()) {
            agent.permission_mode = Some(mode_str.parse().unwrap());
        }

        // Extract model (FromStr is infallible)
        if let Some(model_str) = frontmatter.get("model").and_then(|v| v.as_str()) {
            agent.model = Some(model_str.parse().unwrap());
        }

        // Extract color (FromStr is infallible)
        if let Some(color_str) = frontmatter.get("color").and_then(|v| v.as_str()) {
            agent.color = Some(color_str.parse().unwrap());
        }

        return Some(agent);
    }

    // Fallback: simple text parsing
    let body = extract_fallback_body(content);
    Some(Agent::new(&name, &name, &body))
}

/// Parse YAML frontmatter from markdown content
/// Returns (frontmatter_map, body_after_frontmatter)
fn parse_frontmatter(content: &str) -> Option<(serde_yaml::Mapping, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end_pos = after_first.find("\n---")?;
    let yaml_str = &after_first[..end_pos];
    let body_start = end_pos + 4; // skip "\n---"
    let body = after_first[body_start..].trim_start().to_string();

    let frontmatter: serde_yaml::Mapping = serde_yaml::from_str(yaml_str).ok()?;
    Some((frontmatter, body))
}

fn parse_rule(path: &Path, content: &str) -> Option<Rule> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Try YAML frontmatter first (for path-based rules)
    if let Some((frontmatter, body)) = parse_frontmatter(content) {
        // Extract paths from frontmatter
        let paths = frontmatter.get("paths").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
        });

        let lines: Vec<String> = body
            .lines()
            .filter(|l| !l.starts_with("# ") && !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();

        if lines.is_empty() {
            return None;
        }

        let mut rule = Rule::new(&name, lines);
        if let Some(p) = paths {
            rule.paths = Some(p);
        }
        return Some(rule);
    }

    // Fallback: simple text parsing
    let lines: Vec<String> = content
        .lines()
        .filter(|l| !l.starts_with("# ") && !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();

    if lines.is_empty() {
        return None;
    }

    Some(Rule::new(&name, lines))
}
