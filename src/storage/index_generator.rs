use std::fs;
use std::path::Path;

use super::Database;
use crate::types::Result;

pub struct IndexGenerator<'a> {
    db: &'a Database,
}

impl<'a> IndexGenerator<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn generate_all<P: AsRef<Path>>(&self, output_dir: P) -> Result<()> {
        let output_dir = output_dir.as_ref();
        fs::create_dir_all(output_dir)?;

        self.generate_modules_index(output_dir)?;
        self.generate_api_catalog(output_dir)?;
        self.generate_dependencies_index(output_dir)?;

        Ok(())
    }

    fn generate_modules_index<P: AsRef<Path>>(&self, output_dir: P) -> Result<()> {
        let conn = self.db.connection()?;
        let mut stmt = conn
            .prepare("SELECT id, name, path FROM nodes WHERE node_type = 'module' ORDER BY path")?;

        let modules: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "path": row.get::<_, String>(2)?
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let content = serde_json::to_string_pretty(&modules)?;
        fs::write(output_dir.as_ref().join("modules.json"), content)?;

        Ok(())
    }

    fn generate_api_catalog<P: AsRef<Path>>(&self, output_dir: P) -> Result<()> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, path, metadata FROM nodes WHERE node_type = 'api' ORDER BY path",
        )?;

        let apis: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                let metadata_str: String = row.get(3)?;
                let metadata: serde_json::Value =
                    serde_json::from_str(&metadata_str).unwrap_or_default();

                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "path": row.get::<_, String>(2)?,
                    "metadata": metadata
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let content = serde_json::to_string_pretty(&serde_json::json!({
            "version": "1.0.0",
            "endpoints": apis
        }))?;
        fs::write(output_dir.as_ref().join("api-catalog.json"), content)?;

        Ok(())
    }

    fn generate_dependencies_index<P: AsRef<Path>>(&self, output_dir: P) -> Result<()> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT
                n.id as node_id,
                n.name as node_name,
                GROUP_CONCAT(e.target_id) as dependencies
            FROM nodes n
            LEFT JOIN edges e ON n.id = e.source_id AND e.edge_type = 'DEPENDS_ON'
            WHERE n.node_type = 'module'
            GROUP BY n.id
            ORDER BY n.path
            "#,
        )?;

        let deps: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                let deps_str: Option<String> = row.get(2)?;
                let dependencies: Vec<&str> = deps_str
                    .as_deref()
                    .map(|s| s.split(',').collect())
                    .unwrap_or_default();

                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "dependencies": dependencies
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let content = serde_json::to_string_pretty(&deps)?;
        fs::write(output_dir.as_ref().join("dependencies.json"), content)?;

        Ok(())
    }
}
