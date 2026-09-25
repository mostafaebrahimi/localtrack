-- LocalTrack schema, migration 001_initial.
-- Timestamps are UTC Unix epoch milliseconds stored as INTEGER (spec §13).

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE work_sessions (
    id TEXT PRIMARY KEY,

    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,

    start_timezone_offset_min INTEGER NOT NULL,
    end_timezone_offset_min INTEGER,

    note TEXT,

    created_manually INTEGER NOT NULL DEFAULT 0,
    edited_manually INTEGER NOT NULL DEFAULT 0,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_work_sessions_started ON work_sessions(started_at_ms);
CREATE INDEX idx_work_sessions_open ON work_sessions(ended_at_ms) WHERE ended_at_ms IS NULL;

CREATE TABLE work_breaks (
    id TEXT PRIMARY KEY,

    work_session_id TEXT NOT NULL,

    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,

    note TEXT,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,

    FOREIGN KEY(work_session_id)
        REFERENCES work_sessions(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_work_breaks_session ON work_breaks(work_session_id);
CREATE INDEX idx_work_breaks_started ON work_breaks(started_at_ms);

CREATE TABLE categories (
    id TEXT PRIMARY KEY,

    name TEXT NOT NULL UNIQUE,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE activity_segments (
    id TEXT PRIMARY KEY,

    source TEXT NOT NULL,
    kind TEXT NOT NULL,

    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER NOT NULL,

    timezone_offset_min INTEGER,

    app_name TEXT,
    process_name TEXT,
    window_title TEXT,

    browser TEXT,
    domain TEXT,
    url TEXT,
    page_title TEXT,

    interaction_type TEXT,

    category_id TEXT,

    classification_source TEXT,

    is_afk INTEGER NOT NULL DEFAULT 0,

    metadata_json TEXT,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,

    FOREIGN KEY(category_id)
        REFERENCES categories(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_segments_started ON activity_segments(started_at_ms);
CREATE INDEX idx_segments_period ON activity_segments(started_at_ms, ended_at_ms);
CREATE INDEX idx_segments_source ON activity_segments(source);
CREATE INDEX idx_segments_kind ON activity_segments(kind);
CREATE INDEX idx_segments_app ON activity_segments(app_name);
CREATE INDEX idx_segments_domain ON activity_segments(domain);
CREATE INDEX idx_segments_category ON activity_segments(category_id);

CREATE TABLE classification_rules (
    id TEXT PRIMARY KEY,

    name TEXT NOT NULL,

    enabled INTEGER NOT NULL DEFAULT 1,

    priority INTEGER NOT NULL DEFAULT 0,

    target_field TEXT NOT NULL,
    operator TEXT NOT NULL,
    pattern TEXT NOT NULL,

    category_id TEXT,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,

    FOREIGN KEY(category_id)
        REFERENCES categories(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_rules_priority ON classification_rules(priority DESC);
