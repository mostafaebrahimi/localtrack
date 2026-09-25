-- Migration 003_exclusions: privacy exclusion rules (spec §67).

CREATE TABLE exclusion_rules (
    id TEXT PRIMARY KEY,

    enabled INTEGER NOT NULL DEFAULT 1,

    target TEXT NOT NULL,
    pattern TEXT NOT NULL,

    action TEXT NOT NULL,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_exclusions_enabled ON exclusion_rules(enabled);
