-- =============================================================================
-- WeaveWiki Knowledge Graph & Documentation Schema
-- =============================================================================
-- Multi-Agent Documentation Pipeline (6-Phase)
-- Phase 1: Characterization (project profiling)
-- Phase 2: File Discovery
-- Phase 3: Bottom-Up Analysis (file-level insights)
-- Phase 4: Top-Down Analysis (project-level insights)
-- Phase 5: Consolidation (domain grouping)
-- Phase 6: Refinement (quality-driven iteration)
-- =============================================================================

-- =============================================================================
-- Core Knowledge Graph
-- =============================================================================

-- Nodes: All entities in the knowledge graph
CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    node_type TEXT NOT NULL,      -- file, class, function, module, etc.
    path TEXT,                    -- File path for file-based nodes
    name TEXT NOT NULL,
    metadata TEXT,                -- JSON: Additional metadata
    evidence TEXT,                -- JSON: Source evidence
    tier TEXT DEFAULT 'fact',     -- fact, inference, speculation
    confidence REAL DEFAULT 1.0,
    last_verified TEXT,           -- Last verification timestamp
    status TEXT DEFAULT 'active', -- active, deprecated, stale
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path);
-- Composite index for structural facts query: nodes by path and tier
CREATE INDEX IF NOT EXISTS idx_nodes_path_tier ON nodes(path, tier);
-- Performance index for status/tier filtering
CREATE INDEX IF NOT EXISTS idx_nodes_status_tier ON nodes(status, tier);

-- Edges: Relationships between nodes
CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    edge_type TEXT NOT NULL,      -- depends_on, contains, extends, implements, etc.
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    metadata TEXT,                -- JSON: Additional metadata
    evidence TEXT,                -- JSON: Source evidence
    tier TEXT DEFAULT 'fact',     -- fact, inference, speculation
    confidence REAL DEFAULT 1.0,
    last_verified TEXT,           -- Last verification timestamp
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_unique ON edges(edge_type, source_id, target_id);
-- Performance index for dependency queries by source and type
CREATE INDEX IF NOT EXISTS idx_edges_source_type ON edges(source_id, edge_type);

-- =============================================================================
-- Pipeline Sessions
-- =============================================================================

-- Documentation Sessions: Track pipeline runs
CREATE TABLE IF NOT EXISTS doc_sessions (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,

    -- Pipeline state
    status TEXT DEFAULT 'pending',  -- pending, running, paused, completed, failed
    current_phase INTEGER DEFAULT 1,

    -- Progress
    total_files INTEGER DEFAULT 0,
    files_analyzed INTEGER DEFAULT 0,

    -- Quality
    quality_score REAL DEFAULT 0.0,

    -- Timestamps
    started_at TEXT,
    last_checkpoint_at TEXT,
    completed_at TEXT,

    -- Error handling
    last_error TEXT,

    -- Analysis mode: fast, standard, deep
    analysis_mode TEXT DEFAULT 'standard',

    -- Detected project scale: small, medium, large, enterprise
    detected_scale TEXT DEFAULT 'medium',

    -- Project profile from characterization (JSON)
    project_profile TEXT,

    -- Quality scores history per refinement turn (JSON array)
    quality_scores_history TEXT,

    -- Current refinement turn
    refinement_turn INTEGER DEFAULT 0,

    -- Pipeline checkpoint data for resume (JSON blob of PipelineCheckpoint)
    checkpoint_data TEXT,

    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_sessions_project ON doc_sessions(project_path);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON doc_sessions(status);

-- =============================================================================
-- Phase 1: Characterization
-- =============================================================================

-- Characterization Insights: Store agent outputs
CREATE TABLE IF NOT EXISTS characterization_insights (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    turn_number INTEGER NOT NULL,
    insight_json TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_char_insights_session ON characterization_insights(session_id);
CREATE INDEX IF NOT EXISTS idx_char_insights_agent ON characterization_insights(agent_name);

-- =============================================================================
-- Phase 2-3: File Discovery & Bottom-Up Analysis
-- =============================================================================

-- File Tracking: Track file processing status
CREATE TABLE IF NOT EXISTS file_tracking (
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,

    -- File info
    content_hash TEXT NOT NULL,     -- SHA-256 for incremental processing
    line_count INTEGER NOT NULL,
    language TEXT,

    -- Status tracking
    status TEXT DEFAULT 'discovered',  -- discovered, analyzing, analyzed, failed, unanalyzed

    -- Timestamps
    discovered_at TEXT NOT NULL,
    analyzed_at TEXT,

    -- Error tracking
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,

    -- Composite PK: allows same file to be tracked in multiple sessions
    PRIMARY KEY (session_id, file_path),
    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tracking_session ON file_tracking(session_id);
CREATE INDEX IF NOT EXISTS idx_tracking_status ON file_tracking(status);
-- Composite index for resume queries: get pending files for a session
CREATE INDEX IF NOT EXISTS idx_tracking_session_status ON file_tracking(session_id, status);

-- File Analysis: AI analysis results
CREATE TABLE IF NOT EXISTS file_analysis (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,

    -- Meta information
    language TEXT,
    line_count INTEGER,
    complexity TEXT,                -- low, medium, high, critical
    confidence REAL DEFAULT 1.0,

    -- Core analysis
    purpose_summary TEXT,
    sections TEXT NOT NULL,         -- JSON array of dynamic sections
    key_insights TEXT,              -- JSON array
    cross_references TEXT,          -- JSON array

    -- Value discoveries
    hidden_assumptions TEXT,        -- JSON array
    modification_risks TEXT,        -- JSON array

    -- Deep Research context (for Important/Core tiers)
    research_iterations TEXT,       -- JSON: Research iteration findings
    research_aspects TEXT,          -- JSON: Covered aspects for anti-repetition

    analyzed_at TEXT NOT NULL,

    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_analysis_session ON file_analysis(session_id);
CREATE INDEX IF NOT EXISTS idx_analysis_path ON file_analysis(file_path);
CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_unique ON file_analysis(session_id, file_path);
-- Performance index for complexity-based queries
CREATE INDEX IF NOT EXISTS idx_analysis_session_complexity ON file_analysis(session_id, complexity);

-- =============================================================================
-- Phase 4: Top-Down Analysis
-- =============================================================================

-- Module Summaries: Top-down agent outputs
CREATE TABLE IF NOT EXISTS module_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,

    -- Module info
    module_path TEXT NOT NULL,
    module_name TEXT NOT NULL,
    file_count INTEGER DEFAULT 0,

    -- AI Synthesis
    role TEXT,                      -- Core, Feature, Infrastructure, Utility
    purpose TEXT,
    sections TEXT NOT NULL,         -- JSON array

    -- Relationships
    internal_files TEXT,            -- JSON: files in this module
    sub_modules TEXT,               -- JSON: sub-module paths

    synthesized_at TEXT NOT NULL,

    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_modules_session ON module_summaries(session_id);
CREATE INDEX IF NOT EXISTS idx_modules_path ON module_summaries(module_path);

-- =============================================================================
-- Phase 5: Consolidation
-- =============================================================================

-- Domain Summaries: Semantic grouping results
CREATE TABLE IF NOT EXISTS domain_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,

    -- Domain identity
    domain_label TEXT NOT NULL,
    domain_description TEXT,

    -- Grouping info
    folder_paths TEXT,              -- JSON: folders
    source_count INTEGER,
    source_paths TEXT,              -- JSON: file paths

    -- AI-generated content
    sections TEXT NOT NULL,         -- JSON array
    patterns TEXT,                  -- JSON: discovered patterns
    key_concepts TEXT,              -- JSON: key concepts
    aggregated_knowledge TEXT,      -- JSON: aggregated insights
    related_domains TEXT,           -- JSON: related domain IDs

    created_at TEXT NOT NULL,

    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_domain_session ON domain_summaries(session_id);
CREATE INDEX IF NOT EXISTS idx_domain_label ON domain_summaries(domain_label);

-- =============================================================================
-- Metrics & Monitoring
-- =============================================================================

-- LLM Metrics: Track API calls for cost and performance
CREATE TABLE IF NOT EXISTS llm_metrics (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,

    -- Call details
    timestamp TEXT NOT NULL,
    model TEXT,
    provider TEXT,

    -- Token usage
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,

    -- Cost
    estimated_cost REAL DEFAULT 0.0,

    -- Performance
    response_time_ms INTEGER DEFAULT 0,

    -- Status
    status TEXT NOT NULL,           -- success, error, rate_limited, timeout
    error_category TEXT,

    FOREIGN KEY (session_id) REFERENCES doc_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_llm_session ON llm_metrics(session_id);
CREATE INDEX IF NOT EXISTS idx_llm_timestamp ON llm_metrics(timestamp);
CREATE INDEX IF NOT EXISTS idx_llm_status ON llm_metrics(status);
