//! LocalTrack Chrome Native Messaging host.
//!
//! Chrome starts this process and speaks length-prefixed JSON over stdin and
//! stdout. The host writes browser activity into the same local SQLite
//! database the desktop application uses. It makes no network calls of any
//! kind and never executes anything a browser message asks it to.

mod framing;
mod host;
mod install;
mod logging;
mod protocol;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use localtrack_core::time::now_ms;
use localtrack_storage::{paths, Database};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("install") => run_install(&args),
        Some("uninstall") => run_uninstall(),
        Some("--version") | Some("-V") => {
            println!("localtrack-native-host {}", env!("CARGO_PKG_VERSION"));
        }
        _ => run_host(&args),
    }
}

fn run_install(args: &[String]) {
    let extension_ids: Vec<String> = args
        .iter()
        .skip(2)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .collect();
    if extension_ids.is_empty() {
        eprintln!(
            "usage: localtrack-native-host install <chrome-extension-id> [more-ids...]\n\
             The extension id is shown on chrome://extensions with developer mode enabled."
        );
        std::process::exit(2);
    }

    let executable =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("localtrack-native-host"));
    match install::install(&executable, &extension_ids) {
        Ok(paths) if paths.is_empty() => {
            eprintln!("No Chrome-family browser directory was found; nothing was installed.");
            std::process::exit(1);
        }
        Ok(paths) => {
            for path in paths {
                println!("installed {}", path.display());
            }
        }
        Err(err) => {
            eprintln!("installation failed: {err}");
            std::process::exit(1);
        }
    }
}

fn run_uninstall() {
    match install::uninstall() {
        Ok(paths) => {
            for path in paths {
                println!("removed {}", path.display());
            }
        }
        Err(err) => {
            eprintln!("uninstall failed: {err}");
            std::process::exit(1);
        }
    }
}

fn run_host(args: &[String]) {
    let _guard = logging::init("native-host");

    // Chrome passes the calling extension's origin as the first argument.
    // Chrome passes `chrome-extension://ID/`; Firefox passes the add-on id.
    let origin = args
        .iter()
        .skip(1)
        .find(|a| a.starts_with("chrome-extension://") || a.contains('@'))
        .cloned();

    if let Err(err) = paths::ensure_dirs() {
        tracing::error!(error = %err, "cannot create the LocalTrack data directory");
        std::process::exit(1);
    }

    if !origin_is_allowed(origin.as_deref()) {
        tracing::error!("refusing to serve an unrecognised extension origin");
        std::process::exit(1);
    }

    let db = match Database::open(paths::db_path(), now_ms()) {
        Ok(db) => Arc::new(db),
        Err(err) => {
            tracing::error!(error = %err, "cannot open the database");
            std::process::exit(1);
        }
    };

    let mut host = match host::Host::new(db, origin.clone()) {
        Ok(host) => host,
        Err(err) => {
            tracing::error!(error = %err, "cannot start the native host");
            std::process::exit(1);
        }
    };

    tracing::info!("native host connected");

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut processed: u64 = 0;

    loop {
        match framing::read_message(&mut reader) {
            Ok(raw) => {
                let response = host.handle_raw(&raw);
                if let Err(err) = framing::write_message(&mut writer, &response) {
                    tracing::warn!(error = %err, "cannot write response, disconnecting");
                    break;
                }
                processed += 1;
            }
            Err(framing::FramingError::Closed) => {
                tracing::info!(processed, "browser disconnected");
                break;
            }
            Err(err) => {
                tracing::warn!(error = %err, "framing error, disconnecting");
                break;
            }
        }
    }

    // Close any open browser segment at its last known heartbeat.
    if let Err(err) = host.pipeline_mut().close_all_streams(now_ms()) {
        tracing::warn!(error = %err, "could not flush open segments");
    }

    let _ = writer.flush();
    let _ = reader.read(&mut []);
}

/// Validate the caller against the installed manifest (spec §106).
fn origin_is_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        // Chrome always supplies an origin; a missing one means the host was
        // started manually, which is allowed for diagnostics.
        return true;
    };

    for dir in install::manifest_dirs()
        .into_iter()
        .chain(install::firefox_manifest_dirs())
    {
        let path = dir.join(format!("{}.json", install::HOST_NAME));
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<install::HostManifest>(&body) else {
            continue;
        };
        if install::origin_allowed(&manifest, origin) {
            return true;
        }
    }
    false
}
