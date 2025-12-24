use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convention {
    pub id: String,
    pub category: ConventionCategory,
    pub pattern: PatternDefinition,
    pub examples: Vec<ConventionExample>,
    pub frequency: u32,
    pub confidence: f32,
    pub last_updated: DateTime<Utc>,
}

impl Convention {
    pub fn new(
        id: impl Into<String>,
        category: ConventionCategory,
        pattern: PatternDefinition,
    ) -> Self {
        Self {
            id: id.into(),
            category,
            pattern,
            examples: Vec::new(),
            frequency: 0,
            confidence: 1.0,
            last_updated: Utc::now(),
        }
    }

    pub fn add_example(&mut self, example: ConventionExample) {
        self.examples.push(example);
        self.frequency += 1;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConventionCategory {
    Naming,
    Structure,
    Style,
    Pattern,
    Testing,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDefinition {
    pub description: String,
    pub regex: Option<String>,
    pub template: Option<String>,
    pub rules: Option<Vec<ConventionRule>>,
}

impl PatternDefinition {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            regex: None,
            template: None,
            rules: None,
        }
    }

    pub fn with_regex(mut self, regex: impl Into<String>) -> Self {
        self.regex = Some(regex.into());
        self
    }

    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.template = Some(template.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionRule {
    pub name: String,
    pub condition: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionExample {
    pub location: String,
    pub snippet: String,
}

impl ConventionExample {
    pub fn new(location: impl Into<String>, snippet: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            snippet: snippet.into(),
        }
    }
}
