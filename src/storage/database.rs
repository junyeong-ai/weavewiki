//! Database Layer with Connection Pooling
//!
//! Production-ready SQLite database layer featuring:
//! - Connection pooling via r2d2 for concurrent access
//! - Panic-safe transactions with automatic rollback
//! - WAL mode for optimal read/write performance

use std::path::Path;
use std::sync::Arc;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, params};

use crate::types::{ClaudegenError, Result, ResultExt};

pub type SharedDatabase = Arc<Database>;

const SCHEMA: &str = include_str!("schema.sql");

// Database pool configuration constants
const MIN_POOL_SIZE: u32 = 4;
const MAX_POOL_SIZE: u32 = 32;
const POOL_SIZE_MULTIPLIER: f32 = 2.0;
const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_size: u32,
    pub min_idle: u32,
    pub connection_timeout_secs: u64,
}

impl PoolConfig {
    pub fn optimal_pool_size() -> u32 {
        let cores = std::thread::available_parallelism()
            .map(|p| p.get() as u32)
            .unwrap_or(MIN_POOL_SIZE);

        let calculated = (cores as f32 * POOL_SIZE_MULTIPLIER) as u32;
        calculated.clamp(MIN_POOL_SIZE, MAX_POOL_SIZE)
    }

    pub fn auto() -> Self {
        let max_size = Self::optimal_pool_size();
        Self {
            max_size,
            min_idle: (max_size / 4).max(2),
            connection_timeout_secs: DEFAULT_CONNECTION_TIMEOUT_SECS,
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self::auto()
    }
}

pub struct Database {
    pool: Pool<SqliteConnectionManager>,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_config(path, PoolConfig::default())
    }

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
            .map_err(|e| {
                ClaudegenError::Storage(format!("Failed to create connection pool: {e}"))
            })?;

        Ok(Self { pool })
    }

    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;
            Ok(())
        });

        let pool = Pool::builder().max_size(1).build(manager).map_err(|e| {
            ClaudegenError::Storage(format!("Failed to create in-memory pool: {e}"))
        })?;

        Ok(Self { pool })
    }

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

    fn conn(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|e| {
            ClaudegenError::Storage(format!("Failed to acquire database connection: {e}"))
        })
    }

    pub fn initialize(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(SCHEMA)
            .with_context("Failed to initialize database schema")?;
        Ok(())
    }

    pub fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.conn()
    }

    pub fn execute(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<usize> {
        let conn = self.conn()?;
        conn.execute(sql, params)
            .with_context("Failed to execute SQL")
    }

    pub fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + std::panic::UnwindSafe,
    {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction()
            .with_context("Failed to start transaction")?;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx)));

        match result {
            Ok(Ok(value)) => {
                tx.commit().with_context("Failed to commit transaction")?;
                Ok(value)
            }
            Ok(Err(e)) => Err(e),
            Err(panic_payload) => {
                let panic_msg = panic_payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "Unknown panic".to_string());

                tracing::error!("Transaction panicked: {}", panic_msg);
                Err(ClaudegenError::Storage(format!(
                    "Transaction panicked: {panic_msg}"
                )))
            }
        }
    }

    // =========================================================================
    // Session Management
    // =========================================================================

    pub fn create_session(&self, project_path: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        self.conn()?.execute(
            "INSERT INTO sessions (id, project_path, status, started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, project_path, "running", now, now],
        )?;

        Ok(id)
    }

    pub fn update_session_status(&self, session_id: &str, status: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let completed_at = if status == "completed" || status == "failed" {
            Some(now.clone())
        } else {
            None
        };

        self.conn()?.execute(
            "UPDATE sessions SET status = ?1, completed_at = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, completed_at, now, session_id],
        )?;

        Ok(())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_path, status, started_at, completed_at FROM sessions WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                project_path: row.get(1)?,
                status: row.get(2)?,
                started_at: row.get(3)?,
                completed_at: row.get(4)?,
            })
        });

        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ClaudegenError::Storage(format!(
                "Failed to get session: {e}"
            ))),
        }
    }

    // =========================================================================
    // LLM Metrics
    // =========================================================================

    pub fn record_llm_call(
        &self,
        session_id: &str,
        model: &str,
        provider: &str,
        input_tokens: u32,
        output_tokens: u32,
        status: &str,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();

        self.conn()?.execute(
            "INSERT INTO llm_metrics (id, session_id, timestamp, model, provider, input_tokens, output_tokens, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, session_id, timestamp, model, provider, input_tokens as i64, output_tokens as i64, status],
        )?;

        Ok(())
    }

    pub fn get_session_token_usage(&self, session_id: &str) -> Result<(u64, u64)> {
        let conn = self.conn()?;
        let (input, output): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM llm_metrics WHERE session_id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .with_context("Failed to get token usage")?;

        Ok((input as u64, output as u64))
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Ok(conn) = self.pool.get() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub project_path: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
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

        // Schema has sessions and llm_metrics tables
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"llm_metrics".to_string()));
    }

    #[test]
    fn test_transaction_panic_safety() {
        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");

        let result = db.transaction(|_conn| {
            panic!("Intentional panic for testing");
            #[allow(unreachable_code)]
            Ok(())
        });

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("panicked"));

        let conn = db.connection();
        assert!(
            conn.is_ok(),
            "Database should still be accessible after panic"
        );
    }

    #[test]
    fn test_session_crud() {
        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let session_id = db
            .create_session("/test/project")
            .expect("Failed to create session");

        let session = db
            .get_session(&session_id)
            .expect("Failed to get session")
            .expect("Session should exist");

        assert_eq!(session.project_path, "/test/project");
        assert_eq!(session.status, "running");

        db.update_session_status(&session_id, "completed")
            .expect("Failed to update status");

        let updated = db
            .get_session(&session_id)
            .expect("Failed to get session")
            .expect("Session should exist");

        assert_eq!(updated.status, "completed");
        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn test_llm_metrics() {
        let db = Database::open_in_memory().expect("Failed to open in-memory database");
        db.initialize().expect("Failed to initialize schema");

        let session_id = db
            .create_session("/test/project")
            .expect("Failed to create session");

        db.record_llm_call(&session_id, "claude-3", "anthropic", 100, 50, "success")
            .expect("Failed to record call");
        db.record_llm_call(&session_id, "claude-3", "anthropic", 200, 100, "success")
            .expect("Failed to record call");

        let (input, output) = db
            .get_session_token_usage(&session_id)
            .expect("Failed to get usage");

        assert_eq!(input, 300);
        assert_eq!(output, 150);
    }

    #[test]
    fn test_pool_config_optimal_sizing() {
        let size = PoolConfig::optimal_pool_size();
        assert!(size >= 4); // MIN_POOL_SIZE
        assert!(size <= 32); // MAX_POOL_SIZE

        let auto = PoolConfig::auto();
        assert_eq!(auto.max_size, size);
        assert!(auto.min_idle >= 2);
        assert!(auto.min_idle <= auto.max_size);
    }
}
