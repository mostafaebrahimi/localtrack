# Database

One SQLite file per user: `localtrack.db` in the LocalTrack data directory.

## Connection settings

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

WAL matters because two processes — the desktop application and the Chrome native host —
write to this file concurrently.

## Time

Every timestamp is **UTC Unix epoch milliseconds** stored as `INTEGER`, e.g.
`1787253458123`. Formatted dates are never canonical. `timezone_offset_min` is recorded
alongside sessions and segments for reporting; local day boundaries are computed with
DST-aware arithmetic, so a day is never assumed to be exactly 24 hours.

## Schema (version 5)

| Table | Purpose |
| --- | --- |
| `schema_migrations` | applied migration versions |
| `settings` | `key` → `value_json` |
| `work_sessions` | clock in / clock out records |
| `work_breaks` | breaks inside a session (cascade delete) |
| `categories` | seeded with ten defaults |
| `projects` | added in migration 002 |
| `activity_segments` | merged activity |
| `classification_rules` | rule-based categorization |
| `exclusion_rules` | privacy exclusions (migration 003) |
| `managed_enrollment` | the organization's server, device id and token (004) |
| `managed_policy` | the policy document as received (004) |
| `sync_outbox` | daily reports waiting for delivery (004) |
| `session_tombstones` | deletions waiting to reach the server (005) |
| `sync_state` | synchronisation cursors (005) |
| `agent_uptime` | when LocalTrack itself was running (006) |

### activity_segments

```sql
CREATE TABLE activity_segments (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,               -- DESKTOP | CHROME | SYSTEM | MANUAL
    kind TEXT NOT NULL,                 -- WINDOW | BROWSER_PAGE | IDLE | LOCKED | INTERACTION | MANUAL
    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER NOT NULL,
    timezone_offset_min INTEGER,
    app_name TEXT, process_name TEXT, window_title TEXT,
    browser TEXT, domain TEXT, url TEXT, page_title TEXT,
    interaction_type TEXT,
    category_id TEXT, project_id TEXT,  -- project_id added in 002
    classification_source TEXT,         -- MANUAL | RULE | DEFAULT
    is_afk INTEGER NOT NULL DEFAULT 0,
    metadata_json TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY(category_id) REFERENCES categories(id) ON DELETE SET NULL,
    FOREIGN KEY(project_id)  REFERENCES projects(id)   ON DELETE SET NULL
);
```

Indexes exist on `started_at_ms`, `(started_at_ms, ended_at_ms)`, `source`, `kind`,
`app_name`, `domain`, `category_id` and `project_id`. Range queries use the overlap test
`started_at_ms < :to AND ended_at_ms > :from`, which is index-friendly and correct for
segments that straddle a boundary.

## Migrations

`001_initial`, `002_projects`, `003_exclusions`, `004_managed`, `005_session_sync`,
`006_uptime`. They are embedded in the binary, applied
inside a transaction and recorded in `schema_migrations`. A shipped migration is never
edited; a new change always gets a new version. Opening a database whose version is newer
than the running build is refused rather than guessed at.

## Days and weeks

Reports are computed over local days and local weeks, never over fixed 24-hour
or 168-hour blocks: a day containing a daylight-saving change is 23 or 25 hours
and a week is still seven days. Weeks start on Monday and are labelled by ISO
week (`2026-W34`).

A session that is still running at local midnight is closed at the boundary and
a fresh one opened with the same description, so each day is its own record and
today's figures always start at zero. Turn it off with
`split_sessions_at_midnight` if a night shift should count as one session.

## Explaining a gap

Untracked time has two causes that look identical in the activity table: the
person was clocked in and nothing could be observed, or LocalTrack was not
running — an update, a reboot, a crash. `agent_uptime` records each run, kept
fresh every 15 seconds so a crash still leaves an accurate end, and the summary
reports `untrackedAgentOffMs` alongside `untrackedMs`. Nothing is inferred for
periods before this table existed: an unexplained gap stays unexplained rather
than being guessed at.

## Retention and deletion

Retention runs at most once a day and deletes activity older than the cutoff. Work
sessions survive unless you explicitly enable deleting them.

Deleting a range trims segments that overlap the boundary: a segment that spans the range
is split, heads and tails are clipped, and zero-length remnants are removed. Deleting five
minutes never removes an hour.

## Backup and restore

Backups use SQLite's own online backup API — never a file copy of a live WAL database.
Restoring validates the candidate file first (integrity check plus schema verification),
writes a safety copy of the current database, and only then swaps the contents. The
current database is never destroyed before the replacement has been validated.

## Performance

Reports only ever query the selected period, aggregation happens in Rust over one
range-scoped read, and the activity list is paginated (100 rows by default, 1000 maximum).
`crates/localtrack-app/tests/performance.rs` exercises a million-segment database:

```bash
cargo test -p localtrack-app --release --test performance -- --ignored --nocapture
```
