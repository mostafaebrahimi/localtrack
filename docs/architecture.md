# Architecture

LocalTrack is a local-first desktop application with a browser companion. There is no
server anywhere in the runtime.

```text
                       ┌──────────────────────┐
                       │   Chrome Extension   │
                       │     Manifest V3      │
                       └──────────┬───────────┘
                                  │
                       Chrome Native Messaging
                                  │
                       ┌──────────▼───────────┐
                       │ Native Host Process  │
                       │        Rust          │
                       └──────────┬───────────┘
                                  │
                         shared Rust core
                                  │
                          ┌───────▼───────┐
                          │    SQLite     │
                          │ localtrack.db │
                          └───────▲───────┘
                                  │
                         shared Rust core
                                  │
                    ┌─────────────┴─────────────┐
                    │                           │
          ┌─────────▼──────────┐      ┌────────▼────────┐
          │ Desktop Collectors │      │   Tauri App     │
          │ Window / AFK / OS  │      │ React Dashboard │
          └────────────────────┘      └─────────────────┘
```

The extension and the desktop application are two processes that both write to the same
WAL-mode SQLite database through the same Rust code. There is deliberately **no localhost
HTTP API**: the extension ↔ native bridge is Chrome Native Messaging over stdin/stdout.

## Layers

| Layer | Crates | Responsibility |
| --- | --- | --- |
| 1 — Collectors | `localtrack-collector-linux`, `localtrack-collector-windows` | capture only |
| 2 — Core | `localtrack-core` | normalization, merging, privacy, clock, classification, aggregation |
| 3 — Storage | `localtrack-storage` | connections, transactions, queries, migrations, retention, backup |
| 4 — Interface | `apps/desktop`, `apps/chrome-extension`, `localtrack-export` | dashboard, tray, popup, export |

`localtrack-sync` sits beside layer 4 and holds the only outbound network code in the
workspace; it is constructed at start-up but makes no request until a device is enrolled
(see [managed-mode.md](managed-mode.md)).

`localtrack-collector-common` sits between layers 1 and 2: it owns the collector trait and
the ingest pipeline. `localtrack-app` holds the desktop services (queries, reports,
exports, diagnostics, maintenance) so the Tauri command layer can stay a thin, typed
wrapper — the GUI shell contains no logic worth testing, and everything it calls is unit
tested without a window.

## Event processing pipeline

Every observation follows the same path (implemented in
`localtrack-collector-common/src/pipeline.rs`):

```text
Collector
    ↓ Validate           timestamps, lengths, enum values
    ↓ Normalize          whitespace, truncation, browser detection
    ↓ Tracking state     scope, clock state, pause, break
    ↓ Privacy rules      exclusion rules (ignore / duration-only / redact)
    ↓ URL sanitization   policy applied before anything durable
    ↓ Segment merge      heartbeat + boundary rules
    ↓ Classification     rules by priority; manual overrides preserved
    ↓ Persist            one parameterized upsert
```

Privacy filtering always happens **before** storage, never as a later cleanup.

## Segments, not samples

A row per second would be both wasteful and useless. Collectors emit observations; the
merger keeps one open segment per stream (desktop, browser, system) in memory, extends it
while the metadata is unchanged, and closes it when the metadata changes, a boundary is
crossed (break, clock, idle, lock, exclusion) or the heartbeat stops arriving.

Open segments are checkpointed to storage every 15 seconds, so a crash loses at most one
heartbeat interval — and the offline gap is reported as untracked, never as work.

## Reading titles

`localtrack-core::context` turns a window title into a *workspace* — the project,
directory, task, repository or conversation behind it — and a detail, the file or
channel inside it. Editors, terminals, browsers, chat and document applications
each have their own reading; anything unrecognised says nothing rather than
guessing.

Two decisions matter:

- **It is derived at query time, not stored.** No column, no migration, no
  backfill — and it reads activity recorded long before the feature existed. The
  cost is a few string operations per row while a report is built, which is
  nothing next to the interval arithmetic around it.
- **Unreadable time is reported, not dropped.** Titles that say nothing are
  grouped into one "Not identified" row, so the workspace report still adds up to
  the time that was tracked and nobody has to wonder where an hour went.

Rules can match on `workspace` like any other field, which is what lets an
organization's agent answer a daily report with rules about real projects rather
than about applications (see [managed-mode.md](managed-mode.md)).

## Overlay and double counting

Desktop and browser observations describe the *same* wall-clock time from two angles. The
interval engine resolves them by priority:

```text
system lock / AFK  →  browser page  →  desktop application  →  untracked
```

The primary timeline therefore reads `github.com, chatgpt.com, gmail.com` instead of
`Chrome + github.com`. The application report still attributes the full time to Chrome and
the website report attributes it to the domains — but the two are never summed. Total
active time is always real elapsed time.

## Threads

- one thread per platform collector (poll, publish observations, retry with bounded backoff)
- one ingest worker thread (drain the channel, checkpoint every second)
- one status thread (refresh the tray every second; tell the windows only when something changes, with a ten-second heartbeat)
- the native host runs in its own process, started and stopped by Chrome

Nothing is `async`; the workload is a handful of events per second and OS threads keep the
control flow obvious.

## Resource budget

LocalTrack runs beside the work it measures, all day, so its cost is part of the design
rather than an afterthought. Measured on Linux/WebKitGTK with a normal day of data:

| State | Resident | Proportional (PSS) |
| --- | --- | --- |
| In the tray, timer bar showing | ~440 MB | ~190 MB |
| Dashboard open | ~550 MB | ~350 MB |

Scrolling the busiest page — a full day on the Today dashboard — costs about 13% of a CPU
core. It was 94% before the work described below.

What keeps it there:

- **The dashboard window is destroyed when it is closed to the tray**, not hidden. A hidden
  webview keeps its whole renderer process; recreating the window on demand takes a moment
  and gives back a few hundred megabytes for the hours nobody is looking at it.
- **The timer bar is its own page with no framework** — no React, router, query cache or
  charting library for a 540×40 clock. It ticks locally from the session's start time.
- **The charts are hand-drawn SVG and CSS.** A charting library was tried and cost 12% of a
  CPU core continuously, because its responsive container re-measures in a loop.
- **Nothing on the dashboard updates every second.** A per-second clock repaints the whole
  page — around 8% of a core — so the card counts in minutes and the running seconds live in
  the timer bar and the tray badge, which are small enough to repaint for free.
- **Status is pushed on change**, not on a timer, and the page selects the one field it needs
  rather than re-rendering on every update.
- **Today's summary is cached for five seconds** and the per-second queries are indexed
  look-ups, not scans.
- **The page scrolls, not a container inside it.** A nested `overflow: auto` scroller is
  repainted by the CPU on every wheel notch; the main frame gets the engine's fast path.
  The sidebar sticks instead.
- **Long lists are paged, not scrolled.** A day holds a thousand timeline blocks and as
  many activity rows; the lists show twenty-five at a time, and reports list their busiest
  rows with the rest a click away.
- **The timeline folds runs of very short blocks together.** A busy day produces hundreds of
  blocks a few seconds long, all landing on the same pixel column; drawing each one cost
  paint time on every scroll for something nobody can see or click.
- **Compositing stays on.** Turning it off saves about 20 MB per webview and takes scrolling
  from 20% of a core to 94% — a bad trade. `content-visibility: auto` is worse still: it
  measured 287% of a core, because scrolling forces the skipped content back through layout.
- The allocator is capped to two arenas, with free pages returned to the system once a
  minute.

Idle CPU is under 1% of a core for the whole process tree, dashboard open or not.

## Failure behaviour

| Failure | Behaviour |
| --- | --- |
| Collector crashes | application stays alive, collector marked unhealthy, retried with 1/2/5/10/30 s backoff, gap recorded as untracked |
| Chrome disappears | browser segment closes at its last heartbeat plus tolerance, never extended to "now" |
| Native host dies | extension queues at most 500 messages / 5 MB of already-sanitized observations and flushes them in order on reconnect |
| Database busy | busy timeout plus a small bounded retry; never a spin loop |
| Application crash while clocked in | session stays open, state restored on restart, offline period shown as untracked and attributed to the agent being off |
| Termination signal (update, reboot, `kill`) | open segments are flushed, the uptime row is closed and the WAL checkpointed, so an orderly restart leaves no hole |
| System sleep | segments close at the last heartbeat; wake is treated as a new observation |
| Machine left on overnight | idle keeps accruing as idle, and the clock stops itself after the configured idle limit, backdated to the last real input |
| Working past midnight | the session is closed at the local day boundary and a fresh one opened with the same description, so each day is its own record |
| Organization server unreachable | reports queue in an outbox and are delivered in order later; local tracking is unaffected |
