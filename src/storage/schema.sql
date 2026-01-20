-- claudegen Schema
-- Knowledge graph and session management for Claude Code plugin generation

-- Nodes: Entities in the knowledge graph (files, modules, functions)
CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    node_type TEXT NOT NULL,
    path TEXT,
    name TEXT NOT NULL,
    metadata TEXT,
    evidence TEXT,
    tier TEXT DEFAULT 'fact',
    confidence REAL DEFAULT 1.0,
    last_verified TEXT,
    status TEXT DEFAULT 'verified',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path);
CREATE INDEX IF NOT EXISTS idx_nodes_tier ON nodes(tier);

-- Edges: Relationships between nodes
CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    edge_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    metadata TEXT,
    evidence TEXT,
    tier TEXT DEFAULT 'fact',
    confidence REAL DEFAULT 1.0,
    last_verified TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE INDEX IF NOT EXISTS idx_edges_tier ON edges(tier);
CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_unique ON edges(edge_type, source_id, target_id);

-- Sessions: Track generation runs
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    status TEXT DEFAULT 'pending',
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

-- LLM Metrics: Track API usage
CREATE TABLE IF NOT EXISTS llm_metrics (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    model TEXT,
    provider TEXT,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    status TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_llm_session ON llm_metrics(session_id);
