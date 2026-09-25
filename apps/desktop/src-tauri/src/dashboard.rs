//! The main window.
//!
//! Closing to the tray *destroys* the window rather than hiding it. A hidden
//! webview keeps its whole renderer process — several hundred megabytes for a
//! dashboard nobody is looking at — while LocalTrack is meant to sit quietly in
//! the background for a whole working day. Recreating it on demand costs a
//! fraction of a second and gives the memory back in the meantime.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};

pub const LABEL: &str = "main";

/// Bring the dashboard up, recreating the window if it was closed to the tray.
pub fn open(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }

    match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("LocalTrack")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 640.0)
        .resizable(true)
        .center()
        .build()
    {
        Ok(window) => {
            // Runtime-built windows do not inherit the application handler, so
            // closing this one again has to be wired up here.
            window.on_window_event({
                let handle = app.clone();
                move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let state = handle.state::<crate::AppState>();
                        if state.service.settings().close_to_tray {
                            api.prevent_close();
                            let handle = handle.clone();
                            let _ = handle.clone().run_on_main_thread(move || close(&handle));
                        }
                    }
                }
            });
            let _ = window.set_focus();
        }
        Err(err) => tracing::error!(error = %err, "could not reopen the dashboard"),
    }
}

/// Release the dashboard's renderer, keeping collection running in the tray.
pub fn close(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}
