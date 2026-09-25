-- Migration 006_uptime: when LocalTrack itself was running.
--
-- Untracked time has two very different causes: the person was clocked in and
-- nothing could be observed, or LocalTrack was simply not running (an update, a
-- reboot, a crash). Reports can only tell them apart if the agent records its
-- own uptime, so a gap can be explained instead of looking like lost work.

CREATE TABLE agent_uptime (
    id TEXT PRIMARY KEY,

    started_at_ms INTEGER NOT NULL,
    -- Refreshed while running, so a crash still leaves an accurate end.
    last_seen_ms INTEGER NOT NULL,
    -- Set on a clean shutdown; NULL after a crash or a kill.
    stopped_at_ms INTEGER,

    app_version TEXT NOT NULL
);

CREATE INDEX idx_uptime_period ON agent_uptime(started_at_ms, last_seen_ms);
