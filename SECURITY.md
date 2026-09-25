# Security Policy

## Reporting a vulnerability

Report it privately through
[GitHub Security Advisories](https://github.com/mostafaebrahimi/localtrack/security/advisories/new).
Please include reproduction steps and the affected version. Do not open a public issue
for an unpatched vulnerability.

We aim to acknowledge reports within 7 days.

## Threat model

LocalTrack runs entirely on one machine with the privileges of the user who started it.
The interesting boundaries are:

1. **The Chrome extension → native host boundary.** The host is started by Chrome and
   speaks length-prefixed JSON on stdin/stdout. Every message is validated: protocol
   version, message type, size, payload shape, timestamp plausibility, URL format, string
   lengths and enum values. Unknown message types are rejected. The host never executes a
   command, opens a file path or runs SQL supplied by the browser.
2. **The dashboard → Rust boundary.** The frontend can only call the narrow, typed Tauri
   commands listed in `apps/desktop/src-tauri/src/commands/mod.rs`. There is no
   `execute_sql` command, no filesystem access and no shell access. Tauri capabilities are
   restricted to window control, a save/open dialog, autostart and the tray.
3. **SQLite.** Every statement is parameterized. Values that originate in the browser are
   never concatenated into SQL. Column names used for grouping come from a fixed allow
   list.
4. **The organization's server → agent boundary** (employee mode only). The policy is the
   only inbound channel. It can lock settings drawn from a fixed allow list and pin their
   values, and it can publish categories and rules. It cannot clock a user in or out,
   pause tracking, delete data, request raw activity, or carry executable content. A
   policy revision that is not newer than the applied one is ignored, so a replayed older
   policy cannot unlock a device. Server responses are parsed into typed structures;
   response bodies are never echoed into the interface.

## Network posture

An unenrolled installation makes no outbound requests at all. Once enrolled, the agent
talks to exactly one host, over HTTPS (plain HTTP is refused except for localhost), with a
bearer token, no cookie store and redirects disabled — a compromised server cannot bounce
the agent to another host. The device token is never serialized into any payload.

## Hardening choices

- Native host manifests use current-user installation and name the exact extension id in
  `allowed_origins`; wildcards are refused.
- The host verifies the caller origin against the installed manifest before serving.
- Manifest V3 forbids remotely hosted code and the extension bundles everything locally.
  Its content security policy sets `connect-src 'none'`.
- The dashboard uses a restrictive CSP, never `dangerouslySetInnerHTML`, and loads no
  remote scripts, fonts or icons.
- Errors returned to the extension are stable codes, never stack traces or internal paths.
- Logs never contain URLs, titles or notes.

## What is out of scope

An attacker who already has code execution as your user account can read the database
directly; LocalTrack does not defend against that and does not claim to. The database is
not encrypted at rest — use full-disk encryption if that matters to you.
