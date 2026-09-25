// On non-Windows hosts the polling loop is compiled out; the imports it needs
// would otherwise look unused.
#![cfg_attr(not(windows), allow(unused_imports, dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use localtrack_collector_common::collector::{
    ActivityCollector, CollectorError, CollectorStatus, ObservationSender,
};
use localtrack_collector_common::AfkTracker;
use localtrack_core::activity::{Observation, SystemLockObservation, WindowObservation};
use localtrack_core::time::now_ms;

use crate::WindowsCapabilities;

/// Desktop collector for Windows.
pub struct WindowsDesktopCollector {
    poll_interval_ms: u64,
    afk_threshold_ms: i64,
    status: Arc<Mutex<CollectorStatus>>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WindowsDesktopCollector {
    pub fn new(poll_interval_ms: u64, afk_threshold_ms: i64) -> Self {
        // Spec §22: observe the foreground window every 1–2 seconds, no faster.
        Self {
            poll_interval_ms: poll_interval_ms.clamp(1_000, 5_000),
            afk_threshold_ms,
            status: Arc::new(Mutex::new(CollectorStatus::healthy("desktop", None))),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    pub fn capabilities(&self) -> WindowsCapabilities {
        WindowsCapabilities::default()
    }
}

impl ActivityCollector for WindowsDesktopCollector {
    fn name(&self) -> &'static str {
        "desktop"
    }

    #[cfg(windows)]
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
                let mut afk = AfkTracker::new(afk_threshold_ms);
                let mut last: Option<crate::win32::ForegroundWindow> = None;
                let mut was_locked = false;
                // Windows has no cheap raw-input feed, so pointer movement is
                // inferred by comparing the cursor position between polls.
                let mut last_cursor = crate::win32::cursor_position();
                let mut pointer_moved_at: i64 = 0;
                let mut last_idle_ms: i64 = 0;

                while !stop_flag.load(Ordering::Relaxed) {
                    let now = now_ms();

                    if let Some(locked) = crate::win32::is_locked() {
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

                    if !was_locked {
                        if let Some(idle_ms) = crate::win32::idle_time_ms() {
                            let cursor = crate::win32::cursor_position();
                            if cursor != last_cursor {
                                last_cursor = cursor;
                                pointer_moved_at = now;
                            } else if idle_ms < last_idle_ms {
                                // Input happened without the cursor moving:
                                // a scroll wheel, or the keyboard.
                                pointer_moved_at = pointer_moved_at.max(0);
                            }
                            last_idle_ms = idle_ms;

                            if let Some(observation) = afk.update(idle_ms, now) {
                                if sender.send(observation).is_err() {
                                    break;
                                }
                            }
                        }
                    }

                    // Keep the idle or locked segment alive while it lasts.
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
                        // Prefer the window under the pointer while the pointer
                        // is the thing being used.
                        let observed = if now - pointer_moved_at < 2_000 {
                            crate::win32::pointer_window().or_else(crate::win32::foreground_window)
                        } else {
                            crate::win32::foreground_window()
                        };
                        match observed {
                            Some(window) => {
                                if let Ok(mut guard) = status.lock() {
                                    *guard = CollectorStatus::healthy("desktop", Some(now));
                                }
                                let changed = last.as_ref() != Some(&window);
                                let message = if changed {
                                    last = Some(window.clone());
                                    Observation::WindowChanged(WindowObservation {
                                        captured_at_ms: now,
                                        app_name: window.app_name,
                                        process_name: window.process_name,
                                        window_title: window.title,
                                        pid: window.pid,
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
                            None => {
                                // No foreground window (a desktop switch, or a
                                // full-screen client the shell hides) while the
                                // user is demonstrably active: record the input.
                                if sender
                                    .send(Observation::InputActivity {
                                        captured_at_ms: now,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                                last = None;
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

    #[cfg(not(windows))]
    fn start(&mut self, _sender: ObservationSender) -> Result<(), CollectorError> {
        Err(CollectorError::Unavailable(
            "the Windows collector only runs on Windows".into(),
        ))
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
