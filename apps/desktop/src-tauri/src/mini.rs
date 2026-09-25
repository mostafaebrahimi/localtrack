//! The floating mini timer window.
//!
//! It is deliberately a separate always-on-top window rather than a tray label:
//! several Linux desktops (GNOME's AppIndicator support among them) render only
//! the tray icon and silently drop the text, so a label alone cannot be relied
//! on to tell someone that tracking is running.

use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};

pub const LABEL: &str = "mini";

const WIDTH: f64 = 540.0;
const HEIGHT: f64 = 40.0;

/// Show the timer, parking it near the top-right of the primary monitor the
/// first time it appears.
pub fn show(app: &AppHandle<Wry>) {
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        // Turning the bar off destroys it, so turning it back on rebuilds it.
        None => match build(app) {
            Ok(window) => window,
            Err(err) => {
                tracing::error!(error = %err, "could not create the timer window");
                return;
            }
        },
    };

    // Some window managers ignore the configured size for undecorated windows,
    // so it is set explicitly and pinned.
    let size = LogicalSize::new(WIDTH, HEIGHT);
    // The GTK window itself keeps a minimum size from the embedded webview, so
    // the request has to go to the widget as well as the Tauri window.
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        if let Ok(gtk_window) = window.gtk_window() {
            gtk_window.set_size_request(WIDTH as i32, HEIGHT as i32);
            if let Some(child) = gtk_window.child() {
                child.set_size_request(WIDTH as i32, HEIGHT as i32);
            }
        }
    }
    let _ = window.set_size(size);

    // Where the bar sits is the user's choice, so a remembered position wins
    // over the default corner. Showing it again must not move it.
    let settings = app.state::<crate::AppState>().service.settings();
    match (settings.mini_timer_x, settings.mini_timer_y) {
        (Some(x), Some(y)) => {
            let _ = window.set_position(LogicalPosition::new(x as f64, y as f64));
        }
        _ => {
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let scale = monitor.scale_factor();
                let screen = monitor.size().to_logical::<f64>(scale);
                let position = monitor.position().to_logical::<f64>(scale);
                // A small margin from the top-right corner of the screen.
                let x = position.x + (screen.width - (WIDTH + 20.0)).max(0.0);
                let y = position.y + 56.0;
                let _ = window.set_position(LogicalPosition::new(x, y));
            }
        }
    }

    let _ = window.show();
    let _ = window.set_always_on_top(true);
    // Re-apply after mapping: some window managers only honour a resize once
    // the window is actually on screen.
    let _ = window.set_size(size);
    match window.inner_size() {
        Ok(actual) => tracing::info!(
            width = actual.width,
            height = actual.height,
            "timer window size"
        ),
        Err(err) => tracing::warn!(error = %err, "could not read the timer window size"),
    }
}

/// Create the window the same way `tauri.conf.json` does.
fn build(app: &AppHandle<Wry>) -> tauri::Result<tauri::WebviewWindow<Wry>> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("timer.html".into()))
        .title("LocalTrack Timer")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        // Taking focus would make the collector see LocalTrack as the active
        // window every time the bar is shown.
        .focused(false)
        .build()?;
    // A window built at runtime does not inherit the application's window
    // handler, so dragging the bar would not be remembered without this.
    window.on_window_event({
        let handle = app.clone();
        move |event| {
            if let tauri::WindowEvent::Moved(position) = event {
                let scale = handle
                    .get_webview_window(LABEL)
                    .and_then(|w| w.scale_factor().ok())
                    .unwrap_or(1.0);
                let logical = position.to_logical::<f64>(scale);
                remember_position(&handle, logical.x.round() as i64, logical.y.round() as i64);
            }
        }
    });
    Ok(window)
}

/// Hiding the bar destroys it: a hidden webview still holds a renderer process,
/// and someone who turns the bar off should get that memory back.
pub fn hide(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}

/// Remember where the bar was dragged to.
pub fn remember_position(app: &AppHandle<Wry>, x: i64, y: i64) {
    let service = app.state::<crate::AppState>().service.clone();
    let settings = service.settings();
    if settings.mini_timer_x == Some(x) && settings.mini_timer_y == Some(y) {
        return;
    }
    for (key, value) in [
        (localtrack_core::settings::keys::MINI_TIMER_X, x),
        (localtrack_core::settings::keys::MINI_TIMER_Y, y),
    ] {
        if let Err(err) = service.update_setting(key, serde_json::Value::from(value)) {
            tracing::debug!(error = %err, "could not remember the timer position");
        }
    }
}

pub fn is_visible(app: &AppHandle<Wry>) -> bool {
    app.get_webview_window(LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// Apply the current setting to the window.
pub fn apply(app: &AppHandle<Wry>, visible: bool) {
    if visible {
        show(app);
    } else {
        hide(app);
    }
}
