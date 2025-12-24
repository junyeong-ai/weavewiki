//! Database Layer with Connection Pooling and Safe Transactions
//!
//! Production-ready SQLite database layer featuring:
//! - Connection pooling via r2d2 for concurrent access
//! - Panic-safe transactions with automatic rollback
//! - Version-tracked migrations
//! - WAL mode for optimal read/write performance

use std::path::Path;
use std::sync::Arc;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, params};

use crate::types::{Edge, Node, Result, ResultExt, WeaveError, log_filter_error};

/// Shared database handle for async contexts.
pub type SharedDatabase = Arc<Database>;

const SCHEMA: &str = include_str!("schema.sql");

/// Current schema version for migration tracking
const SCHEMA_VERSION: u32 = 3;

/// Migration definitions
struct Migration {
    version: u32,
    description: &'static str,
    up: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Add checkpoint_data column",
        up: "ALTER TABLE doc_sessions ADD COLUMN checkpoint_data TEXT",
    },
    Migration {
        version: 2,
        description: "Add research context columns",
        up: "ALTER TABLE file_analysis ADD COLUMN research_iterations TEXT;
             ALTER TABLE file_analysis ADD COLUMN research_aspects TEXT",
    },
    Migration {
        version: 3,
        description: "Add WAL checkpoint settings",
        up: "PRAGMA wal_autocheckpoint = 1000",
    },
];

/// Generic agent insight for checkpoint storage
#[derive(Debug, Clone)]
pub struct AgentInsight {
    pub agent_name: String,
    pub turn: u8,
    pub insight_json: serde_json::Value,
    pub confidence: f32,
}

/// File analysis checkpoint data
#[derive(Debug, Clone)]
pub struct FileAnalysisCheckpoint {
    pub file_path: String,
    pub language: Option<String>,
    pub line_count: usize,
    pub complexity: String,
    pub purpose_summary: String,
    pub sections_json: String,
    pub key_insights_json: String,
    pub research_iterations_json: Option<String>,
    pub research_aspects_json: Option<String>,
}

/// Stored file insight for resume support
#[derive(Debug, Clone)]
pub struct StoredFileInsight {
    pub file_path: String,
    pub language: Option<String>,
    pub line_count: usize,
    pub complexity: String,
    pub purpose_summary: String,
    pub sections: serde_json::Value,
    pub key_insights: Vec<String>,
}

/// Type alias for file insight row data (path, language, line_count, complexity, purpose, sections, insights)
type FileInsightRow = (String, Option<String>, i64, String, String, String, String);

/// Connection pool configuration
///
/// Pool size is dynamically calculated based on CPU cores for optimal performance.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_size: u32,
    /// Minimum idle connections to keep ready
    pub min_idle: u32,
    /// Timeout for acquiring a connection (seconds)
    pub connection_timeout_secs: u64,
}

impl PoolConfig {
    /// Minimum pool size regardless of CPU count
    const MIN_POOL_SIZE: u32 = 4;
    /// Maximum pool size regardless of CPU count
    const MAX_POOL_SIZE: u32 = 32;
    /// Multiplier for CPU cores to pool size
    const POOL_SIZE_MULTIPLIER: f32 = 2.0;

    /// Calculate optimal pool size based on available CPU cores
    ///
    /// Formula: clamp(cores * 2, MIN, MAX)
    /// This provides 2 connections per core with sensible bounds.
    pub fn optimal_pool_size() -> u32 {
        let cores = std::thread::available_parallelism()
            .map(|p| p.get() as u32)
            .unwrap_or(4);

        let calculated = (cores as f32 * Self::POOL_SIZE_MULTIPLIER) as u32;
        calculated.clamp(Self::MIN_POOL_SIZE, Self::MAX_POOL_SIZE)
    }

    /// Create config with automatic pool sizing based on CPU cores
    pub fn auto() -> Self {
        let max_size = Self::optimal_pool_size();
        Self {
            max_size,
            min_idle: (max_size / 4).max(2),
            connection_timeout_secs: 30,
        }
    }

    /// Create config for high-load scenarios
    pub fn high_load() -> Self {
        let base = Self::optimal_pool_size();
        let max_size = (base * 2).min(Self::MAX_POOL_SIZE);
        Self {
            max_size,
            min_idle: base / 2,
            connection_timeout_secs: 60,
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self::auto()
    }
}

/// Thread-safe database with connection pooling.
///
/// Uses r2d2 connection pool for concurrent access with automatic
/// connection management and health checking.
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
}

impl Database {
    /// Open database with connection pooling at the specified path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_config(path, PoolConfig::default())
    }

    /// Open database with custom pool configuration.
    pub fn open_with_config<P: AsRef<Path>>(path: P, config: PoolConfig) -> Result<Self> {
        let manager =
            SqliteConnectionManager::file(path.as_ref()).with_init(Self::configure_connection);

        let pool = Pool::builder()
            .max_size(config.max_size)
            .min_idle(Some(config.min_idle))
            .connection_timeout(std::time::Duration::from_secs(
                config.connection_timeout_secs,
            ))
            .build(manager)
            .map_err(|e| WeaveError::Storage(format!("Failed to create connection pool: {}", e)))?;

        Ok(Self { pool })
    }

    /// Open an in-memory database for testing or temporary use.
    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .map_err(|e| WeaveError::Storage(format!("Failed to create in-memory pool: {}", e)))?;

        Ok(Self { pool })
    }

    /// Configure a new connection with production-ready settings.
    fn configure_connection(conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA cache_size = -64000;
            PRAGMA busy_timeout = 5000;
            PRAGMA wal_autocheckpoint = 1000;
            "#,
        )?;
        Ok(())
    }

    /// Get a connection from the pool.
    fn conn(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|e| {
            WeaveError::Storage(format!("Failed to acquire database connection: {}", e))
        })
    }

    /// Initialize database schema.
    pub fn initialize(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(SCHEMA)
            .with_context("Failed to initialize database schema")?;

        // Set version to current since schema.sql includes all columns
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .with_context("Failed to set schema version")?;

        drop(conn);
        // Migrations only needed for existing databases with older versions
        self.migrate()?;
        Ok(())
    }

    /// Run version-tracked migrations.
    fn migrate(&self) -> Result<()> {
        let conn = self.conn()?;

        let current_version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap_or(0);

        for migration in MIGRATIONS {
            if migration.version > current_version {
                // Execute migration in a transaction
                conn.execute_batch(migration.up).with_context_fn(|| {
                    format!(
                        "Failed to apply migration {}: {}",
                        migration.version, migration.description
                    )
                })?;

                tracing::info!(
                    "Applied migration {}: {}",
                    migration.version,
                    migration.description
                );
            }
        }

        // Update schema version
        if current_version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)
                .with_context("Failed to update schema version")?;
        }

        Ok(())
    }

    /// Get a raw connection for advanced operations.
    pub fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.conn()
    }

    /// Execute a single SQL statement.
    pub fn execute(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<usize> {
        let conn = self.conn()?;
        conn.execute(sql, params)
            .with_context("Failed to execute SQL")
    }

    /// Update session progress atomically using type-safe builder.
    pub fn update_session_progress(
        &self,
        session_id: &str,
        total_files: Option<usize>,
        files_analyzed: Option<usize>,
        current_phase: Option<u8>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339();

        // Build query dynamically but safely
        let mut set_clauses = vec!["last_checkpoint_at = ?1"];
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(total) = total_files {
            set_clauses.push("total_files = ?");
            param_values.push(Box::new(total as i32));
        }

        if let Some(analyzed) = files_analyzed {
            set_clauses.push("files_analyzed = ?");
            param_values.push(Box::new(analyzed as i32));
        }

        if let Some(phase) = current_phase {
            set_clauses.push("current_phase = ?");
            param_values.push(Box::new(phase as i32));
        }

        param_values.push(Box::new(session_id.to_string()));

        // Build query with correct parameter indices
        let mut query = String::from("UPDATE doc_sessions SET ");
        for (i, clause) in set_clauses.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }
            // Replace ? with actual parameter index
            if i == 0 {
                query.push_str(clause);
            } else {
                query.push_str(&clause.replace("?", &format!("?{}", i + 1)));
            }
        }
        query.push_str(&format!(" WHERE id = ?{}", param_values.len()));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        conn.execute(&query, params_refs.as_slice())
            .with_context("Failed to update session progress")?;

        Ok(())
    }

    /// Execute a function within a panic-safe database transaction.
    ///
    /// All operations within the closure are atomic. If the closure panics,
    /// the transaction is automatically rolled back and an error is returned
    /// instead of poisoning the connection pool.
    pub fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + std::panic::UnwindSafe,
    {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction()
            .with_context("Failed to start transaction")?;

        // Use catch_unwind for panic safety
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx)));

        match result {
            Ok(Ok(value)) => {
                tx.commit().with_context("Failed to commit transaction")?;
                Ok(value)
            }
            Ok(Err(e)) => {
                // Transaction will be rolled back on drop
                Err(e)
            }
            Err(panic_payload) => {
                // Transaction will be rolled back on drop
                let panic_msg = panic_payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "Unknown panic".to_string());

                tracing::error!("Transaction panicked: {}", panic_msg);
                Err(WeaveError::Storage(format!(
                    "Transaction panicked: {}",
                    panic_msg
                )))
            }
        }
    }

    // =========================================================================
    // Generic Checkpoint Storage
    // =========================================================================

    /// Store an agent insight for resume support.
    pub fn store_agent_insight(&self, session_id: &str, insight: &AgentInsight) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let insight_str = serde_json::to_string(&insight.insight_json)
            .with_context("Failed to serialize insight JSON")?;
        let now = chrono::Utc::now().to_rfc3339();

        self.conn()?
            .execute(
                "INSERT INTO characterization_insights
             (id, session_id, agent_name, turn_number, insight_json, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    session_id,
                    insight.agent_name,
                    insight.turn as i32,
                    insight_str,
                    insight.confidence as f64,
                    now,
                ],
            )
            .with_context("Failed to store agent insight")?;

        tracing::debug!(
            "Stored agent insight: agent={}, turn={}",
            insight.agent_name,
            insight.turn
        );

        Ok(())
    }

    /// Load all agent insights for a session.
    pub fn load_agent_insights(&self, session_id: &str) -> Result<Vec<AgentInsight>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT agent_name, turn_number, insight_json, confidence
             FROM characterization_insights
             WHERE session_id = ?1
             ORDER BY turn_number, agent_name",
            )
            .with_context("Failed to prepare agent insights query")?;

        let rows: Vec<_> = stmt
            .query_map(params![session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context("Failed to fetch agent insights")?;

        let mut outputs = Vec::with_capacity(rows.len());
        for (agent_name, turn, insight_str, confidence) in rows {
            let insight_json: serde_json::Value = serde_json::from_str(&insight_str)
                .with_context_fn(|| {
                    format!(
                        "Corrupted insight JSON for agent '{}' turn {}",
                        agent_name, turn
                    )
                })?;

            outputs.push(AgentInsight {
                agent_name,
                turn: turn as u8,
                insight_json,
                confidence: confidence as f32,
            });
        }

        Ok(outputs)
    }

    /// Check which agents have completed for a session.
    pub fn get_completed_agents(&self, session_id: &str) -> Result<Vec<(String, u8)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT agent_name, turn_number
             FROM characterization_insights
             WHERE session_id = ?1",
        )?;

        let agents = stmt
            .query_map(params![session_id], |row| {
                let name: String = row.get(0)?;
                let turn: i32 = row.get(1)?;
                Ok((name, turn as u8))
            })?
            .filter_map(|r| log_filter_error(r, "loading completed agent"))
            .collect();

        Ok(agents)
    }

    /// Store JSON data for a session profile.
    pub fn store_session_profile(
        &self,
        session_id: &str,
        profile: &serde_json::Value,
    ) -> Result<()> {
        let profile_json =
            serde_json::to_string(profile).with_context("Failed to serialize session profile")?;
        let now = chrono::Utc::now().to_rfc3339();

        self.conn()?
            .execute(
                "UPDATE doc_sessions
             SET project_profile = ?1, updated_at = ?2
             WHERE id = ?3",
                params![profile_json, now, session_id],
            )
            .with_context("Failed to store session profile")?;

        tracing::debug!("Stored session profile");
        Ok(())
    }

    /// Load JSON profile data for a session.
    pub fn load_session_profile(&self, session_id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT project_profile FROM doc_sessions WHERE id = ?1")?;

        let result: std::result::Result<Option<String>, _> =
            stmt.query_row(params![session_id], |row| row.get(0));

        match result {
            Ok(Some(profile_str)) => {
                let value = serde_json::from_str(&profile_str)
                    .with_context_fn(|| format!("Corrupted session profile for {}", session_id))?;
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WeaveError::Storage(format!(
                "Failed to load profile: {}",
                e
            ))),
        }
    }

    /// Clear agent insights for a session.
    pub fn clear_agent_insights(&self, session_id: &str) -> Result<()> {
        self.conn()?.execute(
            "DELETE FROM characterization_insights WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    // =========================================================================
    // File-Level Checkpoint Operations
    // =========================================================================

    /// Atomic checkpoint for a single file analysis with graph nodes.
    ///
    /// Uses prepared statements with batch execution for optimal performance.
    /// This eliminates the N+1 query problem by preparing statements once
    /// and reusing them for all nodes/edges.
    pub fn checkpoint_file_analysis(
        &self,
        session_id: &str,
        checkpoint: &FileAnalysisCheckpoint,
        graph_nodes: &[Node],
        graph_edges: &[Edge],
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();

        self.transaction(|conn| {
            // 1. Insert/update file_analysis
            conn.execute(
                r#"INSERT OR REPLACE INTO file_analysis
                   (id, session_id, file_path, language, line_count, complexity,
                    purpose_summary, sections, key_insights, research_iterations,
                    research_aspects, analyzed_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
                params![
                    id,
                    session_id,
                    checkpoint.file_path,
                    checkpoint.language,
                    checkpoint.line_count as i64,
                    checkpoint.complexity,
                    checkpoint.purpose_summary,
                    checkpoint.sections_json,
                    checkpoint.key_insights_json,
                    checkpoint.research_iterations_json,
                    checkpoint.research_aspects_json,
                    now,
                ],
            ).with_context("Failed to insert file analysis")?;

            // 2. Batch insert graph nodes using prepared statement
            if !graph_nodes.is_empty() {
                let mut node_stmt = conn.prepare_cached(
                    r#"INSERT INTO nodes (id, node_type, path, name, metadata, evidence, tier, confidence, last_verified, status)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                       ON CONFLICT(id) DO UPDATE SET
                           metadata = excluded.metadata,
                           evidence = excluded.evidence,
                           confidence = excluded.confidence,
                           last_verified = excluded.last_verified,
                           updated_at = CURRENT_TIMESTAMP"#,
                ).with_context("Failed to prepare node statement")?;

                for node in graph_nodes {
                    let metadata = serde_json::to_string(&node.metadata)
                        .with_context("Failed to serialize node metadata")?;
                    let evidence = serde_json::to_string(&node.evidence)
                        .with_context("Failed to serialize node evidence")?;

                    node_stmt.execute(params![
                        node.id,
                        crate::types::enum_to_str(&node.node_type),
                        node.path,
                        node.name,
                        metadata,
                        evidence,
                        crate::types::enum_to_str(&node.tier),
                        node.confidence,
                        node.last_verified.to_rfc3339(),
                        crate::types::enum_to_str(&node.status),
                    ]).with_context("Failed to insert node")?;
                }
            }

            // 3. Batch insert graph edges using prepared statement
            if !graph_edges.is_empty() {
                let mut edge_stmt = conn.prepare_cached(
                    r#"INSERT INTO edges (id, edge_type, source_id, target_id, metadata, evidence, tier, confidence, last_verified)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                       ON CONFLICT(edge_type, source_id, target_id) DO UPDATE SET
                           metadata = excluded.metadata,
                           evidence = excluded.evidence,
                           confidence = excluded.confidence,
                           last_verified = excluded.last_verified"#,
                ).with_context("Failed to prepare edge statement")?;

                for edge in graph_edges {
                    let metadata = serde_json::to_string(&edge.metadata)
                        .with_context("Failed to serialize edge metadata")?;
                    let evidence = serde_json::to_string(&edge.evidence)
                        .with_context("Failed to serialize edge evidence")?;

                    edge_stmt.execute(params![
                        edge.id,
                        crate::types::enum_to_str(&edge.edge_type),
                        edge.source_id,
                        edge.target_id,
                        metadata,
                        evidence,
                        crate::types::enum_to_str(&edge.tier),
                        edge.confidence,
                        edge.last_verified.to_rfc3339(),
                    ]).with_context("Failed to insert edge")?;
                }
            }

            // 4. Update file_tracking status
            conn.execute(
                "UPDATE file_tracking SET status = 'analyzed', analyzed_at = ?1
                 WHERE session_id = ?2 AND file_path = ?3",
                params![now, session_id, checkpoint.file_path],
            ).with_context("Failed to update file tracking")?;

            // 5. Increment files_analyzed counter
            conn.execute(
                "UPDATE doc_sessions SET files_analyzed = files_analyzed + 1,
                 last_checkpoint_at = ?1 WHERE id = ?2",
                params![now, session_id],
            ).with_context("Failed to update session progress")?;

            Ok(())
        })
    }

    /// Get pending files with pagination support.
    pub fn get_pending_files_paginated(
        &self,
        session_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>> {
        let conn = self.conn()?;

        // Use cursor-based pagination for better performance with large datasets
        let mut stmt = if limit > 0 {
            conn.prepare(
                "SELECT file_path FROM file_tracking
                 WHERE session_id = ?1 AND status IN ('discovered', 'analyzing')
                 ORDER BY file_path
                 LIMIT ?2 OFFSET ?3",
            )?
        } else {
            conn.prepare(
                "SELECT file_path FROM file_tracking
                 WHERE session_id = ?1 AND status IN ('discovered', 'analyzing')
                 ORDER BY file_path",
            )?
        };

        let files = if limit > 0 {
            stmt.query_map(params![session_id, limit as i64, offset as i64], |row| {
                row.get(0)
            })?
            .filter_map(|r| log_filter_error(r, "reading pending file"))
            .collect()
        } else {
            stmt.query_map(params![session_id], |row| row.get(0))?
                .filter_map(|r| log_filter_error(r, "reading pending file"))
                .collect()
        };

        Ok(files)
    }

    /// Get all pending files.
    pub fn get_pending_files(&self, session_id: &str) -> Result<Vec<String>> {
        self.get_pending_files_paginated(session_id, 0, 0)
    }

    /// Get already analyzed file insights with pagination.
    pub fn load_analyzed_files_paginated(
        &self,
        session_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<StoredFileInsight>> {
        let conn = self.conn()?;

        let mut stmt = if limit > 0 {
            conn.prepare(
                "SELECT file_path, language, line_count, complexity, purpose_summary, sections, key_insights
                 FROM file_analysis
                 WHERE session_id = ?1
                 ORDER BY file_path
                 LIMIT ?2 OFFSET ?3",
            )?
        } else {
            conn.prepare(
                "SELECT file_path, language, line_count, complexity, purpose_summary, sections, key_insights
                 FROM file_analysis
                 WHERE session_id = ?1
                 ORDER BY file_path",
            )?
        };

        let rows: Vec<_> = if limit > 0 {
            stmt.query_map(
                params![session_id, limit as i64, offset as i64],
                Self::map_file_insight_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![session_id], Self::map_file_insight_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut insights = Vec::with_capacity(rows.len());
        for (
            file_path,
            language,
            line_count,
            complexity,
            purpose_summary,
            sections_str,
            key_insights_str,
        ) in rows
        {
            let sections: serde_json::Value = serde_json::from_str(&sections_str)
                .with_context_fn(|| format!("Corrupted sections JSON for file '{}'", file_path))?;

            let key_insights: Vec<String> = serde_json::from_str(&key_insights_str)
                .with_context_fn(|| {
                    format!("Corrupted key_insights JSON for file '{}'", file_path)
                })?;

            insights.push(StoredFileInsight {
                file_path,
                language,
                line_count: line_count as usize,
                complexity,
                purpose_summary,
                sections,
                key_insights,
            });
        }

        Ok(insights)
    }

    fn map_file_insight_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileInsightRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    }

    /// Get all analyzed file insights.
    pub fn load_analyzed_files(&self, session_id: &str) -> Result<Vec<StoredFileInsight>> {
        self.load_analyzed_files_paginated(session_id, 0, 0)
    }

    /// Get count of analyzed files for a session.
    pub fn count_analyzed_files(&self, session_id: &str) -> Result<usize> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_analysis WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .with_context("Failed to count analyzed files")?;
        Ok(count as usize)
    }

    /// Get count of pending files for a session.
    pub fn count_pending_files(&self, session_id: &str) -> Result<usize> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_tracking
             WHERE session_id = ?1 AND status IN ('discovered', 'analyzing')",
                params![session_id],
                |row| row.get(0),
            )
            .with_context("Failed to count pending files")?;
        Ok(count as usize)
    }

    /// Mark a file as analyzing (in progress).
    pub fn mark_file_analyzing(&self, session_id: &str, file_path: &str) -> Result<()> {
        self.conn()?
            .execute(
                "UPDATE file_tracking SET status = 'analyzing'
             WHERE session_id = ?1 AND file_path = ?2",
                params![session_id, file_path],
            )
            .with_context("Failed to mark file as analyzing")?;
        Ok(())
    }

    /// Mark a file as failed with error message.
    pub fn mark_file_failed(&self, session_id: &str, file_path: &str, error: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn()?
            .execute(
                "UPDATE file_tracking SET status = 'failed', error_message = ?1, analyzed_at = ?2,
             retry_count = retry_count + 1
             WHERE session_id = ?3 AND file_path = ?4",
                params![error, now, session_id, file_path],
            )
            .with_context("Failed to mark file as failed")?;
        Ok(())
    }

    /// Get analysis progress for a session.
    pub fn get_analysis_progress(&self, session_id: &str) -> Result<(usize, usize, usize)> {
        let conn = self.conn()?;

        let (total, analyzed, failed): (i64, i64, i64) = conn
            .query_row(
                r#"SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'analyzed' THEN 1 ELSE 0 END) as analyzed,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed
               FROM file_tracking
               WHERE session_id = ?1"#,
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .with_context("Failed to get analysis progress")?;

        Ok((total as usize, analyzed as usize, failed as usize))
    }

    // =========================================================================
    // Knowledge Graph Query Operations
    // =========================================================================

    /// Get structural facts for a file (parser-extracted nodes).
    pub fn get_file_structural_nodes(&self, file_path: &str) -> Result<Vec<Node>> {
        use crate::types::ParseWithDefault;
        use crate::types::node::*;

        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"SELECT id, node_type, path, name, metadata, evidence, tier, confidence, last_verified, status
               FROM nodes
               WHERE path = ?1 AND tier = 'fact'
               ORDER BY node_type, name"#,
        )?;

        let rows: Vec<_> = stmt
            .query_map(params![file_path], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, f32>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut nodes = Vec::with_capacity(rows.len());
        for (
            id,
            node_type_str,
            path,
            name,
            metadata_str,
            evidence_str,
            tier_str,
            confidence,
            last_verified_str,
            status_str,
        ) in rows
        {
            let node_type = NodeType::parse_or_default(&node_type_str);
            let tier = InformationTier::parse_or_default(&tier_str);
            let status = NodeStatus::parse_or_default(&status_str);

            let metadata: NodeMetadata = serde_json::from_str(&metadata_str)
                .with_context_fn(|| format!("Invalid node metadata for {}", id))?;
            let evidence: EvidenceLocation = serde_json::from_str(&evidence_str)
                .with_context_fn(|| format!("Invalid node evidence for {}", id))?;

            let last_verified = chrono::DateTime::parse_from_rfc3339(&last_verified_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            nodes.push(Node {
                id,
                node_type,
                path,
                name,
                metadata,
                evidence,
                tier,
                confidence,
                last_verified,
                status,
            });
        }

        Ok(nodes)
    }

    /// Get internal dependencies for a file.
    pub fn get_file_dependencies(&self, file_path: &str) -> Result<Vec<(String, String)>> {
        let file_id = format!("file:{}", file_path);
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"SELECT target_id, edge_type FROM edges
               WHERE source_id = ?1 AND tier = 'fact'
               ORDER BY target_id"#,
        )?;

        let deps = stmt
            .query_map(params![file_id], |row| {
                let target: String = row.get(0)?;
                let edge_type: String = row.get(1)?;
                Ok((target, edge_type))
            })?
            .filter_map(|r| log_filter_error(r, "reading file dependency"))
            .collect();

        Ok(deps)
    }

    // =========================================================================
    // Checkpoint State Loading
    // =========================================================================

    /// Load checkpoint state directly from database tables.
    pub fn load_checkpoint_state(&self, session_id: &str) -> Result<CheckpointState> {
        let conn = self.conn()?;

        let (current_phase, project_profile): (i32, Option<String>) = conn
            .query_row(
                "SELECT current_phase, project_profile FROM doc_sessions WHERE id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| WeaveError::Session(format!("Session not found: {}", session_id)))?;

        let mut files_stmt = conn.prepare(
            "SELECT file_path FROM file_tracking WHERE session_id = ?1 ORDER BY file_path",
        )?;
        let files: Vec<String> = files_stmt
            .query_map(params![session_id], |row| row.get(0))?
            .filter_map(|r| log_filter_error(r, "loading tracked file"))
            .collect();

        let analyzed_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM file_analysis WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        let char_complete: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT agent_name) FROM characterization_insights WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        let topdown_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM module_summaries WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        let domain_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM domain_summaries WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        // Determine completed phase from data
        let completed_phase = if domain_count > 0 {
            4
        } else if topdown_count > 0 {
            3
        } else if analyzed_count > 0 && analyzed_count >= files.len() as i64 {
            2
        } else if char_complete >= 7 {
            1
        } else {
            0
        };

        Ok(CheckpointState {
            session_id: session_id.to_string(),
            last_completed_phase: completed_phase.max(current_phase as u8 - 1),
            total_files: files.len(),
            analyzed_files: analyzed_count as usize,
            has_project_profile: project_profile.is_some(),
            has_file_insights: analyzed_count > 0,
            has_top_down_insights: topdown_count > 0,
            has_domain_insights: domain_count > 0,
            files,
        })
    }

    /// Get the last checkpoint timestamp.
    pub fn get_last_checkpoint_time(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let result: std::result::Result<Option<String>, _> = conn.query_row(
            "SELECT last_checkpoint_at FROM doc_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        );
        Ok(result.unwrap_or(None))
    }
}

/// Checkpoint state loaded from database tables.
#[derive(Debug, Clone)]
pub struct CheckpointState {
    pub session_id: String,
    pub last_completed_phase: u8,
    pub total_files: usize,
    pub analyzed_files: usize,
    pub has_project_profile: bool,
    pub has_file_insights: bool,
    pub has_top_down_insights: bool,
    pub has_domain_insights: bool,
    pub files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let conn = db.connection().expect("Failed to get connection");
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"doc_sessions".to_string()));
    }

    #[test]
    fn test_transaction_panic_safety() {
        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");

        // This should not poison the connection pool
        let result = db.transaction(|_conn| {
            panic!("Intentional panic for testing");
            #[allow(unreachable_code)]
            Ok(())
        });

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("panicked"));

        // Database should still be usable
        let conn = db.connection();
        assert!(
            conn.is_ok(),
            "Database should still be accessible after panic"
        );
    }

    #[test]
    fn test_execute() {
        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let affected = db
            .execute(
                "INSERT INTO doc_sessions (id, project_path, status, started_at) VALUES (?1, ?2, ?3, ?4)",
                &[
                    &"test-session".to_string(),
                    &"/test/project".to_string(),
                    &"running",
                    &"2025-01-01T00:00:00Z",
                ],
            )
            .expect("Failed to insert");

        assert_eq!(affected, 1);
    }

    #[test]
    fn test_agent_insights_roundtrip() {
        use serde_json::json;

        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let session_id = "test-session-1";
        db.execute(
            "INSERT INTO doc_sessions (id, project_path, current_phase, status, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &session_id.to_string(),
                &"/test/project".to_string(),
                &0i32,
                &"running",
                &"2025-01-01T00:00:00Z",
            ],
        )
        .expect("Failed to create session");

        let insight1 = AgentInsight {
            agent_name: "structure".to_string(),
            turn: 1,
            insight_json: json!({
                "directory_patterns": ["src/lib pattern"],
                "module_boundaries": [],
                "organization_style": "layered"
            }),
            confidence: 0.85,
        };

        let insight2 = AgentInsight {
            agent_name: "purpose".to_string(),
            turn: 2,
            insight_json: json!({
                "purposes": ["CLI tool"],
                "target_users": ["developers"]
            }),
            confidence: 0.9,
        };

        db.store_agent_insight(session_id, &insight1)
            .expect("Failed to store insight 1");
        db.store_agent_insight(session_id, &insight2)
            .expect("Failed to store insight 2");

        let loaded = db
            .load_agent_insights(session_id)
            .expect("Failed to load insights");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].agent_name, "structure");
        assert_eq!(loaded[0].turn, 1);
        assert_eq!(loaded[1].agent_name, "purpose");
        assert_eq!(loaded[1].turn, 2);

        let completed = db
            .get_completed_agents(session_id)
            .expect("Failed to get completed agents");
        assert_eq!(completed.len(), 2);

        db.clear_agent_insights(session_id)
            .expect("Failed to clear");
        let after_clear = db
            .load_agent_insights(session_id)
            .expect("Failed to load after clear");
        assert!(after_clear.is_empty());
    }

    #[test]
    fn test_project_profile_roundtrip() {
        use serde_json::json;

        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let session_id = "test-session-2";
        db.execute(
            "INSERT INTO doc_sessions (id, project_path, current_phase, status, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &session_id.to_string(),
                &"/test/project".to_string(),
                &0i32,
                &"running",
                &"2025-01-01T00:00:00Z",
            ],
        )
        .expect("Failed to create session");

        let profile = json!({
            "name": "test-project",
            "scale": "medium",
            "purposes": ["CLI tool", "Code analyzer"],
            "technical_traits": ["Async", "Multi-threaded"]
        });

        db.store_session_profile(session_id, &profile)
            .expect("Failed to store profile");

        let loaded = db
            .load_session_profile(session_id)
            .expect("Failed to load profile");

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded["name"], "test-project");
        assert_eq!(loaded["purposes"][0], "CLI tool");
    }

    #[test]
    fn test_file_checkpoint_and_resume() {
        use serde_json::json;

        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");

        let session_id = "test-session-checkpoint";

        db.execute(
            "INSERT INTO doc_sessions (id, project_path, status, started_at, files_analyzed)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &session_id.to_string(),
                &"/test".to_string(),
                &"running",
                &"2025-01-01T00:00:00Z",
                &0i32,
            ],
        )
        .expect("Failed to create session");

        let now = chrono::Utc::now().to_rfc3339();
        for file in &["src/main.rs", "src/lib.rs", "src/utils.rs"] {
            db.execute(
                "INSERT INTO file_tracking (file_path, session_id, content_hash, line_count, status, discovered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &[
                    &file.to_string(),
                    &session_id.to_string(),
                    &"hash123".to_string(),
                    &100i64,
                    &"discovered",
                    &now,
                ],
            )
            .expect("Failed to create file tracking");
        }

        let pending = db
            .get_pending_files(session_id)
            .expect("Failed to get pending");
        assert_eq!(pending.len(), 3);

        let checkpoint = FileAnalysisCheckpoint {
            file_path: "src/main.rs".to_string(),
            language: Some("Rust".to_string()),
            line_count: 100,
            complexity: "medium".to_string(),
            purpose_summary: "Entry point".to_string(),
            sections_json: json!({"hidden_assumptions": []}).to_string(),
            key_insights_json: json!(["Main entry point"]).to_string(),
            research_iterations_json: None,
            research_aspects_json: None,
        };

        db.checkpoint_file_analysis(session_id, &checkpoint, &[], &[])
            .expect("Failed to checkpoint");

        let pending = db
            .get_pending_files(session_id)
            .expect("Failed to get pending");
        assert_eq!(pending.len(), 2);

        let analyzed = db
            .load_analyzed_files(session_id)
            .expect("Failed to load analyzed");
        assert_eq!(analyzed.len(), 1);
        assert_eq!(analyzed[0].file_path, "src/main.rs");

        let (total, done, failed) = db.get_analysis_progress(session_id).expect("Failed");
        assert_eq!(total, 3);
        assert_eq!(done, 1);
        assert_eq!(failed, 0);
    }

    #[test]
    fn test_pool_config_optimal_sizing() {
        // Optimal pool size should be within bounds
        let size = PoolConfig::optimal_pool_size();
        assert!(size >= PoolConfig::MIN_POOL_SIZE);
        assert!(size <= PoolConfig::MAX_POOL_SIZE);

        // Auto config should use optimal sizing
        let auto = PoolConfig::auto();
        assert_eq!(auto.max_size, size);
        assert!(auto.min_idle >= 2);
        assert!(auto.min_idle <= auto.max_size);

        // High load should have larger pool
        let high = PoolConfig::high_load();
        assert!(high.max_size >= auto.max_size);
        assert!(high.min_idle > 0);
    }
}
