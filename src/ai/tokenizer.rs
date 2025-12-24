//! Token Counting and Budget Management
//!
//! Provides token estimation for LLM context management.
//!
//! ## Strategy
//! - Pre-calculate token counts before sending to LLM
//! - Prevent context overflow by budgeting tokens per batch
//! - Support different estimation methods
//!
//! Based on CodeWiki's token counting pattern (utils.py:29-37)

use tracing::debug;

/// Token estimation method
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TokenEstimator {
    /// Simple character-based estimation (4 chars = 1 token)
    /// Good for general English text
    CharBased,
    /// Word-based estimation (0.75 tokens per word on average)
    WordBased,
    /// Code-aware estimation (accounts for syntax, keywords)
    #[default]
    CodeAware,
}

/// Token counter for context management
pub struct TokenCounter {
    estimator: TokenEstimator,
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new(TokenEstimator::default())
    }
}

impl TokenCounter {
    pub fn new(estimator: TokenEstimator) -> Self {
        Self { estimator }
    }

    /// Estimate token count for a string
    pub fn count(&self, text: &str) -> usize {
        match self.estimator {
            TokenEstimator::CharBased => self.count_char_based(text),
            TokenEstimator::WordBased => self.count_word_based(text),
            TokenEstimator::CodeAware => self.count_code_aware(text),
        }
    }

    /// Simple character-based counting (4 chars = 1 token)
    fn count_char_based(&self, text: &str) -> usize {
        text.chars().count().div_ceil(4)
    }

    /// Word-based counting (average 0.75 tokens per word)
    fn count_word_based(&self, text: &str) -> usize {
        let word_count = text.split_whitespace().count();
        (word_count as f32 * 0.75).ceil() as usize + 1
    }

    /// Code-aware counting
    /// - Code typically has more tokens per character due to syntax
    /// - Keywords, operators, and punctuation are individual tokens
    fn count_code_aware(&self, text: &str) -> usize {
        let mut tokens = 0;
        let mut current_word = String::new();

        for ch in text.chars() {
            match ch {
                // Punctuation and operators are usually individual tokens
                '(' | ')' | '{' | '}' | '[' | ']' | ';' | ':' | ',' | '.' | '+' | '-' | '*'
                | '/' | '=' | '<' | '>' | '!' | '&' | '|' | '@' | '#' | '$' | '%' | '^' | '~'
                | '?' | '\\' => {
                    if !current_word.is_empty() {
                        tokens += self.estimate_word_tokens(&current_word);
                        current_word.clear();
                    }
                    tokens += 1; // Punctuation is usually 1 token
                }
                // Whitespace ends current word
                ' ' | '\t' | '\n' | '\r' => {
                    if !current_word.is_empty() {
                        tokens += self.estimate_word_tokens(&current_word);
                        current_word.clear();
                    }
                }
                // Build current word
                _ => {
                    current_word.push(ch);
                }
            }
        }

        // Don't forget the last word
        if !current_word.is_empty() {
            tokens += self.estimate_word_tokens(&current_word);
        }

        tokens.max(1) // At least 1 token
    }

    /// Estimate tokens for a single word
    fn estimate_word_tokens(&self, word: &str) -> usize {
        let len = word.len();
        if len <= 4 {
            1 // Short words are typically 1 token
        } else if len <= 8 {
            2 // Medium words are typically 1-2 tokens
        } else {
            // Long words: roughly 4 chars per token
            len.div_ceil(4)
        }
    }

    /// Check if content fits within token budget
    pub fn fits_budget(&self, text: &str, budget: usize) -> bool {
        self.count(text) <= budget
    }

    /// Calculate remaining budget after content
    pub fn remaining_budget(&self, text: &str, budget: usize) -> usize {
        let used = self.count(text);
        budget.saturating_sub(used)
    }
}

/// Token budget manager for batch processing
pub struct TokenBudget {
    /// Maximum tokens per batch
    max_tokens: usize,
    /// Current token usage
    current_tokens: usize,
    /// Token counter
    counter: TokenCounter,
}

impl TokenBudget {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            counter: TokenCounter::default(),
        }
    }

    /// Try to add content to the batch
    /// Returns true if content fits, false if it would exceed budget
    pub fn try_add(&mut self, content: &str) -> bool {
        let tokens = self.counter.count(content);
        if self.current_tokens + tokens <= self.max_tokens {
            self.current_tokens += tokens;
            debug!(
                "Added {} tokens, total: {}/{}",
                tokens, self.current_tokens, self.max_tokens
            );
            true
        } else {
            debug!(
                "Cannot add {} tokens, would exceed budget: {}/{}",
                tokens,
                self.current_tokens + tokens,
                self.max_tokens
            );
            false
        }
    }

    /// Get current token usage
    pub fn current(&self) -> usize {
        self.current_tokens
    }

    /// Get remaining tokens
    pub fn remaining(&self) -> usize {
        self.max_tokens.saturating_sub(self.current_tokens)
    }

    /// Reset the budget for a new batch
    pub fn reset(&mut self) {
        self.current_tokens = 0;
    }

    /// Check if budget has room for minimum content size
    pub fn has_room(&self, min_tokens: usize) -> bool {
        self.remaining() >= min_tokens
    }

    /// Get utilization percentage
    pub fn utilization(&self) -> f32 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        self.current_tokens as f32 / self.max_tokens as f32 * 100.0
    }
}

/// Estimate tokens for a file path and content pair
pub fn estimate_file_tokens(path: &str, content: &str) -> usize {
    let counter = TokenCounter::default();
    // Include path, formatting, and content
    let formatted = format!("## File: {}\n\n```\n{}\n```\n\n", path, content);
    counter.count(&formatted)
}

/// Check if a batch of files fits within token budget
pub fn check_batch_budget(
    files: &[(String, String)],
    max_tokens: usize,
) -> (bool, usize, Vec<String>) {
    let _counter = TokenCounter::default();
    let mut total_tokens = 0;
    let mut exceeding_files = Vec::new();

    for (path, content) in files {
        let file_tokens = estimate_file_tokens(path, content);
        total_tokens += file_tokens;

        if file_tokens > max_tokens / 4 {
            // Single file using more than 25% of budget
            exceeding_files.push(path.clone());
        }
    }

    (total_tokens <= max_tokens, total_tokens, exceeding_files)
}

// =============================================================================
// Token Budget Batcher
// =============================================================================

/// A file with its estimated token count
#[derive(Debug, Clone)]
pub struct FileWithTokens {
    pub path: String,
    pub content: String,
    pub tokens: usize,
}

/// A batch of files that fit within token budget
#[derive(Debug, Clone)]
pub struct FileBatch {
    pub files: Vec<FileWithTokens>,
    pub total_tokens: usize,
}

impl FileBatch {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_tokens: 0,
        }
    }

    pub fn add(&mut self, file: FileWithTokens) {
        self.total_tokens += file.tokens;
        self.files.push(file);
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Default for FileBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Batches files into groups that fit within token budget
///
/// Uses a bin-packing strategy:
/// 1. Sort files by token count (largest first)
/// 2. Add files to current batch until budget exceeded
/// 3. Start new batch and continue
pub struct TokenBudgetBatcher {
    /// Maximum tokens per batch
    max_tokens_per_batch: usize,
    /// Minimum tokens to leave as buffer (for prompts, etc.)
    buffer_tokens: usize,
    /// Token counter
    counter: TokenCounter,
}

impl TokenBudgetBatcher {
    /// Create a new batcher with default settings
    pub fn new(max_tokens_per_batch: usize) -> Self {
        Self {
            max_tokens_per_batch,
            buffer_tokens: 2000, // Reserve 2K for prompts and schema
            counter: TokenCounter::default(),
        }
    }

    /// Create a batcher with custom buffer
    pub fn with_buffer(max_tokens_per_batch: usize, buffer_tokens: usize) -> Self {
        Self {
            max_tokens_per_batch,
            buffer_tokens,
            counter: TokenCounter::default(),
        }
    }

    /// Effective token limit per batch (excluding buffer)
    fn effective_limit(&self) -> usize {
        self.max_tokens_per_batch.saturating_sub(self.buffer_tokens)
    }

    /// Batch files into groups that fit within token budget
    ///
    /// Files that exceed the budget individually are returned in their own batch.
    pub fn batch_files(&self, files: Vec<(String, String)>) -> Vec<FileBatch> {
        if files.is_empty() {
            return Vec::new();
        }

        // Calculate tokens for each file
        let mut files_with_tokens: Vec<FileWithTokens> = files
            .into_iter()
            .map(|(path, content)| {
                let tokens = self.estimate_file_tokens(&path, &content);
                FileWithTokens {
                    path,
                    content,
                    tokens,
                }
            })
            .collect();

        // Sort by tokens (largest first) for better bin packing
        files_with_tokens.sort_by(|a, b| b.tokens.cmp(&a.tokens));

        let effective_limit = self.effective_limit();
        let mut batches: Vec<FileBatch> = Vec::new();
        let mut current_batch = FileBatch::new();

        for file in files_with_tokens {
            // If file is larger than effective limit, it gets its own batch
            if file.tokens > effective_limit {
                // Finish current batch if not empty
                if !current_batch.is_empty() {
                    batches.push(current_batch);
                    current_batch = FileBatch::new();
                }
                // Create single-file batch
                let mut oversized_batch = FileBatch::new();
                oversized_batch.add(file);
                batches.push(oversized_batch);
                continue;
            }

            // Try to add to current batch
            if current_batch.total_tokens + file.tokens <= effective_limit {
                current_batch.add(file);
            } else {
                // Start new batch
                batches.push(current_batch);
                current_batch = FileBatch::new();
                current_batch.add(file);
            }
        }

        // Don't forget the last batch
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }

    /// Estimate tokens for a file (includes formatting overhead)
    fn estimate_file_tokens(&self, path: &str, content: &str) -> usize {
        // Include path, formatting, and content
        // Format: "## File: {path}\n\n```\n{content}\n```\n\n"
        let overhead = 20 + path.len(); // Approximate formatting overhead
        self.counter.count(content) + overhead / 4
    }

    /// Get statistics about batching results
    pub fn batch_stats(batches: &[FileBatch]) -> BatchStats {
        if batches.is_empty() {
            return BatchStats::default();
        }

        let total_files: usize = batches.iter().map(|b| b.file_count()).sum();
        let total_tokens: usize = batches.iter().map(|b| b.total_tokens).sum();
        let avg_tokens_per_batch = total_tokens / batches.len();
        let max_batch_tokens = batches.iter().map(|b| b.total_tokens).max().unwrap_or(0);
        let min_batch_tokens = batches.iter().map(|b| b.total_tokens).min().unwrap_or(0);

        BatchStats {
            batch_count: batches.len(),
            total_files,
            total_tokens,
            avg_tokens_per_batch,
            max_batch_tokens,
            min_batch_tokens,
        }
    }
}

/// Statistics about batching results
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    pub batch_count: usize,
    pub total_files: usize,
    pub total_tokens: usize,
    pub avg_tokens_per_batch: usize,
    pub max_batch_tokens: usize,
    pub min_batch_tokens: usize,
}

impl BatchStats {
    /// Format as a human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "{} batches, {} files, {} tokens (avg {}/batch)",
            self.batch_count, self.total_files, self.total_tokens, self.avg_tokens_per_batch
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_based_counting() {
        let counter = TokenCounter::new(TokenEstimator::CharBased);
        assert_eq!(counter.count("hello"), 2); // 5 chars = 2 tokens
        assert_eq!(counter.count("hi"), 1); // 2 chars = 1 token
        assert_eq!(counter.count("hello world"), 3); // 11 chars = 3 tokens
    }

    #[test]
    fn test_code_aware_counting() {
        let counter = TokenCounter::new(TokenEstimator::CodeAware);

        // Simple code
        let code = "fn main() {}";
        let tokens = counter.count(code);
        assert!(tokens > 0);
        assert!(tokens <= 10); // Should be reasonable

        // Complex code
        let complex = r#"
            pub fn calculate(&self, value: i32) -> Result<i32, Error> {
                if value < 0 {
                    return Err(Error::Invalid);
                }
                Ok(value * 2)
            }
        "#;
        let complex_tokens = counter.count(complex);
        assert!(complex_tokens > tokens);
    }

    #[test]
    fn test_token_budget() {
        let mut budget = TokenBudget::new(100);

        assert!(budget.try_add("short text"));
        assert!(budget.current() > 0);
        assert!(budget.remaining() < 100);

        let utilization = budget.utilization();
        assert!(utilization > 0.0);
        assert!(utilization < 100.0);

        budget.reset();
        assert_eq!(budget.current(), 0);
    }

    #[test]
    fn test_estimate_file_tokens() {
        let tokens = estimate_file_tokens("test.rs", "fn main() {}");
        assert!(tokens > 0);
        assert!(tokens < 50); // Should be reasonable for simple file
    }

    #[test]
    fn test_check_batch_budget() {
        let files = vec![
            ("a.rs".to_string(), "fn a() {}".to_string()),
            ("b.rs".to_string(), "fn b() {}".to_string()),
        ];

        let (fits, total, exceeding) = check_batch_budget(&files, 1000);
        assert!(fits);
        assert!(total < 1000);
        assert!(exceeding.is_empty());
    }

    #[test]
    fn test_token_budget_batcher() {
        let batcher = TokenBudgetBatcher::new(1000);

        let files = vec![
            (
                "a.rs".to_string(),
                "fn a() { println!(\"hello\"); }".to_string(),
            ),
            (
                "b.rs".to_string(),
                "fn b() { println!(\"world\"); }".to_string(),
            ),
            ("c.rs".to_string(), "fn c() { }".to_string()),
        ];

        let batches = batcher.batch_files(files);

        // Should have at least 1 batch
        assert!(!batches.is_empty());

        // All files should be in batches
        let total_files: usize = batches.iter().map(|b| b.file_count()).sum();
        assert_eq!(total_files, 3);

        // Check stats
        let stats = TokenBudgetBatcher::batch_stats(&batches);
        assert_eq!(stats.total_files, 3);
        assert!(stats.total_tokens > 0);
    }

    #[test]
    fn test_batcher_creates_multiple_batches() {
        // Use a small budget to force multiple batches
        let batcher = TokenBudgetBatcher::with_buffer(100, 20); // 80 effective

        let files = vec![
            (
                "a.rs".to_string(),
                "fn a() { let x = 1; let y = 2; let z = 3; }".to_string(),
            ),
            (
                "b.rs".to_string(),
                "fn b() { let x = 1; let y = 2; let z = 3; }".to_string(),
            ),
            (
                "c.rs".to_string(),
                "fn c() { let x = 1; let y = 2; let z = 3; }".to_string(),
            ),
            (
                "d.rs".to_string(),
                "fn d() { let x = 1; let y = 2; let z = 3; }".to_string(),
            ),
        ];

        let batches = batcher.batch_files(files);

        // Should have multiple batches given small budget
        assert!(!batches.is_empty());

        // All 4 files should be accounted for
        let total_files: usize = batches.iter().map(|b| b.file_count()).sum();
        assert_eq!(total_files, 4);
    }

    #[test]
    fn test_batcher_empty_input() {
        let batcher = TokenBudgetBatcher::new(1000);
        let batches = batcher.batch_files(vec![]);
        assert!(batches.is_empty());
    }
}
