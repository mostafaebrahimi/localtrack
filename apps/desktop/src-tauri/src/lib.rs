//! Layer 4 — the desktop interface.
//!
//! This crate is deliberately thin: it wires Tauri, the tray and the window to
//! [`localtrack_app::AppService`], which holds all the logic and is unit tested
//! without a GUI.

pub mod commands;
mod dashboard;
pub mod i18n;
pub mod mini;
pub mod tray;

use std::sync::Arc;

use localtrack_app::AppService;
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

/// Shared state handed to every command.
pub struct AppState {
    pub service: Arc<AppService>,
}

/// Trim the process before anything allocates.
///
/// LocalTrack runs all day beside the user's real work, so it should cost as
/// little as a background service does. This has to be applied before the
/// allocator hands out its first arena, which is why it is called first thing
/// in `main`.
pub fn tune_process() {
    #[cfg(target_os = "linux")]
    {
        // Compositing is left on deliberately. Turning it off saves about 20 MB
        // per webview and costs far more than it is worth: scrolling the
        // dashboard went from 20% of a CPU core to 94%, because every wheel
        // notch repainted the whole window on the CPU.
        //
        // The collector, the sync worker and the webview each bring threads,
        // and glibc gives every one of them its own heap arena by default.
        // Capping the arenas keeps freed memory from being stranded.
        unsafe {
            libc::mallopt(libc::M_ARENA_MAX, 2);
        }
    }
}

/// Return heap pages the allocator is holding but no longer using.
///
/// Reports touch a lot of memory briefly; without this the process keeps that
/// high-water mark for the rest of the day.
fn release_free_memory() {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::malloc_trim(0);
    }
}

/// How long the windows may go without a status update while nothing changes.
const STATUS_HEARTBEAT_SECS: u64 = 10;

/// What has to change before the windows are told about a new status.
///
/// Elapsed durations and today's running totals are left out: the page derives
/// the clock itself, and the heartbeat carries the totals.
fn status_signature(status: &localtrack_app::CurrentStatus) -> String {
    let activity = status
        .current_activity
        .as_ref()
        .map(|activity| format!("{}|{}", activity.label, activity.since_ms))
        .unwrap_or_default();
    format!(
        "{:?}|{:?}|{}|{}|{}|{}|{}|{}",
        status.state,
        status.tracking,
        status
            .session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_default(),
        status
            .session
            .as_ref()
            .and_then(|s| s.note.clone())
            .unwrap_or_default(),
        status
            .open_break
            .as_ref()
            .map(|b| b.started_at_ms)
            .unwrap_or_default(),
        activity,
        status.chrome_connected,
        status.stale_session_warning,
    )
}

/// Build and run the desktop application.
pub fn run() {
    let _log_guard = localtrack_app::logging::init("desktop");

    // The transport only ever sends anything once a device is enrolled; an
    // unenrolled installation makes no network calls at all.
    let transport: Option<std::sync::Arc<dyn localtrack_app::managed::SyncTransport>> =
        match localtrack_sync::HttpTransport::new() {
            Ok(transport) => Some(std::sync::Arc::new(transport)),
            Err(err) => {
                tracing::warn!(error = %err, "server connection unavailable in this build");
                None
            }
        };

    let service = match localtrack_storage::paths::ensure_dirs()
        .map_err(localtrack_app::AppError::from)
        .and_then(|_| {
            let db = std::sync::Arc::new(localtrack_storage::Database::open(
                localtrack_storage::paths::db_path(),
                localtrack_core::time::now_ms(),
            )?);
            AppService::with_transport(db, transport)
        }) {
        Ok(service) => service,
        Err(err) => {
            tracing::error!(error = %err, "LocalTrack could not start");
            eprintln!("LocalTrack could not open its database: {err}");
            std::process::exit(1);
        }
    };

    // A terminated process would otherwise leave its open segment unflushed and
    // its uptime row open, turning an orderly restart into a gap in the day.
    {
        let service = service.clone();
        if let Err(err) = ctrlc::set_handler(move || {
            tracing::info!("stopping on a termination signal");
            service.shutdown();
            std::process::exit(0);
        }) {
            tracing::warn!(error = %err, "could not install the shutdown handler");
        }
    }

    if let Err(err) = service.start() {
        // Collector problems must never stop the dashboard (spec §145).
        tracing::warn!(error = %err, "tracking could not start fully");
    }

    let settings = service.settings();
    let start_hidden = settings.start_minimized;
    let launch_at_startup = settings.launch_at_startup;
    let show_mini_timer = settings.show_mini_timer;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            service: service.clone(),
        })
        .setup(move |app| {
            tray::build(app.handle())?;

            // Keep the OS autostart entry in step with the setting (spec §98).
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                let enabled = autostart.is_enabled().unwrap_or(false);
                if launch_at_startup && !enabled {
                    if let Err(err) = autostart.enable() {
                        tracing::warn!(error = %err, "could not enable autostart");
                    }
                } else if !launch_at_startup && enabled {
                    if let Err(err) = autostart.disable() {
                        tracing::warn!(error = %err, "could not disable autostart");
                    }
                }
            }

            if let Some(window) = app.get_webview_window(dashboard::LABEL) {
                if start_hidden {
                    // Starting in the tray should not pay for a renderer nobody
                    // has asked for; the window is rebuilt when it is opened.
                    let _ = window.destroy();
                } else {
                    // Opening straight onto a page, used when reviewing the
                    // interface: LOCALTRACK_START_PAGE=/settings localtrack
                    //
                    // The fragment is put in the URL and the window navigated,
                    // rather than assigned from a script: a script racing the
                    // first load of the page loses.
                    if let Ok(page) = std::env::var("LOCALTRACK_START_PAGE") {
                        if page.starts_with('/') && page.len() < 64 {
                            match window.url() {
                                Ok(mut url) => {
                                    url.set_fragment(Some(&page));
                                    let _ = window.navigate(url);
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "could not read the window url")
                                }
                            }
                        }
                    }
                }
            }

            // The floating timer is the visible sign that tracking is running.
            if show_mini_timer {
                mini::show(app.handle());
            }

            // The tray refreshes every second; the windows only hear about
            // changes, because a webview re-render is expensive and elapsed
            // time already ticks in the page.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("localtrack-status".into())
                .spawn(move || {
                    let mut ticks: u64 = 0;
                    let mut last_signature = String::new();
                    let mut last_emit = std::time::Instant::now();
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        ticks += 1;
                        // Once a minute, hand back what reports and exports borrowed.
                        if ticks % 60 == 0 {
                            release_free_memory();
                        }
                        let state = handle.state::<AppState>();
                        match state.service.current_status() {
                            Ok(status) => {
                                tray::update(&handle, &status);

                                // Totals still creep up while nothing changes,
                                // so a slow heartbeat keeps the page honest.
                                let signature = status_signature(&status);
                                let stale = last_emit.elapsed()
                                    >= std::time::Duration::from_secs(STATUS_HEARTBEAT_SECS);
                                if signature != last_signature || stale {
                                    last_signature = signature;
                                    last_emit = std::time::Instant::now();
                                    let _ = tauri::Emitter::emit(
                                        &handle,
                                        "localtrack://status",
                                        &status,
                                    );
                                }
                            }
                            Err(err) => tracing::debug!(error = %err, "status refresh failed"),
                        }

                        // Keep the floating timer in step with the setting, whether
                        // it was changed in Settings, from the tray, or by policy.
                        let wanted = state.service.settings().show_mini_timer;
                        if wanted != mini::is_visible(&handle) {
                            mini::apply(&handle, wanted);
                        }
                    }
                })?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Windows created after start-up get this same handler attached in
            // `dashboard::open` and `mini::build`.
            // Dragging the bar around should stick, including across restarts.
            if let WindowEvent::Moved(position) = event {
                if window.label() == mini::LABEL {
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let logical = position.to_logical::<f64>(scale);
                    mini::remember_position(
                        window.app_handle(),
                        logical.x.round() as i64,
                        logical.y.round() as i64,
                    );
                }
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                // Closing the window minimizes to tray by default; collection
                // continues until the user explicitly quits (spec §99). The
                // window is destroyed rather than hidden so its renderer stops
                // costing memory while nobody is looking at it; the tray and the
                // timer bar both bring it back.
                if state.service.settings().close_to_tray && window.label() == dashboard::LABEL {
                    api.prevent_close();
                    let handle = window.app_handle().clone();
                    // Destroying a window from inside its own event handler has
                    // to wait for the handler to return.
                    let _ = window.run_on_main_thread(move || dashboard::close(&handle));
                }
            }
        })
        .invoke_handler(crate::localtrack_handlers!())
        .build(tauri::generate_context!())
        .expect("failed to start LocalTrack")
        .run(move |app_handle, event| match event {
            // Closing the last window leaves LocalTrack running in the tray;
            // only Quit — which exits with a code — really ends it.
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                app_handle.state::<AppState>().service.shutdown();
            }
            _ => {}
        });
}
