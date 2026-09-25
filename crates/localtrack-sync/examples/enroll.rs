//! Enroll a LocalTrack database with a server, for testing employee mode.
//!
//! ```text
//! LOCALTRACK_DATA_DIR=/tmp/lt-employee \
//!   cargo run -p localtrack-sync --example enroll -- http://127.0.0.1:8787 TEAM-CODE
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
        .unwrap_or_else(|| "http://127.0.0.1:8787".into());
    let code = args.get(2).cloned().unwrap_or_else(|| "TEAM-CODE".into());

    paths::ensure_dirs().expect("data directory");
    let db = Arc::new(Database::open(paths::db_path(), now_ms()).expect("database"));
    let transport = Arc::new(HttpTransport::new().expect("transport"));
    let service = AppService::with_transport(db, Some(transport)).expect("service");

    match service.enroll(&server, &code, "Test device") {
        Ok(status) => {
            println!("enrolled with {:?}", status.organization);
            println!("locked settings: {:?}", status.locked_settings);
            println!("categories: {:?}", service.categories().unwrap().len());
            println!("rules: {:?}", service.rules().unwrap().len());
        }
        Err(err) => println!("enrollment failed: {err}"),
    }
}
