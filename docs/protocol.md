# Native messaging protocol

Version **1**. Host name `com.localtrack.native`. Transport: Chrome Native Messaging —
UTF-8 JSON, each message prefixed with its 32-bit length, over stdin/stdout.

The shared TypeScript definitions live in `packages/protocol`; the Rust side is
`crates/localtrack-native-host/src/protocol.rs`. JSON Schemas are in
`packages/protocol/schemas/`.

## Envelope

Every message from the extension:

```json
{
  "version": 1,
  "messageId": "uuid",
  "type": "browser.activity",
  "sentAt": 1787253458123,
  "payload": { }
}
```

## Message types

| Type | Direction | Purpose |
| --- | --- | --- |
| `hello` | extension → host | handshake; the ack returns host version and the active privacy settings |
| `browser.activity` | extension → host | active page changed, focused, blurred or closed |
| `browser.heartbeat` | extension → host | the current page is still open (every ~15 s) |
| `browser.interaction` | extension → host | optional detailed interaction |
| `clock.command` | extension → host | clock in / out, start / end break from the popup |
| `status.request` | extension → host | popup status |

### browser.activity payload

```json
{
  "event": "activated",
  "capturedAt": 1787253458117,
  "browser": "chrome",
  "windowId": 41,
  "tabId": 152,
  "url": "https://github.com/company/project/pull/23",
  "title": "Fix authentication by jdoe · Pull Request #23",
  "incognito": false,
  "audible": false,
  "focused": true
}
```

`url` is **already sanitized** by the extension according to the policy the host reported
in the `hello` ack, so no raw query string is ever held in browser storage or crosses the
pipe. The host sanitizes again — trusting the sender is not part of the design.

## Responses

```json
{ "version": 1, "messageId": "uuid", "type": "ack", "receivedAt": 1787253458130, "success": true }
```

```json
{ "version": 1, "messageId": "uuid", "type": "error",
  "error": { "code": "INVALID_MESSAGE", "message": "Unsupported message schema" } }
```

Error codes: `UNSUPPORTED_PROTOCOL_VERSION`, `MALFORMED_JSON`, `MESSAGE_TOO_LARGE`,
`INVALID_MESSAGE`, `UNKNOWN_MESSAGE_TYPE`, `INVALID_TIMESTAMP`. Stack traces and internal
paths are never returned.

## Validation

The host checks, in order: message size (≤ 1 MB), JSON validity, protocol version, message
id shape, known message type, timestamp plausibility (within 24 h of the host clock), then
the payload schema — enum values and these maximum lengths:

```text
app_name 255 · process_name 255 · window_title 2048
domain 255 · url 8192 · page_title 2048 · interaction label 120
```

An unsupported version is rejected cleanly with `UNSUPPORTED_PROTOCOL_VERSION`; a breaking
change would ship as version 2 rather than mutating version 1.

## Connection lifecycle

The extension keeps one long-lived `connectNative` port while tracking is active — not a
`sendNativeMessage` per event, which would spawn a process per heartbeat. On disconnect it
reconnects with a bounded backoff of 1, 2, 5, 10, 30 seconds (capped at 30).

Because an MV3 service worker can be terminated at any time, operational state
(`lastActiveTabId`, `lastKnownUrl`, `nativeConnectionStatus`, `trackingEnabled`,
`lastHeartbeatAt`) is persisted in `chrome.storage.local`, and a `chrome.alarms` alarm
firing every 30 seconds re-establishes the connection and re-sends a heartbeat. SQLite
remains the only authoritative store.

If the host is unavailable, sanitized observations are queued — at most 500 messages or
5 MB, oldest discarded first — and flushed in order on reconnect. The popup tells the user
when anything had to be dropped.

## Browsers

The same background code runs in Chrome and Firefox. Only the packaging differs: Chrome
uses an MV3 service worker, Firefox an event page with a Gecko add-on id
(`localtrack@localtrack.app`), built from `manifest.firefox.json`.

Firefox identifies the caller by add-on id rather than by origin, so its host manifest uses
`allowed_extensions` where Chrome uses `allowed_origins`. The host accepts either form and
checks it against the installed manifest before serving.

## Installation

```bash
localtrack-native-host install <chrome-extension-id>   # registers for Chrome and Firefox
localtrack-native-host uninstall
```

Linux: `~/.config/google-chrome/NativeMessagingHosts/com.localtrack.native.json` (and the
Chromium equivalent when present), plus `~/.mozilla/native-messaging-hosts/` for Firefox.
Windows: a manifest under `%APPDATA%\LocalTrack` plus
`HKCU\Software\Google\Chrome\NativeMessagingHosts\com.localtrack.native` and
`HKCU\Software\Mozilla\NativeMessagingHosts\com.localtrack.native` registry values.

```json
{
  "name": "com.localtrack.native",
  "description": "LocalTrack Native Messaging Host",
  "path": "/usr/local/bin/localtrack-native-host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://EXTENSION_ID/"]
}
```

`allowed_origins` always names the exact extension. The host additionally verifies the
origin Chrome passes on argv against the installed manifest before serving.
