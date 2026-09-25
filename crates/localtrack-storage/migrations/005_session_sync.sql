-- Migration 005_session_sync: two-way synchronisation of work sessions.
--
-- Someone may start a timer in the web application and stop it on their laptop,
-- so both sides hold changes. Each session therefore carries the server's id,
-- a flag for "changed here and not sent yet", and deletions leave a tombstone
-- so they travel like any other change instead of quietly coming back.

ALTER TABLE work_sessions ADD COLUMN remote_id TEXT;
-- Existing rows are pushed once on the first sync.
ALTER TABLE work_sessions ADD COLUMN sync_dirty INTEGER NOT NULL DEFAULT 1;

ALTER TABLE work_breaks ADD COLUMN remote_id TEXT;

CREATE UNIQUE INDEX idx_sessions_remote ON work_sessions(remote_id)
    WHERE remote_id IS NOT NULL;
CREATE INDEX idx_sessions_dirty ON work_sessions(sync_dirty) WHERE sync_dirty = 1;

CREATE TABLE session_tombstones (
    client_id TEXT PRIMARY KEY,
    remote_id TEXT,
    deleted_at_ms INTEGER NOT NULL,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE sync_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
