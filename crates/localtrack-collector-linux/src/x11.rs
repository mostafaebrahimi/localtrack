//! X11 adapter (spec §24) using EWMH properties and the MIT screen-saver
//! extension. Fully supported in V1.

use x11rb::connection::Connection;
use x11rb::protocol::screensaver::{ConnectionExt as ScreenSaverExt, State as ScreenSaverState};
use x11rb::protocol::xinput::{
    ConnectionExt as XInputExt, Device, EventMask as XiEventMask, XIEventMask,
};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::adapter::{DesktopCapabilities, WindowAdapter, WindowSnapshot};

/// The title of LocalTrack's own floating timer window.
const OWN_TIMER_TITLE: &str = "LocalTrack Timer";

pub struct X11Adapter {
    conn: RustConnection,
    root: Window,
    net_active_window: u32,
    net_wm_name: u32,
    net_wm_pid: u32,
    net_wm_window_type: u32,
    net_wm_window_type_normal: u32,
    net_wm_window_type_dialog: u32,
    utf8_string: u32,
    screensaver_available: bool,
    /// Raw input events tell us *what kind* of input happened, which decides
    /// whether the keyboard's window or the pointer's window is the one being
    /// used right now.
    xinput_available: bool,
    last_key_ms: i64,
    last_pointer_ms: i64,
}

impl X11Adapter {
    pub fn connect() -> Result<Self, String> {
        let (conn, screen_num) =
            RustConnection::connect(None).map_err(|e| format!("cannot connect to X11: {e}"))?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            conn.intern_atom(false, name.as_bytes())
                .map_err(|e| e.to_string())?
                .reply()
                .map(|r| r.atom)
                .map_err(|e| e.to_string())
        };

        let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let net_wm_pid = intern("_NET_WM_PID")?;
        let net_wm_window_type = intern("_NET_WM_WINDOW_TYPE")?;
        let net_wm_window_type_normal = intern("_NET_WM_WINDOW_TYPE_NORMAL")?;
        let net_wm_window_type_dialog = intern("_NET_WM_WINDOW_TYPE_DIALOG")?;
        let utf8_string = intern("UTF8_STRING")?;

        let screensaver_available = conn
            .screensaver_query_version(1, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some();

        // Raw input events on the root window: no contents, only the fact that
        // a key, button or movement happened.
        let xinput_available = conn
            .xinput_xi_query_version(2, 3)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some()
            && conn
                .xinput_xi_select_events(
                    root,
                    &[XiEventMask {
                        deviceid: Device::ALL_MASTER.into(),
                        mask: vec![
                            XIEventMask::RAW_KEY_PRESS
                                | XIEventMask::RAW_BUTTON_PRESS
                                | XIEventMask::RAW_MOTION,
                        ],
                    }],
                )
                .is_ok();
        if xinput_available {
            let _ = conn.flush();
        }

        Ok(Self {
            conn,
            root,
            net_active_window,
            net_wm_name,
            net_wm_pid,
            net_wm_window_type,
            net_wm_window_type_normal,
            net_wm_window_type_dialog,
            utf8_string,
            screensaver_available,
            xinput_available,
            last_key_ms: 0,
            last_pointer_ms: 0,
        })
    }

    /// Drain pending raw input events, recording when each kind last happened.
    fn drain_input_events(&mut self, now_ms: i64) {
        if !self.xinput_available {
            return;
        }
        // Bounded so a flood of motion events can never stall the poll.
        for _ in 0..512 {
            match self.conn.poll_for_event() {
                Ok(Some(Event::XinputRawKeyPress(_))) => self.last_key_ms = now_ms,
                Ok(Some(Event::XinputRawButtonPress(_))) | Ok(Some(Event::XinputRawMotion(_))) => {
                    self.last_pointer_ms = now_ms
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    /// Is this a window somebody works in, rather than a panel, dock,
    /// notification or menu that the pointer merely passed over?
    fn is_workable_window(&self, window: Window) -> bool {
        let reply = self
            .conn
            .get_property(false, window, self.net_wm_window_type, AtomEnum::ATOM, 0, 8)
            .ok()
            .and_then(|cookie| cookie.reply().ok());

        let Some(reply) = reply else {
            // No type at all: most window managers treat that as a normal window.
            return true;
        };
        let Some(types) = reply.value32() else {
            return true;
        };
        let mut saw_any = false;
        for atom in types {
            saw_any = true;
            if atom == self.net_wm_window_type_normal || atom == self.net_wm_window_type_dialog {
                return true;
            }
        }
        !saw_any
    }

    /// LocalTrack's own floating timer sits above everything, so the pointer
    /// rests on it constantly. Looking at the timer is not an activity.
    fn is_own_timer(&self, window: Window) -> bool {
        if self.window_title(window).as_deref() != Some(OWN_TIMER_TITLE) {
            return false;
        }
        // Confirm it really is ours rather than a window that happens to share
        // the title.
        self.window_pid(window)
            .and_then(process_name_for_pid)
            .map(|process| process.to_lowercase().starts_with("localtrack"))
            .unwrap_or(false)
    }

    /// The top-level window under the pointer, if any.
    fn pointer_window(&self) -> Option<Window> {
        let reply = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        let child = reply.child;
        if child == x11rb::NONE {
            return None;
        }
        // The pointer reports the direct child of the root, which is the frame
        // the window manager put around the application window.
        self.top_level_with_class(child, 0)
    }

    /// Walk down a frame until a window that identifies itself is found.
    fn top_level_with_class(&self, window: Window, depth: u8) -> Option<Window> {
        if depth > 4 {
            return None;
        }
        if self.window_class(window).is_some() {
            return Some(window);
        }
        let tree = self.conn.query_tree(window).ok()?.reply().ok()?;
        for child in tree.children {
            if let Some(found) = self.top_level_with_class(child, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    fn active_window_id(&self) -> Result<Option<Window>, String> {
        let reply = self
            .conn
            .get_property(
                false,
                self.root,
                self.net_active_window,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let value = reply.value32().and_then(|mut v| v.next());
        Ok(match value {
            Some(0) | None => None,
            Some(window) => Some(window),
        })
    }

    fn window_title(&self, window: Window) -> Option<String> {
        // Prefer _NET_WM_NAME (UTF-8), fall back to WM_NAME (latin-1).
        let utf8 = self
            .conn
            .get_property(false, window, self.net_wm_name, self.utf8_string, 0, 4096)
            .ok()?
            .reply()
            .ok()?;
        if !utf8.value.is_empty() {
            return String::from_utf8(utf8.value).ok();
        }
        let legacy = self
            .conn
            .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 4096)
            .ok()?
            .reply()
            .ok()?;
        if legacy.value.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&legacy.value).into_owned())
        }
    }

    fn window_pid(&self, window: Window) -> Option<u32> {
        let reply = self
            .conn
            .get_property(false, window, self.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        reply.value32().and_then(|mut v| v.next())
    }

    fn window_class(&self, window: Window) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        // WM_CLASS is "instance\0class\0"; the class is the human-facing name.
        let parts: Vec<&[u8]> = reply.value.split(|b| *b == 0).collect();
        let class = parts.get(1).or_else(|| parts.first())?;
        if class.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(class).into_owned())
        }
    }
}

/// Turn an X11 window class into a readable application name.
///
/// WM_CLASS values are identifiers, not labels: `Gnome-terminal`,
/// `TelegramDesktop`, `jetbrains-idea`. Splitting on separators and camel case
/// gives something a person recognises without inventing information.
pub fn humanize_class(class: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for chunk in class.split(['-', '_', '.']).filter(|c| !c.is_empty()) {
        let mut current = String::new();
        let chars: Vec<char> = chunk.chars().collect();
        for (index, ch) in chars.iter().enumerate() {
            let previous = if index == 0 {
                None
            } else {
                chars.get(index - 1)
            };
            let next = chars.get(index + 1);
            // Split `fooBar` into `foo Bar`, and `VSCodium` into `VS Codium`.
            let after_lowercase = previous.is_some_and(|p| p.is_lowercase() || p.is_numeric());
            let acronym_boundary = previous.is_some_and(|p| p.is_uppercase())
                && next.is_some_and(|n| n.is_lowercase());
            let starts_word = ch.is_uppercase() && (after_lowercase || acronym_boundary);
            if starts_word && !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            current.push(*ch);
        }
        if !current.is_empty() {
            words.push(current);
        }
    }

    words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => word,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read the process name for a pid from procfs.
pub fn process_name_for_pid(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = comm.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

impl WindowAdapter for X11Adapter {
    fn name(&self) -> &'static str {
        "x11"
    }

    fn capabilities(&self) -> DesktopCapabilities {
        DesktopCapabilities {
            adapter: "x11".into(),
            active_window: true,
            window_title: true,
            process_name: true,
            afk: self.screensaver_available,
            lock: self.screensaver_available,
            detail: if self.screensaver_available {
                None
            } else {
                Some("The X11 screen-saver extension is unavailable, so idle time cannot be measured.".into())
            },
        }
    }

    fn active_window(&mut self) -> Result<Option<WindowSnapshot>, String> {
        let now = crate::now_ms();
        self.drain_input_events(now);

        let focused = self.active_window_id()?;

        // Scrolling or clicking in a window that never took keyboard focus is
        // still working in it. The most recent kind of input decides which
        // window the time belongs to — but only for a window somebody could
        // actually be working in.
        let pointer = if self.xinput_available && self.last_pointer_ms > self.last_key_ms {
            self.pointer_window()
                .filter(|window| self.is_workable_window(*window))
        } else {
            None
        };

        let window = pointer
            .or(focused)
            .filter(|window| !self.is_own_timer(*window))
            .or_else(|| focused.filter(|window| !self.is_own_timer(*window)));

        let Some(window) = window else {
            // Only our own timer is in play: report no window rather than
            // billing time to the tracker itself.
            return Ok(None);
        };
        let pid = self.window_pid(window);
        let process_name = pid.and_then(process_name_for_pid);
        let app_name = self
            .window_class(window)
            .map(|class| humanize_class(&class))
            .or_else(|| process_name.clone());
        Ok(Some(WindowSnapshot {
            app_name,
            process_name,
            window_title: self.window_title(window),
            pid,
        }))
    }

    fn idle_time_ms(&mut self) -> Result<i64, String> {
        if !self.screensaver_available {
            return Err("screen-saver extension unavailable".into());
        }
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        Ok(info.ms_since_user_input as i64)
    }

    fn is_locked(&mut self) -> Result<Option<bool>, String> {
        if !self.screensaver_available {
            return Ok(None);
        }
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        Ok(Some(
            ScreenSaverState::from(info.state) == ScreenSaverState::ON,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::humanize_class;

    #[test]
    fn window_classes_become_readable_names() {
        assert_eq!(humanize_class("Gnome-terminal"), "Gnome Terminal");
        assert_eq!(humanize_class("Google-chrome"), "Google Chrome");
        assert_eq!(humanize_class("TelegramDesktop"), "Telegram Desktop");
        assert_eq!(humanize_class("code"), "Code");
        assert_eq!(humanize_class("jetbrains-idea"), "Jetbrains Idea");
        assert_eq!(humanize_class("localtrack"), "Localtrack");
        assert_eq!(humanize_class("VSCodium"), "VS Codium");
        assert_eq!(humanize_class(""), "");
    }
}
