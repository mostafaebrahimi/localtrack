-- Migration 002_projects: projects and project assignment.

CREATE TABLE projects (
    id TEXT PRIMARY KEY,

    name TEXT NOT NULL UNIQUE,

    archived INTEGER NOT NULL DEFAULT 0,

    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

ALTER TABLE activity_segments
    ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;

ALTER TABLE classification_rules
    ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;

CREATE INDEX idx_segments_project ON activity_segments(project_id);
