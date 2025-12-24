use rusqlite::params;

use super::Database;
use crate::types::{Edge, Node, ParseWithDefault, Result, enum_to_str, log_filter_error};

pub struct GraphStore<'a> {
    db: &'a Database,
}

impl<'a> GraphStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn insert_node(&self, node: &Node) -> Result<()> {
        let metadata = serde_json::to_string(&node.metadata)?;
        let evidence = serde_json::to_string(&node.evidence)?;

        self.db.execute(
            r#"
            INSERT INTO nodes (id, node_type, path, name, metadata, evidence, tier, confidence, last_verified, status)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                node_type = excluded.node_type,
                path = excluded.path,
                name = excluded.name,
                metadata = excluded.metadata,
                evidence = excluded.evidence,
                tier = excluded.tier,
                confidence = excluded.confidence,
                last_verified = excluded.last_verified,
                status = excluded.status,
                updated_at = CURRENT_TIMESTAMP
            "#,
            &[
                &node.id,
                &enum_to_str(&node.node_type),
                &node.path,
                &node.name,
                &metadata,
                &evidence,
                &enum_to_str(&node.tier),
                &node.confidence,
                &node.last_verified.to_rfc3339(),
                &enum_to_str(&node.status),
            ],
        )?;
        Ok(())
    }

    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        let metadata = serde_json::to_string(&edge.metadata)?;
        let evidence = serde_json::to_string(&edge.evidence)?;

        self.db.execute(
            r#"
            INSERT INTO edges (id, edge_type, source_id, target_id, metadata, evidence, tier, confidence, last_verified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(edge_type, source_id, target_id) DO UPDATE SET
                metadata = excluded.metadata,
                evidence = excluded.evidence,
                tier = excluded.tier,
                confidence = excluded.confidence,
                last_verified = excluded.last_verified
            "#,
            &[
                &edge.id,
                &enum_to_str(&edge.edge_type),
                &edge.source_id,
                &edge.target_id,
                &metadata,
                &evidence,
                &enum_to_str(&edge.tier),
                &edge.confidence,
                &edge.last_verified.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_node(&self, id: &str) -> Result<Option<Node>> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, node_type, path, name, metadata, evidence, tier, confidence, last_verified, status FROM nodes WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![id], |row| Ok(self.row_to_node(row)));

        match result {
            Ok(node) => Ok(Some(node?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_dependents(&self, node_id: &str) -> Result<Vec<String>> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT source_id FROM edges WHERE target_id = ?1 AND edge_type = 'depends_on'",
        )?;

        let ids: Vec<String> = stmt
            .query_map(params![node_id], |row| row.get(0))?
            .filter_map(|r| log_filter_error(r, "reading dependent"))
            .collect();

        Ok(ids)
    }

    pub fn get_dependencies(&self, node_id: &str) -> Result<Vec<String>> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT target_id FROM edges WHERE source_id = ?1 AND edge_type = 'depends_on'",
        )?;

        let ids: Vec<String> = stmt
            .query_map(params![node_id], |row| row.get(0))?
            .filter_map(|r| log_filter_error(r, "reading dependency"))
            .collect();

        Ok(ids)
    }

    /// Clear all nodes and edges from the graph
    pub fn clear(&self) -> Result<()> {
        self.db.execute("DELETE FROM edges", &[])?;
        self.db.execute("DELETE FROM nodes", &[])?;
        Ok(())
    }

    fn row_to_node(&self, row: &rusqlite::Row) -> Result<Node> {
        use crate::types::node::*;

        let id: String = row.get(0)?;
        let node_type_str: String = row.get(1)?;
        let path: String = row.get(2)?;
        let name: String = row.get(3)?;
        let metadata_str: String = row.get(4)?;
        let evidence_str: String = row.get(5)?;
        let tier_str: String = row.get(6)?;
        let confidence: f32 = row.get(7)?;
        let last_verified_str: String = row.get(8)?;
        let status_str: String = row.get(9)?;

        // Use ParseWithDefault for consistent enum parsing with logging
        let node_type = NodeType::parse_or_default(&node_type_str);
        let tier = InformationTier::parse_or_default(&tier_str);
        let status = NodeStatus::parse_or_default(&status_str);

        let metadata: NodeMetadata = serde_json::from_str(&metadata_str).map_err(|e| {
            crate::types::WeaveError::Storage(format!("Invalid node metadata for {}: {}", id, e))
        })?;
        let evidence: EvidenceLocation = serde_json::from_str(&evidence_str).map_err(|e| {
            crate::types::WeaveError::Storage(format!("Invalid node evidence for {}: {}", id, e))
        })?;

        let last_verified = chrono::DateTime::parse_from_rfc3339(&last_verified_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        Ok(Node {
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::node::*;

    fn create_test_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::File,
            path: "test.rs".to_string(),
            name: "test".to_string(),
            metadata: NodeMetadata::default(),
            evidence: EvidenceLocation {
                file: "test.rs".to_string(),
                start_line: 1,
                end_line: 10,
                start_column: None,
                end_column: None,
            },
            tier: InformationTier::Fact,
            confidence: 1.0,
            last_verified: chrono::Utc::now(),
            status: NodeStatus::Verified,
        }
    }

    #[test]
    fn test_insert_and_get_node() {
        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");
        let store = GraphStore::new(&db);

        let node = create_test_node("file:test.rs");
        store.insert_node(&node).expect("Failed to insert node");

        let retrieved = store
            .get_node("file:test.rs")
            .expect("Failed to get node")
            .expect("Node should exist");

        assert_eq!(retrieved.id, "file:test.rs");
        assert_eq!(retrieved.node_type, NodeType::File);
        assert_eq!(retrieved.name, "test");
    }

    #[test]
    fn test_get_nonexistent_node() {
        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");
        let store = GraphStore::new(&db);

        let result = store.get_node("nonexistent").expect("Query should succeed");

        assert!(result.is_none());
    }

    #[test]
    fn test_upsert_node() {
        let db = Database::open_in_memory().expect("Failed to open database");
        db.initialize().expect("Failed to initialize");
        let store = GraphStore::new(&db);

        let mut node = create_test_node("file:test.rs");
        store.insert_node(&node).expect("Failed to insert node");

        // Update the node
        node.name = "updated".to_string();
        store.insert_node(&node).expect("Failed to upsert node");

        let retrieved = store
            .get_node("file:test.rs")
            .expect("Failed to get node")
            .expect("Node should exist");

        assert_eq!(retrieved.name, "updated");
    }
}
