//! The transport against a real socket.
//!
//! `localtrack-app`'s fake transport proves the rules; this proves the wire:
//! paths, authorization headers, JSON shapes and error handling.

use std::sync::mpsc::channel;
use std::sync::Arc;

use localtrack_app::managed::SyncTransport;
use localtrack_core::managed::{Enrollment, Heartbeat, SessionSyncItem};
use localtrack_sync::HttpTransport;

struct Captured {
    method: String,
    path: String,
    authorization: Option<String>,
    device_header: Option<String>,
    body: String,
}

/// A server that answers a fixed script and reports what it received.
fn serve(responses: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<Captured>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
    let address = format!("http://{}", server.server_addr());
    let (tx, rx) = channel();

    std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok(mut request) = server.recv() else {
                return;
            };
            let mut payload = String::new();
            let _ = std::io::Read::read_to_string(request.as_reader(), &mut payload);

            let header = |name: &'static str| -> Option<String> {
                request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv(name))
                    .map(|h| h.value.as_str().to_string())
            };
            let authorization = header("Authorization");
            let device_header = header("X-LocalTrack-Device");
            let _ = tx.send(Captured {
                method: request.method().as_str().to_string(),
                path: request.url().to_string(),
                authorization,
                device_header,
                body: payload,
            });

            let response = tiny_http::Response::from_string(body).with_status_code(status);
            let _ = request.respond(response);
        }
    });

    (address, rx)
}

fn enrollment(server: &str) -> Enrollment {
    Enrollment {
        server_url: server.to_string(),
        device_id: "device-7".into(),
        device_token: "token-xyz".into(),
        organization: Some("Acme".into()),
        employee_ref: None,
        enrolled_at_ms: 0,
        last_policy_at_ms: None,
        last_report_at_ms: None,
        last_heartbeat_at_ms: None,
        last_error: None,
    }
}

#[test]
fn enrolling_posts_the_code_and_reads_the_identity_back() {
    let (address, received) = serve(vec![(
        200,
        r#"{"deviceId":"device-7","deviceToken":"token-xyz","organization":"Acme"}"#.into(),
    )]);
    let transport = HttpTransport::new().unwrap();

    let response = transport
        .enroll(
            &address,
            &localtrack_app::managed::EnrollRequest {
                code: "TEAM-CODE".into(),
                device_name: "laptop".into(),
                platform: "linux".into(),
                app_version: "1.0.0".into(),
                employee_ref: None,
            },
        )
        .unwrap();
    assert_eq!(response.device_id, "device-7");
    assert_eq!(response.organization.as_deref(), Some("Acme"));

    let request = received.recv().unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/enroll");
    assert!(request.body.contains("TEAM-CODE"));
    assert!(request.authorization.is_none(), "no token exists yet");
}

#[test]
fn requests_carry_the_device_token_and_id() {
    let (address, received) = serve(vec![(204, String::new())]);
    let transport = HttpTransport::new().unwrap();

    transport
        .send_report(&enrollment(&address), r#"{"schema":1,"date":"2026-08-20"}"#)
        .unwrap();

    let request = received.recv().unwrap();
    assert_eq!(request.path, "/v1/reports/daily");
    assert_eq!(request.authorization.as_deref(), Some("Bearer token-xyz"));
    assert_eq!(request.device_header.as_deref(), Some("device-7"));
    assert!(request.body.contains("2026-08-20"));
}

#[test]
fn a_policy_is_parsed_and_not_modified_when_unchanged() {
    let policy = r#"{"revision":9,"organization":"Acme","lock":"LOCKED",
        "lockedSettings":["tracking_scope"],"settings":{"tracking_scope":"ALWAYS"},
        "categories":[{"key":"dev","name":"Development"}],"rules":[]}"#;
    let (address, _received) = serve(vec![(200, policy.into()), (304, String::new())]);
    let transport = HttpTransport::new().unwrap();
    let enrollment = enrollment(&address);

    let fetched = transport
        .fetch_policy(&enrollment)
        .unwrap()
        .expect("a policy");
    assert_eq!(fetched.revision, 9);
    assert_eq!(fetched.categories[0].name, "Development");

    // 304 means the device already has the current policy.
    assert!(transport.fetch_policy(&enrollment).unwrap().is_none());
}

#[test]
fn sessions_are_exchanged_in_both_directions() {
    let remote = r#"{"cursor":"c-2","sessions":[{"remoteId":"srv-1","clientId":"web-1",
        "startedAtMs":1000,"endedAtMs":2000,"note":"Client call","breaks":[],
        "updatedAtMs":3000}],"assignedIds":{"local-1":"srv-9"}}"#;
    let (address, received) = serve(vec![(200, remote.into())]);
    let transport = HttpTransport::new().unwrap();

    let local = vec![SessionSyncItem {
        remote_id: None,
        client_id: "local-1".into(),
        started_at_ms: 10,
        ended_at_ms: Some(20),
        note: Some("Offline work".into()),
        breaks: vec![],
        updated_at_ms: 30,
        deleted_at_ms: None,
    }];

    let result = transport
        .sync_sessions(&enrollment(&address), Some("c-1"), &local)
        .unwrap();
    assert_eq!(result.cursor.as_deref(), Some("c-2"));
    assert_eq!(result.sessions.len(), 1);
    assert_eq!(result.sessions[0].note.as_deref(), Some("Client call"));
    assert_eq!(
        result.assigned_ids.get("local-1").map(String::as_str),
        Some("srv-9")
    );

    let request = received.recv().unwrap();
    assert_eq!(request.path, "/v1/sessions/sync");
    assert!(request.body.contains("Offline work"));
    assert!(request.body.contains("\"since\":\"c-1\""));
}

#[test]
fn heartbeats_are_posted_as_json() {
    let (address, received) = serve(vec![(200, "{}".into())]);
    let transport = HttpTransport::new().unwrap();

    transport
        .send_heartbeat(
            &enrollment(&address),
            &Heartbeat {
                schema: 1,
                device_id: "device-7".into(),
                at_ms: 42,
                app_version: "1.0.0".into(),
                clock_state: "CLOCKED_IN".into(),
                today_active_ms: 1234,
                tracking_healthy: true,
                policy_revision: Some(9),
                pending_reports: 0,
            },
        )
        .unwrap();

    let request = received.recv().unwrap();
    assert_eq!(request.path, "/v1/heartbeat");
    assert!(request.body.contains("CLOCKED_IN"));
    assert!(request.body.contains("\"todayActiveMs\":1234"));
}

#[test]
fn server_errors_become_messages_a_person_can_act_on() {
    let (address, _received) = serve(vec![
        (401, "nope".into()),
        (500, "<html>stack trace</html>".into()),
    ]);
    let transport = HttpTransport::new().unwrap();
    let enrollment = enrollment(&address);

    let unauthorized = transport
        .send_report(&enrollment, "{}")
        .unwrap_err()
        .to_string();
    assert!(unauthorized.contains("not authorized"));
    assert!(unauthorized.contains("401"));

    let server_error = transport
        .send_report(&enrollment, "{}")
        .unwrap_err()
        .to_string();
    assert!(server_error.contains("server reported an error"));
    // A server's body is untrusted text and never surfaces.
    assert!(!server_error.contains("stack trace"));
}

#[test]
fn an_unreachable_server_fails_quickly_and_clearly() {
    let transport = HttpTransport::new().unwrap();
    // Port 1 is reserved and never listening.
    let mut enrollment = enrollment("http://127.0.0.1:1");
    enrollment.server_url = "http://127.0.0.1:1".into();

    let error = transport
        .send_report(&enrollment, "{}")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("could not be reached") || error.contains("request to the server failed"),
        "unexpected message: {error}"
    );
}

#[test]
fn a_transport_never_follows_a_redirect_to_another_host() {
    let (address, _received) = serve(vec![(302, String::new())]);
    let transport = HttpTransport::new().unwrap();
    // A redirect is surfaced as a rejection rather than silently followed.
    let error = transport
        .send_report(&enrollment(&address), "{}")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("302") || error.contains("rejected"),
        "got: {error}"
    );
}

/// The service layer over a real socket, end to end.
#[test]
fn the_service_enrols_and_delivers_over_http() {
    let policy = r#"{"revision":1,"organization":"Acme","lock":"UNLOCKED",
        "lockedSettings":[],"settings":{},"categories":[],"rules":[]}"#;
    let (address, received) = serve(vec![
        (
            200,
            format!(
                r#"{{"deviceId":"device-7","deviceToken":"token-xyz","organization":"Acme","policy":{policy}}}"#
            ),
        ),
        (204, String::new()),
    ]);

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        localtrack_storage::Database::open(
            dir.path().join("localtrack.db"),
            localtrack_core::time::now_ms(),
        )
        .unwrap(),
    );
    let service = localtrack_app::AppService::with_transport(
        db,
        Some(Arc::new(HttpTransport::new().unwrap())),
    )
    .unwrap();

    let status = service.enroll(&address, "TEAM-CODE", "laptop").unwrap();
    assert!(status.enrolled);
    assert_eq!(status.organization.as_deref(), Some("Acme"));
    let _ = received.recv().unwrap();

    // A finished day, queued and delivered over the wire.
    let yesterday = localtrack_core::time::local_day_start_ms(
        localtrack_core::time::now_ms() - localtrack_core::time::DAY_MS,
    );
    service
        .create_manual_session(
            yesterday + 3_600_000,
            yesterday + 7_200_000,
            Some("Work".into()),
        )
        .unwrap();
    let managed = service.managed().unwrap();
    managed
        .queue_daily_report(yesterday, &service.settings_view().unwrap().settings)
        .unwrap();
    assert_eq!(managed.flush_outbox().unwrap(), 1);

    let report = received.recv().unwrap();
    assert_eq!(report.path, "/v1/reports/daily");
    assert!(report.body.contains("\"clockedMs\":3600000"));
    assert!(
        !report.body.contains("Work"),
        "the session note stays local"
    );
}
