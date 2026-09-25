//! The whole employee-mode loop, against a server, for testing.
//!
//! Enrolls, sends a day's report, and prints what came back — which is how the
//! workspace hand-off is meant to work: the agent reads the workspaces out of
//! the report and answers with categories and rules through the policy channel.
//!
//! ```text
//! LOCALTRACK_DATA_DIR=/tmp/lt-employee \
//!   cargo run -p localtrack-sync --example handoff -- http://127.0.0.1:8765 TEAM-CODE 2026-08-21
//! ```

use std::sync::Arc;

use localtrack_app::AppService;
use localtrack_core::time::now_ms;
use localtrack_storage::{paths, Database};
use localtrack_sync::HttpTransport;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let server = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:8765".into());
    let code = args.get(2).cloned().unwrap_or_else(|| "TEAM-CODE".into());

    paths::ensure_dirs().expect("data directory");
    let db = Arc::new(Database::open(paths::db_path(), now_ms()).expect("database"));
    let transport = Arc::new(HttpTransport::new().expect("transport"));
    let service = AppService::with_transport(db, Some(transport)).expect("service");

    if service.managed_status().expect("status").enrolled {
        println!("already enrolled");
    } else {
        let status = service
            .enroll(&server, &code, "Test device")
            .expect("enrollment");
        println!("enrolled with {:?}", status.organization);
    }

    let settings = service.settings();
    println!(
        "share_workspace_context = {}",
        settings.share_workspace_context
    );

    // Yesterday, so the day is finished and reportable.
    let day = localtrack_core::time::local_day_start_ms(now_ms()) - 1;
    let report = service
        .managed()
        .expect("managed")
        .queue_daily_report(localtrack_core::time::local_day_start_ms(day), &settings)
        .expect("queued report");
    println!(
        "queued {} — {} workspaces",
        report.describe(),
        report.workspaces.len()
    );
    for workspace in report.workspaces.iter().take(8) {
        println!(
            "  {:<9} {:<45} {}ms",
            workspace.kind, workspace.name, workspace.ms
        );
    }

    service.sync_now().expect("sync");
    println!("sent: {:?}", service.sent_reports(3).expect("sent").len());

    // The agent's answer arrives as categories and rules on the next policy read.
    let status = service.managed_status().expect("status");
    println!("policy revision now {:?}", status.policy_revision);
    println!(
        "categories: {}, rules: {}",
        service.categories().expect("categories").len(),
        service.rules().expect("rules").len()
    );
    for rule in service.rules().expect("rules").iter().take(5) {
        println!(
            "  rule: {} [{}] {:?}",
            rule.name, rule.pattern, rule.target_field
        );
    }
}
