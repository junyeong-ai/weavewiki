//! Distributed Analysis Module
//!
//! Implements parallel chunked analysis for 100% file coverage.
//! Uses a Map-Reduce pattern to analyze large codebases efficiently.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::response::generate_schema;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::ai::LlmProvider;
use crate::ai::validation::deserialize_llm_response;
use crate::config::DistributedAnalysisConfig;
use crate::types::{ClaudegenError, Result};

const MAX_RETRIES: usize = 3;
const INITIAL_BACKOFF_MS: u64 = 1000;

use super::super::context::{FileMetadata, VerifiedFileRegistry};
use super::deep_analyzer::{DiscoveredConstraint, Gotcha, ModuleDependency, PatternInstance};

// =============================================================================
// CHUNK TYPES
// =============================================================================

/// A chunk of files for parallel analysis
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisChunk {
    pub chunk_id: String,
    pub module_path: String,
    pub files: Vec<String>,
    pub total_lines: usize,
    pub estimated_tokens: usize,
}

impl AnalysisChunk {
    pub fn new(chunk_id: String, module_path: String, files: Vec<String>) -> Self {
        Self {
            chunk_id,
            module_path,
            files,
            total_lines: 0,
            estimated_tokens: 0,
        }
    }

    pub fn metrics(mut self, total_lines: usize, estimated_tokens: usize) -> Self {
        self.total_lines = total_lines;
        self.estimated_tokens = estimated_tokens;
        self
    }
}

/// Result from analyzing a single chunk
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ChunkAnalysisResult {
    pub chunk_id: String,
    pub module_path: String,
    pub patterns: Vec<PatternInstance>,
    pub conventions: ChunkConventions,
    pub constraints: Vec<DiscoveredConstraint>,
    pub gotchas: Vec<Gotcha>,
    pub dependencies: Vec<ModuleDependency>,
    pub file_count: usize,
    pub lines_analyzed: usize,
    pub confidence: f32,
}

/// Conventions discovered in a chunk
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ChunkConventions {
    pub naming_patterns: HashMap<NamingCase, usize>,
    pub error_handling: HashMap<ErrorStyle, usize>,
    pub async_patterns: HashMap<AsyncStyle, usize>,
    pub import_patterns: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamingCase {
    #[default]
    SnakeCase,
    CamelCase,
    PascalCase,
    KebabCase,
    ScreamingSnakeCase,
}

impl std::fmt::Display for NamingCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStyle {
    ResultType,
    ExceptionBased,
    NullCheck,
    EarlyReturn,
    MonadicChain,
}

impl std::fmt::Display for ErrorStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AsyncStyle {
    AsyncAwait,
    Callbacks,
    Promises,
    Channels,
    Actors,
}

// =============================================================================
// CHUNKING STRATEGY
// =============================================================================

pub struct ChunkingStrategy;

impl ChunkingStrategy {
    /// Create chunks from file registry respecting token limits
    pub fn create_chunks(
        registry: &VerifiedFileRegistry,
        config: &DistributedAnalysisConfig,
    ) -> Vec<AnalysisChunk> {
        let files_by_module = registry.files_by_module();
        let mut chunks = Vec::new();
        let mut chunk_counter = 0;

        for (module, files) in files_by_module {
            let module_chunks = Self::chunk_module(
                &module,
                files,
                config.max_tokens_per_chunk,
                &mut chunk_counter,
            );
            chunks.extend(module_chunks);
        }

        chunks
    }

    fn chunk_module(
        module: &str,
        files: Vec<&FileMetadata>,
        max_tokens: usize,
        counter: &mut usize,
    ) -> Vec<AnalysisChunk> {
        let mut chunks = Vec::new();
        let mut current_files = Vec::new();
        let mut current_tokens = 0usize;
        let mut current_lines = 0usize;

        for file in files {
            if current_tokens + file.estimated_tokens > max_tokens && !current_files.is_empty() {
                *counter += 1;
                chunks.push(
                    AnalysisChunk::new(
                        format!("chunk-{}", counter),
                        module.to_string(),
                        current_files.clone(),
                    )
                    .metrics(current_lines, current_tokens),
                );
                current_files.clear();
                current_tokens = 0;
                current_lines = 0;
            }

            current_files.push(file.path.clone());
            current_tokens += file.estimated_tokens;
            current_lines += file.line_count;
        }

        if !current_files.is_empty() {
            *counter += 1;
            chunks.push(
                AnalysisChunk::new(
                    format!("chunk-{}", counter),
                    module.to_string(),
                    current_files,
                )
                .metrics(current_lines, current_tokens),
            );
        }

        chunks
    }

    /// Extract exported symbol names from file content.
    ///
    /// Language-aware extraction using file extension:
    /// - Rust: `pub fn/struct/enum/trait/mod/type/const`
    /// - Python: top-level `def`/`class`
    /// - TypeScript/JavaScript: `export` declarations
    /// - Go: capitalized `func`/`type`/`var`/`const`
    /// - Java/Kotlin/Scala: `public class/interface/enum/record`
    pub fn extract_exported_symbols(content: &str, file_path: &Path) -> Vec<String> {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mut symbols = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            match ext {
                "rs" => {
                    if let Some(rest) = trimmed.strip_prefix("pub ") {
                        if let Some(sym) = Self::parse_rust_declaration(rest) {
                            symbols.push(sym);
                        }
                    } else if trimmed.starts_with("pub(")
                        && let Some(after_vis) = trimmed.find(") ")
                    {
                        let rest = &trimmed[after_vis + 2..];
                        if let Some(sym) = Self::parse_rust_declaration(rest) {
                            symbols.push(sym);
                        }
                    }
                }
                "py" => {
                    let keyword_rest = line.strip_prefix("async def ")
                        .or_else(|| line.strip_prefix("def "))
                        .or_else(|| line.strip_prefix("class "));
                    if let Some(keyword_rest) = keyword_rest
                        && let Some(name) = keyword_rest.split(|c: char| !c.is_alphanumeric() && c != '_').next()
                        && !name.is_empty()
                    {
                        symbols.push(name.to_string());
                    }
                }
                "ts" | "tsx" | "js" | "jsx" => {
                    if trimmed.starts_with("export ") {
                        let rest = trimmed.strip_prefix("export ").unwrap_or("");
                        let rest = rest.strip_prefix("default ").unwrap_or(rest);
                        let rest = rest.strip_prefix("async ").unwrap_or(rest);
                        for keyword in [
                            "function ", "class ", "const ", "let ", "interface ", "type ",
                            "enum ",
                        ] {
                            if let Some(after) = rest.strip_prefix(keyword)
                                && let Some(name) = after
                                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                                    .next()
                                && !name.is_empty()
                            {
                                symbols.push(name.to_string());
                            }
                        }
                    }
                }
                "go" => {
                    for keyword in ["func ", "type ", "var ", "const "] {
                        if let Some(stripped) = trimmed.strip_prefix(keyword)
                            && let Some(name) = stripped
                                .split(|c: char| !c.is_alphanumeric() && c != '_')
                                .next()
                            && name.starts_with(|c: char| c.is_uppercase())
                        {
                            symbols.push(name.to_string());
                        }
                    }
                }
                "java" | "kt" | "scala" => {
                    if trimmed.starts_with("public ") {
                        for keyword in ["class ", "interface ", "enum ", "record "] {
                            if let Some(pos) = trimmed.find(keyword) {
                                let after = &trimmed[pos + keyword.len()..];
                                if let Some(name) = after
                                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                                    .next()
                                    && !name.is_empty()
                                {
                                    symbols.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        symbols
    }

    /// Parse a Rust declaration after the `pub ` prefix and return a compact
    /// symbol signature like `pub fn foo()` or `pub struct Bar`.
    fn parse_rust_declaration(rest: &str) -> Option<String> {
        let keywords = [
            ("fn ", "pub fn "),
            ("struct ", "pub struct "),
            ("trait ", "pub trait "),
            ("enum ", "pub enum "),
            ("type ", "pub type "),
            ("mod ", "pub mod "),
            ("const ", "pub const "),
            ("async fn ", "pub async fn "),
        ];

        for (keyword, prefix) in &keywords {
            if let Some(after_kw) = rest.strip_prefix(keyword) {
                let ident: String = after_kw
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    let suffix = if keyword.contains("fn ") { "()" } else { "" };
                    return Some(format!("{}{}{}", prefix, ident, suffix));
                }
            }
        }

        None
    }
}

// =============================================================================
// DISTRIBUTED ANALYZER
// =============================================================================

pub struct DistributedAnalyzer {
    provider: Arc<dyn LlmProvider>,
    config: DistributedAnalysisConfig,
}

impl DistributedAnalyzer {
    pub fn new(provider: Arc<dyn LlmProvider>, config: DistributedAnalysisConfig) -> Self {
        Self { provider, config }
    }

    /// Analyze all chunks in parallel with bounded concurrency
    pub async fn analyze_all_chunks(
        &self,
        chunks: Vec<AnalysisChunk>,
        project_root: &Path,
    ) -> Result<Vec<ChunkAnalysisResult>> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_parallel_agents));
        let total_chunks = chunks.len();
        let project_root = project_root.to_path_buf();

        tracing::info!(
            chunk_count = total_chunks,
            max_parallel = self.config.max_parallel_agents,
            "Starting distributed analysis"
        );

        let mut join_set: JoinSet<std::result::Result<ChunkAnalysisResult, (String, String)>> =
            JoinSet::new();

        for chunk in chunks {
            let semaphore = Arc::clone(&semaphore);
            let provider = Arc::clone(&self.provider);
            let root = project_root.clone();
            let config = self.config.clone();

            join_set.spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| (chunk.chunk_id.clone(), format!("Semaphore error: {}", e)))?;
                Self::analyze_chunk(&chunk, &root, &provider, &config)
                    .await
                    .map_err(|e| (chunk.chunk_id.clone(), e.to_string()))
            });
        }

        let mut results = Vec::with_capacity(total_chunks);
        let mut failed = 0usize;

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok(Ok(r)) => results.push(r),
                Ok(Err((chunk_id, error))) => {
                    tracing::warn!(chunk_id = %chunk_id, error = %error, "Chunk analysis failed");
                    failed += 1;
                }
                Err(join_err) => {
                    tracing::error!(error = %join_err, "Task join failed");
                    failed += 1;
                }
            }
        }

        tracing::info!(
            successful = results.len(),
            failed = failed,
            "Distributed analysis complete"
        );

        Ok(results)
    }

    async fn analyze_chunk(
        chunk: &AnalysisChunk,
        project_root: &Path,
        provider: &Arc<dyn LlmProvider>,
        config: &DistributedAnalysisConfig,
    ) -> Result<ChunkAnalysisResult> {
        let mut file_contents = Vec::new();
        let mut total_lines = 0usize;

        let timeout = Duration::from_secs(config.file_read_timeout_secs);

        for file_path in &chunk.files {
            let full_path = project_root.join(file_path);
            match tokio::time::timeout(timeout, tokio::fs::read_to_string(&full_path)).await {
                Ok(Ok(content)) => {
                    total_lines += content.lines().count();
                    file_contents.push((file_path.clone(), content));
                }
                Ok(Err(e)) => {
                    tracing::debug!(file = %file_path, error = %e, "Failed to read file");
                }
                Err(_) => {
                    tracing::warn!(file = %file_path, timeout_secs = config.file_read_timeout_secs, "File read timed out");
                }
            }
        }

        if file_contents.is_empty() {
            return Ok(ChunkAnalysisResult {
                chunk_id: chunk.chunk_id.clone(),
                module_path: chunk.module_path.clone(),
                file_count: 0,
                ..Default::default()
            });
        }

        let analysis = Self::llm_analyze_chunk(
            &file_contents,
            &chunk.module_path,
            provider,
            config.max_file_content_chars,
        )
        .await?;

        Ok(ChunkAnalysisResult {
            chunk_id: chunk.chunk_id.clone(),
            module_path: chunk.module_path.clone(),
            patterns: analysis.patterns,
            conventions: analysis.conventions,
            constraints: analysis.constraints,
            gotchas: analysis.gotchas,
            dependencies: analysis.dependencies,
            file_count: file_contents.len(),
            lines_analyzed: total_lines,
            confidence: analysis.confidence,
        })
    }

    fn build_content_prompt(
        file_contents: &[(String, String)],
        max_chars_per_file: usize,
    ) -> String {
        let mut content_prompt = String::with_capacity(file_contents.len() * 1000);

        for (path, content) in file_contents {
            content_prompt.push_str(&format!("\n=== {} ===\n", path));
            let truncated = if content.len() > max_chars_per_file {
                // Find valid UTF-8 char boundary
                let mut end = max_chars_per_file;
                while end > 0 && !content.is_char_boundary(end) {
                    end -= 1;
                }
                &content[..end]
            } else {
                content
            };
            content_prompt.push_str(truncated);
            content_prompt.push('\n');
        }

        content_prompt
    }

    async fn llm_analyze_chunk(
        file_contents: &[(String, String)],
        module_path: &str,
        provider: &Arc<dyn LlmProvider>,
        max_file_content_chars: usize,
    ) -> Result<ChunkAnalysisOutput> {
        let content_prompt = Self::build_content_prompt(file_contents, max_file_content_chars);

        let prompt = format!(
            r#"Analyze the following code files from module "{module_path}". Extract:

1. **Patterns**: Recurring code patterns (architecture, error handling, concurrency, etc.)
2. **Conventions**: Naming conventions, error handling styles, async patterns
3. **Constraints**: Hidden rules, invariants, anti-patterns to avoid
4. **Gotchas**: Non-obvious pitfalls or tricky behaviors
5. **Dependencies**: Module dependencies and relationships

Focus on project-specific knowledge that would help someone work with this code.
Ignore generic language knowledge.

CODE:
{content_prompt}

Analyze thoroughly and return structured findings."#
        );

        let schema = Self::chunk_analysis_schema();

        let response = Self::retry_llm_call(provider, &prompt, &schema, module_path).await?;
        deserialize_llm_response(&response.content, module_path)
    }

    async fn retry_llm_call(
        provider: &Arc<dyn LlmProvider>,
        prompt: &str,
        schema: &Value,
        context: &str,
    ) -> Result<crate::ai::LlmResponse> {
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            match provider.generate(prompt, schema).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let is_retryable = e.to_string().contains("rate")
                        || e.to_string().contains("timeout")
                        || e.to_string().contains("503")
                        || e.to_string().contains("overloaded");

                    if !is_retryable || attempt == MAX_RETRIES - 1 {
                        return Err(e);
                    }

                    let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * (1 << attempt));
                    tracing::warn!(
                        context = %context,
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        backoff_ms = backoff.as_millis(),
                        error = %e,
                        "LLM call failed, retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ClaudegenError::pipeline(0, "llm_retry", "Max retries exceeded".to_string())
        }))
    }

    fn chunk_analysis_schema() -> Value {
        generate_schema::<ChunkAnalysisOutput>()
    }
}

// =============================================================================
// LLM OUTPUT TYPES
// =============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
struct ChunkAnalysisOutput {
    #[serde(default)]
    patterns: Vec<PatternInstance>,
    #[serde(default)]
    conventions: ChunkConventions,
    #[serde(default)]
    constraints: Vec<DiscoveredConstraint>,
    #[serde(default)]
    gotchas: Vec<Gotcha>,
    #[serde(default)]
    dependencies: Vec<ModuleDependency>,
    #[serde(default)]
    confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_creation() {
        let chunk = AnalysisChunk::new(
            "chunk-1".to_string(),
            "src/pipeline".to_string(),
            vec!["src/pipeline/mod.rs".to_string()],
        )
        .metrics(100, 1500);

        assert_eq!(chunk.chunk_id, "chunk-1");
        assert_eq!(chunk.total_lines, 100);
        assert_eq!(chunk.estimated_tokens, 1500);
    }

    #[test]
    fn test_chunk_analysis_result_default() {
        let result = ChunkAnalysisResult::default();
        assert!(result.patterns.is_empty());
        assert!(result.constraints.is_empty());
        assert_eq!(result.confidence, 0.0);
    }

    // =========================================================================
    // extract_exported_symbols tests
    // =========================================================================

    #[test]
    fn test_extract_exported_symbols_rust() {
        let content = r#"
pub fn process_data() -> Result<()> {
    unimplemented!()
}

pub struct Config {
    pub name: String,
}

pub(crate) enum Mode {
    Fast,
    Slow,
}

fn private_helper() {}

pub trait Provider {
    fn name(&self) -> &str;
}

pub async fn run_server() {}

pub type Alias = Vec<String>;

pub const MAX: usize = 100;

pub mod utils;
"#;
        let path = Path::new("src/lib.rs");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"pub fn process_data()".to_string()));
        assert!(symbols.contains(&"pub struct Config".to_string()));
        assert!(symbols.contains(&"pub trait Provider".to_string()));
        assert!(symbols.contains(&"pub async fn run_server()".to_string()));
        assert!(symbols.contains(&"pub type Alias".to_string()));
        assert!(symbols.contains(&"pub const MAX".to_string()));
        assert!(symbols.contains(&"pub mod utils".to_string()));
        // pub(crate) should also be captured
        assert!(symbols.contains(&"pub enum Mode".to_string()));
        // private function should NOT be captured
        assert!(!symbols.iter().any(|s| s.contains("private_helper")));
    }

    #[test]
    fn test_extract_exported_symbols_python() {
        let content = r#"def hello():
    pass

class MyClass:
    def method(self):
        pass

async def fetch_data():
    pass

    def indented_func():
        pass
"#;
        let path = Path::new("app.py");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"hello".to_string()));
        assert!(symbols.contains(&"MyClass".to_string()));
        assert!(symbols.contains(&"fetch_data".to_string()));
        // Indented definitions should NOT be captured (methods, nested)
        assert!(!symbols.contains(&"indented_func".to_string()));
        assert!(!symbols.contains(&"method".to_string()));
    }

    #[test]
    fn test_extract_exported_symbols_typescript() {
        let content = r#"
export function processData(input: string): void {}
export class UserService {}
export const MAX_RETRIES = 3;
export let counter = 0;
export interface Config {}
export type Result<T> = T | Error;
export enum Status { Active, Inactive }
export default class MainApp {}
export async function fetchUser() {}
function privateHelper() {}
"#;
        let path = Path::new("service.ts");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"processData".to_string()));
        assert!(symbols.contains(&"UserService".to_string()));
        assert!(symbols.contains(&"MAX_RETRIES".to_string()));
        assert!(symbols.contains(&"counter".to_string()));
        assert!(symbols.contains(&"Config".to_string()));
        assert!(symbols.contains(&"Status".to_string()));
        assert!(symbols.contains(&"MainApp".to_string()));
        assert!(symbols.contains(&"fetchUser".to_string()));
        // Non-exported should NOT be captured
        assert!(!symbols.contains(&"privateHelper".to_string()));
    }

    #[test]
    fn test_extract_exported_symbols_go() {
        let content = r#"
func ProcessData(input string) error {
    return nil
}

func helper() {}

type Config struct {
    Name string
}

type internal struct {}

var MaxRetries = 3

const DefaultTimeout = 30

var localCache = make(map[string]string)
"#;
        let path = Path::new("service.go");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"ProcessData".to_string()));
        assert!(symbols.contains(&"Config".to_string()));
        assert!(symbols.contains(&"MaxRetries".to_string()));
        assert!(symbols.contains(&"DefaultTimeout".to_string()));
        // Lowercase (unexported in Go) should NOT be captured
        assert!(!symbols.contains(&"helper".to_string()));
        assert!(!symbols.contains(&"internal".to_string()));
        assert!(!symbols.contains(&"localCache".to_string()));
    }

    #[test]
    fn test_extract_exported_symbols_java() {
        let content = r#"
public class UserService {
    public void process() {}
}

public interface Repository {
    void save();
}

public enum Status {
    ACTIVE, INACTIVE
}

public record UserDto(String name, int age) {}

class InternalHelper {}
"#;
        let path = Path::new("UserService.java");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"UserService".to_string()));
        assert!(symbols.contains(&"Repository".to_string()));
        assert!(symbols.contains(&"Status".to_string()));
        assert!(symbols.contains(&"UserDto".to_string()));
        // Non-public should NOT be captured
        assert!(!symbols.contains(&"InternalHelper".to_string()));
    }

    #[test]
    fn test_extract_exported_symbols_unknown_extension() {
        let content = "some random content\nwith multiple lines";
        let path = Path::new("readme.txt");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_extract_exported_symbols_empty_content() {
        let path = Path::new("empty.rs");
        let symbols = ChunkingStrategy::extract_exported_symbols("", path);
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_extract_exported_symbols_jsx() {
        let content = r#"
export function App() { return <div />; }
export const Header = () => <header />;
"#;
        let path = Path::new("App.jsx");
        let symbols = ChunkingStrategy::extract_exported_symbols(content, path);

        assert!(symbols.contains(&"App".to_string()));
        assert!(symbols.contains(&"Header".to_string()));
    }
}
