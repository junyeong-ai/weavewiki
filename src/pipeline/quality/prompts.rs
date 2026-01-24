use serde::{Deserialize, Serialize};

use super::judge::IssueSeverity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub description: String,
    pub severity: IssueSeverity,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Criterion {
    pub name: String,
    pub description: String,
    pub weight: f32,
}

pub struct QualityPrompts {
    max_steps: usize,
}

impl Default for QualityPrompts {
    fn default() -> Self {
        Self { max_steps: 10 }
    }
}

impl QualityPrompts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_steps(mut self, max: usize) -> Self {
        self.max_steps = max;
        self
    }

    pub fn max_steps(&self) -> usize {
        self.max_steps
    }

    pub fn analysis_prompt(&self, context: &str, focus: &[String]) -> String {
        let focus_list = focus
            .iter()
            .map(|f| format!("- {}", f))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are in ANALYSIS mode. Follow this structured thinking framework:

<context>
{context}
</context>

<focus_areas>
{focus_list}
</focus_areas>

<thinking>
1. **STATE**: Describe current state with specific evidence (file paths, code references)
2. **ISSUES**: Identify problems with severity (Critical/Major/Minor) and evidence
3. **GAPS**: What is missing? Required elements not present?
4. **ASSESSMENT**: Quality score (0.0-1.0) with justification
</thinking>

Output JSON:
```json
{{
  "state_summary": "...",
  "issues": [
    {{"id": "ISSUE-001", "description": "...", "severity": "critical|major|minor", "evidence": ["..."]}}
  ],
  "gaps": ["..."],
  "quality_score": 0.0,
  "strengths": ["..."],
  "weaknesses": ["..."]
}}
```"#
        )
    }

    pub fn improvement_prompt(&self, context: &str, issues: &[Issue]) -> String {
        let issues_list = issues
            .iter()
            .map(|i| {
                format!(
                    "- **{}** [{}]: {}\n  Evidence: {}",
                    i.id,
                    match i.severity {
                        IssueSeverity::Critical => "CRITICAL",
                        IssueSeverity::Major => "MAJOR",
                        IssueSeverity::Minor => "MINOR",
                    },
                    i.description,
                    i.evidence.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are in IMPROVEMENT mode.

<context>
{context}
</context>

<issues>
{issues_list}
</issues>

<thinking>
1. **UNDERSTAND**: Root cause analysis for each issue
2. **PROPOSE**: Specific changes (exact content to add/modify/remove)
3. **VERIFY**: Side effects consideration
4. **PRIORITIZE**: High impact, low effort first
</thinking>

Output JSON:
```json
{{
  "improvements": [
    {{"issue_id": "ISSUE-001", "action": "add|modify|remove", "target": "...", "content": "...", "rationale": "..."}}
  ],
  "expected_quality_improvement": 0.0
}}
```"#
        )
    }

    pub fn verification_prompt(&self, context: &str, criteria: &[Criterion]) -> String {
        let criteria_list = criteria
            .iter()
            .map(|c| {
                format!(
                    "- **{}** (weight: {:.1}): {}",
                    c.name, c.weight, c.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are in VERIFICATION mode.

<context>
{context}
</context>

<criteria>
{criteria_list}
</criteria>

<thinking>
1. **CHECK**: Does artifact meet each criterion? Evidence for/against
2. **SCORE**: Rate compliance (0.0-1.0) per criterion
3. **ISSUES**: Any remaining or new problems?
</thinking>

Output JSON:
```json
{{
  "criterion_results": [
    {{"criterion": "...", "score": 0.0, "compliance_evidence": ["..."], "non_compliance_evidence": ["..."]}}
  ],
  "overall_score": 0.0,
  "new_issues": [],
  "ready_to_converge": false
}}
```"#
        )
    }

    pub fn convergence_prompt(&self, context: &str, threshold: f32, current_score: f32) -> String {
        let gap = threshold - current_score;
        format!(
            r#"You are in CONVERGENCE DECISION mode.

<context>
{context}
</context>

<metrics>
- Current score: {current_score:.2}
- Target: {threshold:.2}
- Gap: {gap:.2}
</metrics>

<thinking>
1. **THRESHOLD**: Current >= target?
2. **PLATEAU**: Diminishing returns?
3. **CRITICAL_ISSUES**: Any blockers remaining?
4. **DECISION**: Continue or converge?
</thinking>

Output JSON:
```json
{{
  "should_converge": false,
  "rationale": "...",
  "remaining_issues": ["..."],
  "confidence": 0.0
}}
```"#
        )
    }

    pub fn compaction_prompt(&self, messages: &str, keep_coding_instructions: bool) -> String {
        if keep_coding_instructions {
            format!(
                r#"Summarize this conversation, preserving:
- Key decisions and rationale
- File paths and code changes
- Error fixes and their solutions
- Current work status

Conversation:
{messages}

Provide structured summary with sections:
1. Primary Request
2. Key Technical Concepts
3. Files Modified
4. Errors Fixed
5. Current Status
6. Next Steps"#
            )
        } else {
            format!(
                r#"Summarize this conversation concisely:
- Main goal
- Key decisions
- Current status

Conversation:
{messages}

Brief summary:"#
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_prompt() {
        let prompts = QualityPrompts::new();
        let prompt = prompts.analysis_prompt("test content", &["quality".to_string()]);
        assert!(prompt.contains("ANALYSIS mode"));
        assert!(prompt.contains("quality"));
    }

    #[test]
    fn test_improvement_prompt() {
        let prompts = QualityPrompts::new();
        let issues = vec![Issue {
            id: "TEST-001".to_string(),
            description: "Test issue".to_string(),
            severity: IssueSeverity::Major,
            evidence: vec!["evidence".to_string()],
        }];
        let prompt = prompts.improvement_prompt("context", &issues);
        assert!(prompt.contains("IMPROVEMENT mode"));
        assert!(prompt.contains("TEST-001"));
    }
}
