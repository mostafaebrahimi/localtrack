# Employee mode

LocalTrack is local-first by default: an installation that is never enrolled has
no account, no server and no outbound network traffic at all.

**Employee mode** is the opt-in exception. A device is enrolled with an
organization's server and then:

- sends a **daily summary** of tracked time — aggregates only;
- receives a **policy** that may lock some settings and share categories and
  classification rules;
- **synchronizes work sessions in both directions**, so a timer started in a web
  application can be stopped on the laptop and vice versa;
- sends a periodic **heartbeat** so an administrator can see the agent is alive.

Nothing here is hidden from the person being measured. Employee mode shows a
banner on every page, the tray and floating timer keep working as normal, and
the *Shared with work* page prints the exact payloads that were sent.

## What is and is not sent

| Sent | Never sent |
| --- | --- |
| Daily totals: clocked, work, active, idle, break, untracked | URLs, page titles |
| Time per application, category and project | Window titles |
| Workspace names and their totals, if the policy asks for them (see below) | The titles those names were read from |
| Clock in / clock out times and break lengths | Notes attached to individual activities |
| Timer descriptions — what the person typed in "What are you working on?" — as part of session sync | Individual activity segments |
| Context switches, average and longest focus | Individual activity segments |
| Agent version, clock state, health | Screenshots, keystrokes, clipboard — never recorded at all |

The payload type (`localtrack_core::managed::report::DailyReport`) has no field
for any of the right-hand column, so a change that tried to add one would not
compile. `crates/localtrack-app/tests/managed.rs` asserts on the serialized JSON —
including that a session's description never appears in a *daily report*.

The one thing that does travel as free text is the timer description, and only
through session sync: a timer whose description stayed behind would be useless in
the organization's own web application. The interface says so on the *Shared with
work* page rather than burying it here.

## Workspaces, and the agent that reads them

An application name says how the day was spent; it does not say what it was spent
*on*. LocalTrack reads that out of the window titles it already records — the
project a editor has open, the directory or task a terminal is named after, the
repository or ticket behind a page — and calls it a **workspace**
(`localtrack_core::context`). It is derived, never collected: nothing extra is
recorded to produce it, and it applies to history as well as to new activity.

Workspaces stay on the machine unless the organization's policy turns
`share_workspace_context` on, at which point the daily report carries a list of
names, kinds and totals — never the titles they came from:

```json
"workspaces": [
  { "name": "helpdesk-v2", "kind": "EDITOR", "app": "Code", "ms": 4320000, "visits": 12 },
  { "name": "Kubernetes config review", "kind": "TERMINAL", "app": "Gnome Terminal",
    "ms": 1380000, "visits": 32 }
]
```

That is enough for an agent on the server to do the grouping a person would
otherwise do by hand. The answer comes back through the channel that already
exists — the policy — as categories and rules that match on the `workspace`
field:

```json
{ "revision": 12,
  "categories": [{ "key": "dev", "name": "Development", "color": "#2f6f4f" }],
  "rules": [{ "key": "ws-1", "name": "helpdesk-v2 is Development",
              "targetField": "workspace", "operator": "EXACT",
              "pattern": "helpdesk-v2", "categoryKey": "dev", "priority": 10 }] }
```

The employee sees the whole loop: the setting and its effect are listed on the
*Shared with work* page, the rules the server sent are listed and editable on
*Categories & Rules*, and time whose title said nothing is reported to them as
"Not identified" rather than being quietly dropped — though that row never
leaves the machine.

Two properties keep this from becoming surveillance by another name. The server
can only ask for names and totals, because that is all the payload type has room
for. And it cannot ask for the workspace of a *particular moment*: the report is
a day's aggregate, so "what were they doing at 14:32" has no answer in it.

## Enrolling

The employee enters a server address, an enrollment code and a device name. The
address must be `https://` unless it is localhost.

```http
POST /v1/enroll
{ "code": "TEAM-CODE", "deviceName": "Jane's laptop",
  "platform": "linux x86_64", "appVersion": "1.0.0" }

200 OK
{ "deviceId": "dev_123", "deviceToken": "…", "organization": "Acme",
  "employeeRef": "jdoe", "policy": { … } }
```

The token is stored locally, never serialized into any other payload, and sent
only as a bearer token to the enrolled server.

## Policy

The policy is the **only** inbound channel. It can lock settings, pin their
values, and publish categories and rules. It cannot clock anybody in or out,
pause tracking, delete data, request raw activity, or carry anything executable.

```http
GET /v1/policy        (304 when nothing changed)
{
  "revision": 7,
  "organization": "Acme",
  "lock": "LOCKED",
  "lockedSettings": ["tracking_scope", "url_policy", "launch_at_startup"],
  "settings": { "tracking_scope": "ALWAYS", "url_policy": "DOMAIN_ONLY" },
  "reporting": { "dailyReportLocalTime": "23:30", "heartbeatMinutes": 5,
                 "policyPollMinutes": 30 },
  "categories": [{ "key": "dev", "name": "Development" }],
  "rules": [{ "key": "github", "name": "GitHub", "targetField": "domain",
              "operator": "CONTAINS", "pattern": "github.com",
              "categoryKey": "dev", "priority": 50, "enabled": true }],
  "notice": "Daily summaries are shared with your team"
}
```

Rules of the road, enforced on the device:

- Only settings in `LOCKABLE_SETTINGS` can be locked. The theme is deliberately
  not lockable, and neither is anything that would hide that tracking is on.
- A policy may only pin values for settings it also locks.
- A revision that is not newer than the one already applied is ignored, so a
  replayed older policy cannot unlock a device.
- Categories and rules the server publishes are marked as managed and become
  read-only on the device; the employee can still add their own alongside them.
- When the server stops publishing a rule it is removed. A category is only
  unlinked, because activity may already point at it.

## Daily reports

Queued once an hour for any finished day that has data and has not been reported,
up to a fortnight of catch-up. Today is never reported: it is not over.

```http
POST /v1/reports/daily
{ "schema": 2, "deviceId": "dev_123", "date": "2026-08-20",
  "timezoneOffsetMinutes": 210, "generatedAtMs": 1787…, "appVersion": "1.0.0",
  "totals": { "clockedMs": 28800000, "workMs": …, "activeMs": …,
              "idleMs": …, "breakMs": …, "untrackedMs": … },
  "focus": { "contextSwitches": 143, "averageFocusMs": …, "longestFocusMs": … },
  "sessions": [{ "startedAtMs": …, "endedAtMs": …, "breakMs": … }],
  "applications": [{ "name": "Visual Studio Code", "ms": 12600000 }],
  "categories": [{ "key": "dev", "name": "Development", "ms": … }],
  "projects": [{ "name": "Hub", "ms": … }],
  "workspaces": [{ "name": "helpdesk-v2", "kind": "EDITOR", "app": "Code",
                   "ms": …, "visits": 12 }] }
```

`workspaces` appears only when the policy asked for it; schema 1 servers can read
a schema 2 report by ignoring the field.

Delivery is an outbox: a laptop that spends the day offline keeps its reports and
sends them in order when the server comes back. Re-running a day replaces its
report rather than adding a second one.

## Two-way session sync

```http
POST /v1/sessions/sync
{ "deviceId": "dev_123", "since": "<cursor>", "sessions": [ …local changes… ] }

200 OK
{ "cursor": "<new cursor>", "sessions": [ …remote changes… ],
  "assignedIds": { "<clientId>": "<serverId>" } }
```

A session travels as `{remoteId?, clientId, startedAtMs, endedAtMs?, note,
breaks[], updatedAtMs, deletedAtMs?}`. Merge rules, implemented as pure functions
in `localtrack_core::managed::sync` and tested exhaustively:

- last writer wins on `updatedAtMs`;
- a local edit that has not reached the server yet wins ties, so a change made on
  the device is never silently discarded;
- deletions travel as tombstones and win over equal-or-older edits, so a deleted
  session does not come back at the next pull;
- if the server sends more than one running session, all but the most recent are
  closed at the next session's start — only one timer may run (spec §15).

## Heartbeat

```http
POST /v1/heartbeat
{ "schema": 1, "deviceId": "dev_123", "atMs": …, "appVersion": "1.0.0",
  "clockState": "CLOCKED_IN", "todayActiveMs": …, "trackingHealthy": true,
  "policyRevision": 7, "pendingReports": 0 }
```

## Leaving

An employee can disconnect a device whenever the policy is unlocked. While it is
locked, unenrolling is refused and the interface says to ask an administrator —
otherwise the lock would not be a lock. Disconnecting removes the enrollment, the
policy and the outbox; it never deletes recorded activity, which belongs to the
person who did the work.

## What we deliberately did not build

- No stealth or hidden mode. The banner, tray icon and timer cannot be turned off
  by a policy.
- No remote command channel: the server cannot start, stop or pause anything.
- No raw activity endpoint. If an administrator needs per-application detail
  beyond daily totals, that is a product decision to take deliberately — and to
  tell employees about — not something to slip into a report schema.
