//! Global Constants
//!
//! Centralized constants for configuration and tuning.
//! All magic numbers should be defined here with documentation.

/// Verification engine constants
pub mod verification {
    /// Files modified within this many seconds are considered "recently modified"
    pub const STALE_FILE_THRESHOLD_SECS: u64 = 60;

    /// Maximum number of claims to verify in one batch
    pub const MAX_CLAIMS_PER_BATCH: usize = 1000;

    /// Default cache size for file content
    pub const DEFAULT_FILE_CACHE_SIZE: usize = 100;
}

/// Provider chain constants
pub mod chain {
    /// Maximum total attempts across all providers
    pub const MAX_TOTAL_ATTEMPTS: usize = 10;

    /// Default maximum retries per provider
    pub const DEFAULT_MAX_RETRIES: u8 = 3;

    /// Base delay for exponential backoff (milliseconds)
    pub const BASE_DELAY_MS: u64 = 500;

    /// Maximum delay between retries (seconds)
    pub const MAX_DELAY_SECS: u64 = 30;

    /// Backoff multiplier
    pub const BACKOFF_FACTOR: f32 = 2.0;
}

/// Circuit breaker constants
pub mod circuit_breaker {
    /// Number of failures before opening circuit
    pub const FAILURE_THRESHOLD: u32 = 5;

    /// Duration to wait before attempting recovery (seconds)
    pub const RECOVERY_TIMEOUT_SECS: u64 = 30;

    /// Maximum requests allowed in half-open state
    pub const HALF_OPEN_MAX_REQUESTS: u32 = 3;

    /// Success threshold to close circuit from half-open
    pub const SUCCESS_THRESHOLD: u32 = 2;
}

/// Token budget constants
pub mod budget {
    /// Default total token budget
    pub const DEFAULT_BUDGET: u64 = 1_000_000;

    /// Warning threshold (percentage of budget)
    pub const WARNING_THRESHOLD: f64 = 0.75;

    /// Critical threshold (percentage of budget)
    pub const CRITICAL_THRESHOLD: f64 = 0.90;

    /// Reserve buffer percentage for retries and repairs
    pub const RESERVE_BUFFER_PCT: f64 = 0.05;

    /// Phase allocations (percentages, must sum to 100)
    ///
    /// These are the single source of truth for phase budget distribution.
    pub mod phase {
        /// Characterization phase: lightweight project profiling
        pub const CHARACTERIZATION_PCT: u8 = 5;
        /// Bottom-up analysis: bulk of work (per-file analysis)
        pub const BOTTOM_UP_PCT: u8 = 50;
        /// Top-down analysis: project-level insights
        pub const TOP_DOWN_PCT: u8 = 10;
        /// Consolidation: domain synthesis
        pub const CONSOLIDATION_PCT: u8 = 20;
        /// Refinement: quality improvement passes
        pub const REFINEMENT_PCT: u8 = 15;
    }
}

/// Pipeline constants
pub mod pipeline {
    /// Default processing tier batch sizes
    pub mod batch_size {
        pub const LEAF: usize = 50;
        pub const STANDARD: usize = 30;
        pub const IMPORTANT: usize = 15;
        pub const CORE: usize = 5;
    }

    /// Maximum child context tokens for core files
    pub const MAX_CHILD_CONTEXT_TOKENS: usize = 2000;

    /// Number of characterization turns
    pub const CHARACTERIZATION_TURNS: u8 = 3;

    /// Maximum refinement turns
    pub const MAX_REFINEMENT_TURNS: usize = 5;

    /// Default quality target for refinement
    pub const DEFAULT_QUALITY_TARGET: f32 = 0.7;
}

/// File analysis constants
pub mod analysis {
    /// Maximum file size to analyze (5MB)
    pub const MAX_FILE_SIZE: usize = 5 * 1024 * 1024;

    /// Maximum lines per file for analysis
    pub const MAX_LINES: usize = 10_000;

    /// Minimum file size to consider for analysis (bytes)
    pub const MIN_FILE_SIZE: usize = 10;
}

/// Cache constants
pub mod cache {
    /// Maximum entries in wiki cache
    pub const MAX_WIKI_CACHE_ENTRIES: usize = 100;

    /// Cache entry expiration (hours)
    pub const CACHE_EXPIRATION_HOURS: u64 = 24;

    /// Maximum size of cached content (bytes)
    pub const MAX_CACHED_CONTENT_SIZE: usize = 1024 * 1024;
}

/// HTTP/Network constants
pub mod network {
    /// Default request timeout (seconds)
    pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

    /// Connection timeout (seconds)
    pub const CONNECTION_TIMEOUT_SECS: u64 = 30;

    /// Maximum retries for network requests
    pub const MAX_NETWORK_RETRIES: u32 = 3;
}
