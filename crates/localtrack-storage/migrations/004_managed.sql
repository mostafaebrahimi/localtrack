-- Migration 004_managed: employee mode.
--
-- Enrollment with an organization's server, the policy it pushes, the outbox of
-- reports waiting to be delivered, and the marks that tell a server-managed
-- category or rule apart from one the person created themselves.

CREATE TABLE managed_enrollment (
    id INTEGER PRIMARY KEY CHECK (id = 1),

    server_url TEXT NOT NULL,
    device_id TEXT NOT NULL,
    device_token TEXT NOT NULL,

    organization TEXT,
    employee_ref TEXT,

    enrolled_at_ms INTEGER NOT NULL,
    last_policy_at_ms INTEGER,
    last_report_at_ms INTEGER,
    last_heartbeat_at_ms INTEGER,
    last_error TEXT
);

CREATE TABLE managed_policy (
    id INTEGER PRIMARY KEY CHECK (id = 1),

    revision INTEGER NOT NULL,
    document_json TEXT NOT NULL,
    received_at_ms INTEGER NOT NULL
);

-- Everything queued for the server, so a laptop that spends the day offline
-- still delivers its days in order once it reconnects.
CREATE TABLE sync_outbox (
    id TEXT PRIMARY KEY,

    kind TEXT NOT NULL,
    period_key TEXT,

    payload_json TEXT NOT NULL,

    created_at_ms INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_ms INTEGER,
    last_error TEXT,
    delivered_at_ms INTEGER
);

CREATE INDEX idx_outbox_pending ON sync_outbox(delivered_at_ms, created_at_ms);
-- One report per day per kind: re-running a day replaces it instead of piling up.
CREATE UNIQUE INDEX idx_outbox_period ON sync_outbox(kind, period_key)
    WHERE period_key IS NOT NULL;

ALTER TABLE categories ADD COLUMN managed_key TEXT;
ALTER TABLE classification_rules ADD COLUMN managed_key TEXT;

CREATE UNIQUE INDEX idx_categories_managed ON categories(managed_key)
    WHERE managed_key IS NOT NULL;
CREATE UNIQUE INDEX idx_rules_managed ON classification_rules(managed_key)
    WHERE managed_key IS NOT NULL;
