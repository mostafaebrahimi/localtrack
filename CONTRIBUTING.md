# Contributing

Thanks for helping. LocalTrack has a small number of hard rules, and almost everything
else is negotiable.

## The hard rules

1. **No network by default.** Personal mode has no backend, no account, no telemetry, no
   remote crash reporting, no CDN assets and no remote fonts. All outbound code lives in
   `localtrack-sync` and only runs for a device that somebody deliberately enrolled. A
   request from anywhere else will not be merged.
2. **Employee mode stays visible.** No stealth mode, no policy that can hide the banner,
   the tray icon or the timer, and no remote command channel. A server may lock settings;
   it may never start, stop or pause tracking.
3. **Reports stay aggregate.** URLs, page titles, window titles and notes must not become
   sendable. The payload types are the guarantee — keep them that way.
4. **No surveillance features.** No keylogging, typed text, clipboard, screenshots, screen
   or webcam recording, network interception, form values, cookies or DOM text harvesting.
5. **Privacy before storage.** Sanitization and exclusion rules run before anything
   durable is written, never afterwards.
6. **Never fabricate activity.** A gap is untracked time. It must never become active time.
   Time the machine was on but nobody was there is idle, not untracked, and not work.
7. **Never double count.** Application and website reports are alternative dimensions over
   the same intervals.

## Definition of done

A change is done when: the implementation is complete, tests are added, error states are
handled, privacy implications are checked, cross-platform implications are considered,
there is no hidden network dependency, docs are updated, a migration is included if the
schema changed, and lint and typecheck are clean.

## Layers

```text
collectors → core → storage → interface
```

Collectors know nothing about reporting. The core is pure domain logic with no I/O.
Storage owns every SQL statement. The interface layer (Tauri commands, tray, popup,
export) stays thin.

Interval arithmetic lives in `localtrack-core::interval` and nowhere else — not in SQL and
not in React.

## Local checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test

pnpm -r typecheck
pnpm -r lint
pnpm -r test
```

Please keep tests narrow and fast; the acceptance tests in `crates/localtrack-app/tests`
are the reference for end-to-end behaviour.

## Migrations

Every schema change needs a new numbered migration in
`crates/localtrack-storage/migrations/`. Never edit a migration that has shipped.
