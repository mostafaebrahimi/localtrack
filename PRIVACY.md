# Privacy

LocalTrack has two modes, and which one you are in decides where your data goes.

**Personal mode** is the default and needs no configuration:

- All data is stored on your computer, in one SQLite file you own.
- There is no server, no account, no sync and no telemetry.
- No feature requires internet access. Disconnect the machine and everything still works.
- You can see, edit, export and delete everything.

**Employee mode** is opt-in and only starts when somebody enrolls the device with an
organization's server, using an address and a code from an administrator:

- A daily summary of tracked time is sent to that server — totals per application,
  category and project, clock in and clock out times, and nothing else.
- URLs, page titles, window titles and the notes you attach to activities never leave
  the machine.
- Timers synchronize both ways, so what you type in "What are you working on?" is shared
  as the timer's description — that is the entry your organization sees in its own
  application.
- An administrator may lock some settings and publish shared categories and rules.
- Timers synchronize both ways with the organization's web application.
- The app says so on every page, and the *Shared with work* page prints the exact
  payloads that were sent so you can read them.

Employee mode never hides itself. There is no stealth mode, no way for a policy to
switch off the tray icon or the banner, and no remote command channel that could start,
stop or pause tracking behind your back. See [docs/managed-mode.md](docs/managed-mode.md)
for the full protocol.

The rest of this document applies to both modes.

## What LocalTrack records

While you are clocked in (the default scope), it records:

| Data | Example |
| --- | --- |
| Application and process | `Visual Studio Code`, `Code.exe` |
| Window title | `hub-backend — auth.service.ts` |
| Browser, domain, page title | `chrome`, `github.com`, `Fix authentication · PR #23` |
| Sanitized URL | `https://github.com/company/hub/pull/23` |
| Idle and lock periods | `10:30–10:40 idle` |
| Clock records | clock in, clock out, breaks |

## What LocalTrack never records

Keystrokes. Typed text. Passwords. Clipboard contents. Screenshots. Screen or webcam or
microphone recordings. Network traffic or HTTP bodies. Form field values. Cookies. Browser
history imports. Authentication tokens. Page text.

## URL handling

URLs are sanitized **before** anything is written to disk. The default policy stores the
domain and path and discards credentials, query strings and fragments:

```text
https://user:pw@example.com/orders/183?token=abc&page=2#payment
→ https://example.com/orders/183
```

Three policies are available: domain only, domain + path (default), and full URL. Full URL
keeps query strings; LocalTrack warns you before enabling it, and it still strips
credentials. Non-web pages (`chrome://`, `file://`, `about:`) are never stored.

## Exclusions

Exclusion rules match a domain, URL, application, process or window title and apply one of:

- **Record nothing** (default) — the activity never reaches the database.
- **Duration only** — the time is kept under a generic label; the identity is discarded.
  Off unless you enable "record excluded duration".
- **Redact** — the domain or application is kept; URL and title are dropped.

Incognito windows are never tracked unless you explicitly opt in, even if Chrome is
allowed to run the extension in incognito.

## Detailed browser interactions

This module is optional and **off by default**. When you enable it and grant host
permission, LocalTrack records that an interaction happened — a button click, link click,
form submission or coarse scroll activity — plus, for buttons and links, a short
accessible label (max 120 characters).

It never records input values, textarea contents, passwords, selected text, keystrokes,
clipboard data or submitted payloads. The content script refuses to read a label from any
element that can hold typed text.

## Logs

Local rotating logs record operational events only: collectors starting and failing,
migrations, browser connect/disconnect, event counts. URLs, page titles, window titles,
interaction labels and notes are never written to a log file.

## Deleting data

- Delete the last 5 or 15 minutes, today, or any date range
- Delete all activity, with or without clock records
- Retention: keep forever (default), or 30/90/180/365 days. Work sessions are only deleted
  if you explicitly ask for it.

Deleting a range trims segments that straddle the boundary rather than removing whole
blocks of unrelated time.

## Verifying these claims

```bash
# The only outbound network code in the workspace lives in localtrack-sync,
# which is constructed but never used until a device is enrolled.
grep -rn "reqwest" crates --include='*.rs' | grep -v localtrack-sync

# The frontend and the extension make no network calls at all; both are lint-enforced.
grep -rn "fetch(\|XMLHttpRequest" apps --include='*.ts' --include='*.tsx' | grep -v node_modules

# The privacy tests assert that secrets never reach storage.
cargo test -p localtrack-core privacy
cargo test -p localtrack-collector-common --test pipeline
```

Reading the code is the point: LocalTrack is MIT licensed and has no build-time code
generation that could hide behaviour.
