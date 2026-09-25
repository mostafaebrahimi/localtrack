# Development

## Prerequisites

- Rust 1.77 or newer (`rustup toolchain install stable`)
- Node 20+ and pnpm 10+
- Linux only, for the Tauri shell: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
  `libsoup-3.0-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`
- Linux X11 tracking additionally needs a running X server; Wayland sessions get reduced
  capability, reported honestly in Diagnostics

```bash
pnpm install
```

## Everyday commands

```bash
# Rust (the GUI shell is excluded from the default members, so this works headless)
cargo test
cargo test -p localtrack-core
cargo test -p localtrack-app --test acceptance
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

# TypeScript
pnpm -r typecheck
pnpm -r lint
pnpm -r test

# Extension bundles
pnpm build:extension                                        # Chrome  → dist/
pnpm --filter @localtrack/chrome-extension build:firefox    # Firefox → dist-firefox/

# Desktop app (requires the GTK/WebKit packages)
pnpm dev:desktop
pnpm build:desktop
```

The million-segment performance check is opt-in:

```bash
cargo test -p localtrack-app --release --test performance -- --ignored --nocapture
```

## Running the pieces separately

The database lives in `~/.local/share/localtrack/localtrack.db`. Override the location
with `LOCALTRACK_DATA_DIR`, which is handy for experiments:

```bash
LOCALTRACK_DATA_DIR=/tmp/lt-dev cargo run -p localtrack-desktop
LOCALTRACK_DATA_DIR=/tmp/lt-dev ./target/debug/localtrack-native-host --version
```

Log verbosity is controlled by `LOCALTRACK_LOG` (`info` by default):

```bash
LOCALTRACK_LOG=debug pnpm dev:desktop
```

Open the application straight onto a page, optionally with a period, when
reviewing the interface:

```bash
LOCALTRACK_START_PAGE=/settings ./target/release/localtrack
LOCALTRACK_START_PAGE="/reports?range=last30" ./target/release/localtrack
```

Ranges accepted by the deep link: `today`, `yesterday`, `thisWeek`, `lastWeek`,
`thisMonth`, `lastMonth`, `last7`, `last30`.

Two examples help when tracking misbehaves:

```bash
# What the Linux collector actually observes, for eight seconds.
DISPLAY=:0 cargo run -p localtrack-collector-linux --example probe

# Enroll a database with a server, for testing employee mode.
LOCALTRACK_DATA_DIR=/tmp/lt-test \
  cargo run -p localtrack-sync --example enroll -- http://127.0.0.1:8787 CODE
```

A plain `cargo build --release -p localtrack-desktop` needs `--features custom-protocol`;
without it Tauri looks for the Vite dev server instead of the embedded dashboard.
`pnpm build:desktop` passes the feature for you.

## Connecting the extension in development

1. `pnpm build:extension`
2. `chrome://extensions` → developer mode → **Load unpacked** → `apps/chrome-extension/dist`
3. Copy the generated extension id (it is stable for an unpacked directory)
4. `cargo build -p localtrack-native-host && ./target/debug/localtrack-native-host install <id>`
5. Reload the extension. The popup should show **Connected**.

To debug the service worker, use the "service worker" link on the extension card. The
native host writes to `~/.local/share/localtrack/logs/native-host.log*` — stdout belongs to
the protocol and must never be used for logging.

## Where to add things

| Change | Where |
| --- | --- |
| New duration or interval maths | `crates/localtrack-core/src/interval` (and tests) |
| New report | `crates/localtrack-core/src/aggregation/reports.rs` + `localtrack-app/src/queries.rs` |
| New period (week, month) | `localtrack-core/src/time.rs` for the boundaries, then `aggregation/summary.rs` |
| New setting | `crates/localtrack-core/src/settings.rs` (add to `keys::ALL`) then the Settings page |
| New table or column | a new migration in `crates/localtrack-storage/migrations/` |
| New command for the UI | `apps/desktop/src-tauri/src/commands/mod.rs` + `apps/desktop/src/services/api.ts` |
| New collector | implement `ActivityCollector`, publish `Observation` values, nothing else |

## Test map

| Area | Tests |
| --- | --- |
| Intervals, clock machine, privacy, classification, aggregation | `cargo test -p localtrack-core` |
| Migrations, CRUD, concurrency, retention, backup/restore | `cargo test -p localtrack-storage` |
| Pipeline: privacy, tracking state, focus rules, boundaries | `cargo test -p localtrack-collector-common` |
| Protocol validation, host end-to-end | `cargo test -p localtrack-native-host` |
| XLSX/CSV export | `cargo test -p localtrack-export` |
| Specification acceptance scenarios §163–§166 | `cargo test -p localtrack-app --test acceptance` |
| Employee mode against a fake server | `cargo test -p localtrack-app --test managed` |
| URL sanitization, queue limits, backoff (TS) | `pnpm -r test` |

## Release checklist

1. `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test`
2. `pnpm -r typecheck && pnpm -r lint && pnpm -r test`
3. `pnpm build:extension`, zip `apps/chrome-extension/dist`
4. `pnpm build:desktop` on Windows and Linux
5. Verify: install, disconnect the network, clock in, work, clock out, report, export
6. Verify an upgrade over the previous version (migrations must apply cleanly)
