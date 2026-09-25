//! System tray (spec §96, §97).
//!
//! The tray always shows the truth: clocked state, elapsed time, the current
//! activity and whether tracking is paused.
//!
//! The menu is built once and its items are updated in place. Rebuilding the
//! whole menu every second would re-register the tray object on Linux, which
//! floods the session bus with warnings.

use std::sync::{Mutex, OnceLock};

use localtrack_app::CurrentStatus;
use localtrack_core::sessions::ClockState;
use localtrack_core::settings::Language;
use localtrack_core::time::format_duration_hms;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use crate::i18n::{self, Key};

pub const TRAY_ID: &str = "localtrack-tray";

const ID_STATUS: &str = "status";
const ID_CURRENT: &str = "current";
const ID_OPEN: &str = "open";
const ID_CLOCK: &str = "clock";
const ID_BREAK: &str = "break";
const ID_PAUSE: &str = "pause";
const ID_MINI: &str = "mini";
const ID_QUIT: &str = "quit";

/// Handles to the menu items that change while the app runs.
///
/// `open` and `quit` never change with the clock, but they do change with the
/// language, and the menu is built once at startup — so they are kept too,
/// and re-labelled when the language moves under them.
struct TrayItems {
    status: MenuItem<Wry>,
    current: MenuItem<Wry>,
    clock: MenuItem<Wry>,
    take_break: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    mini: MenuItem<Wry>,
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// The language the fixed entries were last written in.
static MENU_LANGUAGE: Mutex<Option<Language>> = Mutex::new(None);

static ITEMS: OnceLock<TrayItems> = OnceLock::new();
/// The colour currently drawn on the tray icon, so it is only redrawn on change.
static ICON_STATE: Mutex<Option<IconState>> = Mutex::new(None);

/// What the tray icon colour communicates at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    ClockedOut,
    ClockedIn,
    OnBreak,
    Paused,
}

impl IconState {
    fn of(status: &CurrentStatus) -> Self {
        if matches!(
            status.tracking,
            localtrack_collector_common::pipeline::TrackingDecision::Paused
        ) {
            return IconState::Paused;
        }
        match status.state {
            ClockState::ClockedOut => IconState::ClockedOut,
            ClockState::ClockedIn => IconState::ClockedIn,
            ClockState::OnBreak => IconState::OnBreak,
        }
    }

    fn colour(&self) -> [u8; 3] {
        match self {
            IconState::ClockedIn => [0x3f, 0x9d, 0x6b], // green: on the clock
            IconState::OnBreak => [0xc8, 0x91, 0x3c],   // amber: on a break
            IconState::Paused => [0x9a, 0xa4, 0xae],    // grey: tracking paused
            IconState::ClockedOut => [0x5f, 0x6b, 0x76], // dim: not tracking
        }
    }
}

/// Draw the LocalTrack clock mark tinted by state.
///
/// Not every desktop renders the tray label — GNOME's AppIndicator support
/// drops it — so the icon itself has to carry the status. The mark stays
/// recognisable while the colour says whether the clock is running.
fn icon_for(state: IconState) -> Option<Image<'static>> {
    const SIZE: i32 = 32;
    const SAMPLES: i32 = 3;
    let [r, g, b] = state.colour();

    let centre = SIZE as f64 / 2.0;
    let corner = SIZE as f64 * 0.23;
    let ring = SIZE as f64 * 0.28;
    let stroke = SIZE as f64 * 0.11;

    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (mut tile, mut mark) = (0.0f64, 0.0f64);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = px as f64 + (sx as f64 + 0.5) / SAMPLES as f64;
                    let y = py as f64 + (sy as f64 + 0.5) / SAMPLES as f64;

                    let dx = x.min(SIZE as f64 - x);
                    let dy = y.min(SIZE as f64 - y);
                    let in_tile = if dx < corner && dy < corner {
                        let (ox, oy) = (corner - dx, corner - dy);
                        ox * ox + oy * oy <= corner * corner
                    } else {
                        true
                    };
                    if !in_tile {
                        continue;
                    }
                    tile += 1.0;

                    let dist = ((x - centre).powi(2) + (y - centre).powi(2)).sqrt();
                    let on_ring = (dist - ring).abs() <= stroke / 2.0;
                    let in_quadrant = dist <= ring - stroke && y < centre && x > centre;
                    if on_ring || in_quadrant {
                        mark += 1.0;
                    }
                }
            }

            let total = (SAMPLES * SAMPLES) as f64;
            if tile == 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mark_ratio = mark / total;
            let blend = |channel: u8| -> u8 {
                (channel as f64 * (1.0 - mark_ratio) + 255.0 * mark_ratio).round() as u8
            };
            let alpha = (255.0 * tile / total).round() as u8;
            rgba.extend_from_slice(&[blend(r), blend(g), blend(b), alpha]);
        }
    }

    Some(Image::new_owned(rgba, SIZE as u32, SIZE as u32))
}

/// The language for the tray, from the same setting the dashboard reads.
///
/// The menu is built once, so the fixed entries follow the setting from the
/// next start; the entries that change with the clock are translated on every
/// update and follow it immediately.
fn language(app: &AppHandle<Wry>) -> Language {
    app.try_state::<crate::AppState>()
        .map(|state| state.service.settings().language)
        .unwrap_or(Language::System)
}

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<TrayIcon<Wry>> {
    let lang = language(app);
    let (headline, current, clock_label, break_label, pause_label) = labels(None, lang);

    let status_item = MenuItem::with_id(app, ID_STATUS, headline, false, None::<&str>)?;
    let current_item = MenuItem::with_id(app, ID_CURRENT, current, false, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        ID_OPEN,
        i18n::t(lang, Key::OpenDashboard),
        true,
        None::<&str>,
    )?;
    let clock = MenuItem::with_id(app, ID_CLOCK, clock_label, true, None::<&str>)?;
    let take_break = MenuItem::with_id(app, ID_BREAK, break_label, false, None::<&str>)?;
    let pause = MenuItem::with_id(app, ID_PAUSE, pause_label, true, None::<&str>)?;
    let mini = MenuItem::with_id(
        app,
        ID_MINI,
        i18n::t(lang, Key::ShowTimer),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, ID_QUIT, i18n::t(lang, Key::Quit), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &current_item,
            &separator,
            &open,
            &clock,
            &take_break,
            &separator,
            &mini,
            &pause,
            &separator,
            &quit,
        ],
    )?;

    let _ = ITEMS.set(TrayItems {
        status: status_item,
        current: current_item,
        clock: clock.clone(),
        take_break: take_break.clone(),
        pause: pause.clone(),
        mini: mini.clone(),
        open: open.clone(),
        quit: quit.clone(),
    });
    if let Ok(mut seen) = MENU_LANGUAGE.lock() {
        *seen = Some(i18n::resolve(lang));
    }

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_for(IconState::ClockedOut).expect("tray icon"))
        .icon_as_template(false)
        .tooltip("LocalTrack")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let state = app.state::<crate::AppState>();
            let service = state.service.clone();
            match event.id().as_ref() {
                ID_OPEN => show_dashboard(app),
                ID_CLOCK => {
                    let result = match service.current_status().map(|s| s.state) {
                        Ok(ClockState::ClockedOut) => service.clock_in(),
                        Ok(_) => service.clock_out(),
                        Err(err) => Err(err),
                    };
                    if let Err(err) = result {
                        tracing::warn!(error = %err, "tray clock action failed");
                    }
                }
                ID_BREAK => {
                    let result = match service.current_status().map(|s| s.state) {
                        Ok(ClockState::OnBreak) => service.end_break(),
                        Ok(ClockState::ClockedIn) => service.start_break(),
                        Ok(ClockState::ClockedOut) => return,
                        Err(err) => Err(err),
                    };
                    if let Err(err) = result {
                        tracing::warn!(error = %err, "tray break action failed");
                    }
                }
                ID_MINI => {
                    let visible = crate::mini::is_visible(app);
                    crate::mini::apply(app, !visible);
                    if let Err(err) = service.update_setting(
                        localtrack_core::settings::keys::SHOW_MINI_TIMER,
                        serde_json::Value::Bool(!visible),
                    ) {
                        tracing::warn!(error = %err, "could not persist the timer window setting");
                    }
                }
                ID_PAUSE => {
                    let paused = service.settings().tracking_paused;
                    if let Err(err) = service.set_paused(!paused) {
                        tracing::warn!(error = %err, "tray pause action failed");
                    }
                }
                ID_QUIT => {
                    // Quit really stops collection (spec §99).
                    service.shutdown();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)
}

/// The short badge shown next to the tray icon.
///
/// On Linux this is the AppIndicator label, on macOS the status-item title.
/// It answers "am I on the clock, and for how long?" without opening anything.
pub fn badge(status: Option<&CurrentStatus>) -> String {
    let Some(status) = status else {
        return String::new();
    };

    let paused = matches!(
        status.tracking,
        localtrack_collector_common::pipeline::TrackingDecision::Paused
    );

    match status.state {
        // Nothing on the clock: keep the panel quiet.
        ClockState::ClockedOut => String::new(),
        ClockState::ClockedIn => {
            let mark = if paused { "\u{23f8}" } else { "\u{25cf}" };
            format!("{mark} {}", compact_duration(status.session_duration_ms))
        }
        ClockState::OnBreak => {
            let elapsed = status
                .open_break
                .as_ref()
                .map(|b| localtrack_core::time::now_ms() - b.started_at_ms)
                .unwrap_or(0);
            format!("\u{2016} {}", compact_duration(elapsed))
        }
    }
}

/// `M:SS` under an hour, `H:MM:SS` above it — short enough for a panel.
fn compact_duration(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Menu labels for a status. Pure, so it can be reasoned about and tested.
pub fn labels(
    status: Option<&CurrentStatus>,
    lang: Language,
) -> (String, String, String, String, String) {
    let Some(status) = status else {
        return (
            "LocalTrack".into(),
            i18n::t(lang, Key::NotTracking).into(),
            i18n::t(lang, Key::ClockIn).into(),
            i18n::t(lang, Key::TakeBreak).into(),
            i18n::t(lang, Key::PauseTracking).into(),
        );
    };

    let paused = matches!(
        status.tracking,
        localtrack_collector_common::pipeline::TrackingDecision::Paused
    );

    let headline = match status.state {
        ClockState::ClockedOut => i18n::t(lang, Key::ClockedOut).to_string(),
        ClockState::ClockedIn => format!(
            "\u{25cf} {} \u{2014} {}",
            i18n::t(lang, Key::ClockedIn),
            i18n::figures(lang, &format_duration_hms(status.session_duration_ms))
        ),
        ClockState::OnBreak => format!(
            "\u{2016} {} \u{2014} {}",
            i18n::t(lang, Key::OnBreak),
            i18n::figures(
                lang,
                &format_duration_hms(
                    status
                        .open_break
                        .as_ref()
                        .map(|b| localtrack_core::time::now_ms() - b.started_at_ms)
                        .unwrap_or(0)
                )
            )
        ),
    };

    let current = if paused {
        i18n::t(lang, Key::TrackingPaused).to_string()
    } else {
        status
            .current_activity
            .as_ref()
            .map(|activity| format!("{}: {}", i18n::t(lang, Key::Current), activity.label))
            .unwrap_or_else(|| i18n::t(lang, Key::NoActivity).to_string())
    };

    let clock_label = match status.state {
        ClockState::ClockedOut => i18n::t(lang, Key::ClockIn),
        _ => i18n::t(lang, Key::ClockOut),
    }
    .to_string();

    let break_label = match status.state {
        ClockState::OnBreak => i18n::t(lang, Key::Resume),
        _ => i18n::t(lang, Key::TakeBreak),
    }
    .to_string();

    let pause_label = if paused {
        i18n::t(lang, Key::ResumeTracking)
    } else {
        i18n::t(lang, Key::PauseTracking)
    }
    .to_string();

    (headline, current, clock_label, break_label, pause_label)
}

/// Refresh the tray after a status change, updating items in place.
pub fn update(app: &AppHandle<Wry>, status: &CurrentStatus) {
    let (headline, current, clock_label, break_label, pause_label) =
        labels(Some(status), language(app));

    if let Some(items) = ITEMS.get() {
        let _ = items.status.set_text(&headline);
        let _ = items.current.set_text(&current);
        let _ = items.clock.set_text(&clock_label);
        let _ = items.take_break.set_text(&break_label);
        let _ = items
            .take_break
            .set_enabled(status.state != ClockState::ClockedOut);
        let _ = items.pause.set_text(&pause_label);
        let lang = language(app);
        let _ = items.mini.set_text(if crate::mini::is_visible(app) {
            i18n::t(lang, Key::HideTimer)
        } else {
            i18n::t(lang, Key::ShowTimer)
        });

        // The fixed entries only need rewriting when the language actually
        // moved; every other update would be setting them to what they say.
        let resolved = i18n::resolve(lang);
        if let Ok(mut seen) = MENU_LANGUAGE.lock() {
            if *seen != Some(resolved) {
                let _ = items.open.set_text(i18n::t(lang, Key::OpenDashboard));
                let _ = items.quit.set_text(i18n::t(lang, Key::Quit));
                *seen = Some(resolved);
            }
        }
    }

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!("LocalTrack\n{headline}\n{current}")));
        let _ = tray.set_title(Some(badge(Some(status))));

        // Redraw the icon only when the state actually changes.
        let state = IconState::of(status);
        if let Ok(mut cached) = ICON_STATE.lock() {
            if *cached != Some(state) {
                if let Some(icon) = icon_for(state) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(false);
                }
                *cached = Some(state);
            }
        }
    }
}

fn show_dashboard(app: &AppHandle<Wry>) {
    crate::dashboard::open(app);
}

#[cfg(test)]
mod tests {
    use super::compact_duration;

    #[test]
    fn badge_durations_stay_short() {
        assert_eq!(compact_duration(0), "0:00");
        assert_eq!(compact_duration(59_000), "0:59");
        assert_eq!(compact_duration(12 * 60_000 + 5_000), "12:05");
        assert_eq!(
            compact_duration(3 * 3_600_000 + 28 * 60_000 + 14_000),
            "3:28:14"
        );
        assert_eq!(compact_duration(-5), "0:00");
    }
}
