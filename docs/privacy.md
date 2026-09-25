# Privacy design notes

The user-facing statement lives in [`../PRIVACY.md`](../PRIVACY.md). This file records
*how* those guarantees are implemented, so a reviewer can check them.

## Where sanitization happens

| Step | Code |
| --- | --- |
| Extension sanitizes before queueing or sending | `apps/chrome-extension/src/tracking/activity.ts` |
| Shared sanitizer (TS) | `packages/protocol/src/index.ts` → `sanitizeUrl` |
| Host validates and re-sanitizes | `crates/localtrack-native-host/src/protocol.rs` |
| Pipeline applies policy + exclusions before storage | `crates/localtrack-collector-common/src/pipeline.rs` |
| Canonical sanitizer (Rust) | `crates/localtrack-core/src/privacy/url.rs` |

Both sanitizers are tested against the same cases, including the specification's
`https://example.com/login?password=secret&token=123` example.

## Ordering guarantee

`IngestPipeline::observe` runs the tracking-state check, then exclusions, then the merge —
so an excluded or out-of-scope observation can never reach `repo::segments::upsert`. The
integration test `excluded_domains_are_never_stored` asserts on the database contents, not
on intermediate values.

## Exclusion semantics

`ExclusionMatcher` evaluates every enabled rule and the **strictest** action wins
(`IGNORE` > `DURATION_ONLY` > `REDACT`). Domain rules also cover subdomains; URL rules
match with or without the scheme. `DURATION_ONLY` produces a segment labelled
`Excluded Website` / `Excluded Application` and only when `record_excluded_duration` is on
— otherwise the activity is dropped entirely.

Excluded activity is a hard boundary for merging: the stream is closed so an excluded page
can never be absorbed into the neighbouring segment.

## Interaction capture

The content script is registered dynamically and only when both conditions hold: the user
enabled detailed interactions **and** granted host permission. `<all_urls>` is never
requested at install time; it is an optional host permission.

`safeLabel` refuses to read a label from `input`, `textarea` or `select` unless the input
is a button/submit/reset, so typed text cannot become a label. Labels are whitespace-
collapsed and clamped to 120 characters. Form submissions record only that a submission
happened.

## Logging

`localtrack_app::logging` and the native host logger write to rotating files under
`logs/`, keep five files, and log only operational events. Activity metadata is never
passed to a log macro; diagnostics text is assembled from counters and health values only,
which the test `diagnostics_contain_no_activity_metadata` asserts.

## Employee mode boundaries

| Guarantee | Where it is enforced |
| --- | --- |
| Reports carry aggregates only | `localtrack-core/src/managed/report.rs` — the type has no field for a URL, title or note |
| Workspaces are names and totals, never the titles they were read from | `report.rs::WorkspaceTotal`, and off unless `share_workspace_context` is on |
| A policy may only lock listed settings | `managed/policy.rs::LOCKABLE_SETTINGS` |
| A policy may only pin what it locks | `ManagedPolicy::pinned_settings` |
| Replayed older policies are ignored | `ManagedPolicy::supersedes` |
| A locked device cannot be silently unenrolled | `ManagedService::unenroll` |
| The device token never leaves as data | `#[serde(skip_serializing)]` on `Enrollment::device_token` |
| Only HTTPS off localhost | `managed::validate_server_url` |

`crates/localtrack-app/tests/managed.rs` asserts each of these against a fake server,
including that a session note never appears in a delivered payload.

## Workspaces

LocalTrack reads a **workspace** — the project, directory, repository, task or
conversation — out of window titles it has already recorded
(`localtrack-core/src/context.rs`). Three things are worth being clear about:

- It is derived, not collected. Nothing new is recorded to make it work, it is
  computed when a report is drawn, and turning it off is a matter of not looking
  at the column.
- It reads history. Because it is a pure function of stored fields, it applies to
  activity recorded before the feature existed. Nothing was retro-fitted into the
  database.
- Titles never travel. Personal mode sends nothing at all; in employee mode the
  workspace *name* can travel if the organization's policy asks for it, and the
  title it was read from stays here either way.

## Offline guarantee

The frontend and the extension make no network calls (both lint-enforced), and the
extension's CSP sets `connect-src 'none'`. The single HTTP client in the workspace lives in
`localtrack-sync` and is only ever called for an enrolled device: `a_device_starts_personal_and_local`
and `syncing_does_nothing_at_all_when_the_device_is_not_enrolled` assert that an unenrolled
installation never touches it. The acceptance test for §164 runs the whole clock-in →
browse → export flow with no network involvement of any kind.
