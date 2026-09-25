use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use localtrack_collector_common::collector::{
    ActivityCollector, CollectorError, CollectorStatus, ObservationSender,
};
use localtrack_collector_common::{AfkTracker, Backoff};
use localtrack_core::activity::{Observation, SystemLockObservation, WindowObservation};
use localtrack_core::time::now_ms;

use crate::adapter::{DesktopCapabilities, UnsupportedAdapter, WindowAdapter, WindowSnapshot};
#[cfg(target_os = "linux")]
use crate::session::{desktop_environment, detect_session, SessionKind};

/// Desktop collector for Linux.
///
/// Polls the active window (spec §22 recommends 1–2 seconds), measures idle
/// time and reports lock state, publishing normalized observations.
pub struct LinuxDesktopCollector {
    poll_interval_ms: u64,
    afk_threshold_ms: i64,
    capabilities: DesktopCapabilities,
    status: Arc<Mutex<CollectorStatus>>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LinuxDesktopCollector {
    pub fn new(poll_interval_ms: u64, afk_threshold_ms: i64) -> Self {
        let capabilities = detect_capabilities();
        let status = if capabilities.active_window {
            CollectorStatus::healthy("desktop", None)
        } else {
            CollectorStatus::unavailable(
                "desktop",
                capabilities
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Active-window tracking is unavailable.".into())
                    .as_str(),
            )
        };
        Self {
            poll_interval_ms: poll_interval_ms.max(500),
            afk_threshold_ms,
            capabilities,
            status: Arc::new(Mutex::new(status)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    pub fn capabilities(&self) -> DesktopCapabilities {
        self.capabilities.clone()
    }
}

/// Record that the collector completed a poll, so Diagnostics can show when it
/// was last heard from rather than "never".
fn mark_healthy(status: &Arc<Mutex<CollectorStatus>>, now_ms: i64) {
    if let Ok(mut guard) = status.lock() {
        *guard = CollectorStatus::healthy("desktop", Some(now_ms));
    }
}

/// Build the adapter for this session, honestly reporting what it can do.
pub fn build_adapter() -> Box<dyn WindowAdapter> {
    #[cfg(target_os = "linux")]
    {
        match detect_session() {
            SessionKind::X11 => match crate::x11::X11Adapter::connect() {
                Ok(adapter) => return Box::new(adapter),
                Err(err) => {
                    tracing::warn!(error = %err, "X11 adapter unavailable");
                    return Box::new(UnsupportedAdapter {
                        capabilities: DesktopCapabilities::unsupported("x11", &err),
                    });
                }
            },
            SessionKind::Wayland => {
                // Wayland has no universal foreground-window API (spec §25).
                // XWayland may still expose idle time, so try to use it for AFK
                // only, and never claim active-window support.
                let compositor = desktop_environment().unwrap_or_else(|| "this compositor".into());
                match crate::x11::X11Adapter::connect() {
                    Ok(adapter) => {
                        return Box::new(WaylandAdapter {
                            inner: Some(Box::new(adapter)),
                            compositor,
                        })
                    }
                    Err(_) => {
                        return Box::new(WaylandAdapter {
                            inner: None,
                            compositor,
                        });
                    }
                }
            }
            SessionKind::Unknown => {}
        }
    }

    Box::new(UnsupportedAdapter {
        capabilities: DesktopCapabilities::unsupported(
            "none",
            "No supported display server was detected on this system.",
        ),
    })
}

/// Wayland adapter: idle detection through XWayland when available, never a
/// fabricated active window.
pub struct WaylandAdapter {
    inner: Option<Box<dyn WindowAdapter>>,
    compositor: String,
}

impl WindowAdapter for WaylandAdapter {
    fn name(&self) -> &'static str {
        "wayland"
    }

    fn capabilities(&self) -> DesktopCapabilities {
        let afk = self
            .inner
            .as_ref()
            .map(|inner| inner.capabilities().afk)
            .unwrap_or(false);
        DesktopCapabilities {
            adapter: "wayland".into(),
            active_window: false,
            window_title: false,
            process_name: false,
            afk,
            lock: afk,
            detail: Some(format!(
                "Desktop application tracking is limited on {}. Chrome tracking, clock in/out and reports are unaffected.",
                self.compositor
            )),
        }
    }

    fn active_window(&mut self) -> Result<Option<WindowSnapshot>, String> {
        // Never guess: no active-window API means no active-window data.
        Ok(None)
    }

    fn idle_time_ms(&mut self) -> Result<i64, String> {
        match self.inner.as_mut() {
            Some(inner) => inner.idle_time_ms(),
            None => Err("idle detection unavailable on this compositor".into()),
        }
    }

    fn is_locked(&mut self) -> Result<Option<bool>, String> {
        match self.inner.as_mut() {
            Some(inner) => inner.is_locked(),
            None => Ok(None),
        }
    }
}

pub fn detect_capabilities() -> DesktopCapabilities {
    build_adapter().capabilities()
}

impl ActivityCollector for LinuxDesktopCollector {
    fn name(&self) -> &'static str {
        "desktop"
    }

    fn start(&mut self, sender: ObservationSender) -> Result<(), CollectorError> {
        if self.thread.is_some() {
            return Err(CollectorError::AlreadyRunning);
        }

        let stop_flag = self.stop_flag.clone();
        stop_flag.store(false, Ordering::Relaxed);
        let status = self.status.clone();
        let poll_interval = Duration::from_millis(self.poll_interval_ms);
        let afk_threshold_ms = self.afk_threshold_ms;

        let handle = std::thread::Builder::new()
            .name("localtrack-desktop".into())
            .spawn(move || {
                let mut adapter = build_adapter();
                let mut afk = AfkTracker::new(afk_threshold_ms);
                let mut backoff = Backoff::default();
                let mut last_snapshot: Option<WindowSnapshot> = None;
                let mut was_locked = false;

                while !stop_flag.load(Ordering::Relaxed) {
                    let now = now_ms();

                    // Lock state first: it overrides idle handling (spec §28).
                    match adapter.is_locked() {
                        Ok(Some(locked)) => {
                            if locked != was_locked {
                                was_locked = locked;
                                let observation = SystemLockObservation {
                                    captured_at_ms: now,
                                    locked,
                                };
                                let message = if locked {
                                    Observation::SystemLocked(observation)
                                } else {
                                    Observation::SystemUnlocked(observation)
                                };
                                if sender.send(message).is_err() {
                                    break;
                                }
                                if locked {
                                    afk.force_idle();
                                } else {
                                    afk.force_active(now);
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(err) => tracing::debug!(error = %err, "lock state unavailable"),
                    }

                    if !was_locked {
                        match adapter.idle_time_ms() {
                            Ok(idle_ms) => {
                                if let Some(observation) = afk.update(idle_ms, now) {
                                    if sender.send(observation).is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(err) => tracing::debug!(error = %err, "idle time unavailable"),
                        }
                    }

                    // Being away is a state, not an instant: without this the
                    // idle segment would be closed as stale after half a minute
                    // and a night away from the desk would read as untracked.
                    if was_locked || afk.is_idle() {
                        let alive = Observation::Heartbeat {
                            captured_at_ms: now,
                            stream: "system".into(),
                        };
                        if sender.send(alive).is_err() {
                            break;
                        }
                    }

                    if !was_locked && !afk.is_idle() {
                        match adapter.active_window() {
                            Ok(Some(snapshot)) => {
                                backoff.reset();
                                mark_healthy(&status, now);
                                let changed = last_snapshot.as_ref() != Some(&snapshot);
                                let message = if changed {
                                    last_snapshot = Some(snapshot.clone());
                                    Observation::WindowChanged(WindowObservation {
                                        captured_at_ms: now,
                                        app_name: snapshot.app_name,
                                        process_name: snapshot.process_name,
                                        window_title: snapshot.window_title,
                                        pid: snapshot.pid,
                                    })
                                } else {
                                    Observation::Heartbeat {
                                        captured_at_ms: now,
                                        stream: "desktop".into(),
                                    }
                                };
                                if sender.send(message).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {
                                backoff.reset();
                                mark_healthy(&status, now);
                                // No nameable window (an unsupported Wayland
                                // compositor, or nothing focused) but the user
                                // is at the machine: record the input itself.
                                if sender
                                    .send(Observation::InputActivity { captured_at_ms: now })
                                    .is_err()
                                {
                                    break;
                                }
                                last_snapshot = None;
                            }
                            Err(err) => {
                                // The collector stays alive and retries with
                                // bounded backoff (spec §145).
                                if let Ok(mut guard) = status.lock() {
                                    *guard = CollectorStatus::failed("desktop", &err);
                                }
                                let delay = backoff.next_delay_ms();
                                tracing::warn!(error = %err, delay_ms = delay, "desktop collector failed");
                                std::thread::sleep(Duration::from_millis(delay));
                                adapter = build_adapter();
                                continue;
                            }
                        }
                    }

                    std::thread::sleep(poll_interval);
                }
            })
            .map_err(|e| CollectorError::Failed(e.to_string()))?;

        self.thread = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CollectorError> {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    fn status(&self) -> CollectorStatus {
        self.status
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| CollectorStatus::failed("desktop", "status unavailable"))
    }
}
