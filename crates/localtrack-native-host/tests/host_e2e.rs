//! End-to-end native host behaviour: extension messages in, segments out.

use std::sync::Arc;

use localtrack_core::time::now_ms;
use localtrack_storage::{repo, Database};
use serde_json::{json, Value};

#[path = "../src/framing.rs"]
mod framing;
#[path = "../src/host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../src/install.rs"]
mod install;
#[path = "../src/protocol.rs"]
mod protocol;

fn message(kind: &str, id: &str, payload: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": 1,
        "messageId": id,
        "type": kind,
        "sentAt": now_ms(),
        "payload": payload,
    }))
    .unwrap()
}

fn response(raw: Vec<u8>) -> Value {
    serde_json::from_slice(&raw).unwrap()
}

#[test]
fn browser_activity_is_stored_after_clocking_in() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    let mut host = host::Host::new(db.clone(), Some("chrome-extension://abc/".into())).unwrap();

    let hello = response(host.handle_raw(&message("hello", "1", json!({}))));
    assert_eq!(hello["type"], "ack");
    assert_eq!(hello["payload"]["protocolVersion"], 1);

    let clock = response(host.handle_raw(&message(
        "clock.command",
        "2",
        json!({"command": "clock_in"}),
    )));
    assert_eq!(clock["payload"]["state"], "CLOCKED_IN");

    let start = now_ms();
    let activity = response(host.handle_raw(&message(
        "browser.activity",
        "3",
        json!({
            "event": "activated",
            "capturedAt": start,
            "browser": "chrome",
            "windowId": 41,
            "tabId": 152,
            "url": "https://github.com/company/project/pull/23?token=secret",
            "title": "Fix authentication · Pull Request #23",
            "incognito": false,
            "audible": false,
            "focused": true
        }),
    )));
    assert_eq!(activity["type"], "ack");

    // A page change closes the previous segment.
    host.handle_raw(&message(
        "browser.activity",
        "4",
        json!({
            "event": "activated",
            "capturedAt": start + 5_000,
            "browser": "chrome",
            "url": "https://chatgpt.com/",
            "title": "ChatGPT",
            "focused": true
        }),
    ));

    let segments = db
        .read(|conn| repo::segments::load_range(conn, start - 1000, start + 60_000))
        .unwrap();
    let github = segments
        .iter()
        .find(|s| s.domain.as_deref() == Some("github.com"))
        .expect("github segment stored");
    assert_eq!(
        github.url.as_deref(),
        Some("https://github.com/company/project/pull/23"),
        "the query string is stripped before storage"
    );
    assert!(!format!("{segments:?}").contains("secret"));
}

#[test]
fn incognito_and_excluded_activity_never_reach_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    db.write(|tx| {
        repo::exclusions::upsert(
            tx,
            &localtrack_core::privacy::ExclusionRule {
                id: "x".into(),
                enabled: true,
                target: localtrack_core::privacy::ExclusionTarget::Domain,
                pattern: "bank.example.com".into(),
                action: localtrack_core::privacy::ExclusionAction::Ignore,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
        )
    })
    .unwrap();

    let mut host = host::Host::new(db.clone(), None).unwrap();
    host.handle_raw(&message(
        "clock.command",
        "1",
        json!({"command": "clock_in"}),
    ));

    let start = now_ms();
    host.handle_raw(&message(
        "browser.activity",
        "2",
        json!({"event": "activated", "capturedAt": start, "browser": "chrome",
               "url": "https://secret.example.com/x", "title": "Secret", "incognito": true,
               "focused": true}),
    ));
    host.handle_raw(&message(
        "browser.activity",
        "3",
        json!({"event": "activated", "capturedAt": start + 1000, "browser": "chrome",
               "url": "https://bank.example.com/accounts", "title": "Accounts", "focused": true}),
    ));
    host.handle_raw(&message(
        "browser.activity",
        "4",
        json!({"event": "activated", "capturedAt": start + 30_000, "browser": "chrome",
               "url": "https://github.com/a", "title": "A", "focused": true}),
    ));
    host.pipeline_mut()
        .close_all_streams(start + 60_000)
        .unwrap();

    let dump = format!(
        "{:?}",
        db.read(|conn| repo::segments::load_range(conn, start - 1000, start + 120_000))
            .unwrap()
    );
    assert!(!dump.contains("secret.example.com"));
    assert!(!dump.contains("bank.example.com"));
    assert!(dump.contains("github.com"));
}

#[test]
fn malformed_and_hostile_messages_are_rejected_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    let mut host = host::Host::new(db, None).unwrap();

    let bad_version = response(
        host.handle_raw(
            &serde_json::to_vec(
                &json!({"version": 99, "messageId": "a", "type": "hello", "sentAt": now_ms()}),
            )
            .unwrap(),
        ),
    );
    assert_eq!(bad_version["error"]["code"], "UNSUPPORTED_PROTOCOL_VERSION");

    let unknown =
        response(host.handle_raw(&message("shell.exec", "b", json!({"cmd": "rm -rf /"}))));
    assert_eq!(unknown["error"]["code"], "UNKNOWN_MESSAGE_TYPE");

    let malformed = response(host.handle_raw(b"{ not json"));
    assert_eq!(malformed["error"]["code"], "MALFORMED_JSON");

    // No stack traces or internal paths leak back to the extension.
    let body = malformed.to_string();
    assert!(!body.contains("src/"));
    assert!(!body.contains("panic"));
}

#[test]
fn clock_commands_reject_invalid_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    let mut host = host::Host::new(db, None).unwrap();

    let out = response(host.handle_raw(&message(
        "clock.command",
        "1",
        json!({"command": "clock_out"}),
    )));
    assert_eq!(out["type"], "error");

    host.handle_raw(&message(
        "clock.command",
        "2",
        json!({"command": "clock_in"}),
    ));
    let again = response(host.handle_raw(&message(
        "clock.command",
        "3",
        json!({"command": "clock_in"}),
    )));
    assert_eq!(again["type"], "error");

    let status = response(host.handle_raw(&message("status.request", "4", json!({}))));
    assert_eq!(status["payload"]["state"], "CLOCKED_IN");
}

#[test]
fn framing_round_trip_matches_chrome() {
    let mut buffer = Vec::new();
    framing::write_message(&mut buffer, &message("hello", "1", json!({}))).unwrap();
    let length = u32::from_ne_bytes(buffer[0..4].try_into().unwrap()) as usize;
    assert_eq!(length, buffer.len() - 4);
    let mut cursor = std::io::Cursor::new(buffer);
    let body = framing::read_message(&mut cursor).unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["type"], "hello");
}
