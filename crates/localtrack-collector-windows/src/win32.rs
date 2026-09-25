//! Thin, safe wrappers over the Win32 calls the collector needs.

use std::path::Path;

use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, MAX_PATH};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_SWITCHDESKTOP,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, WindowFromPoint, GA_ROOT,
};

/// The foreground window's title, process name and pid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForegroundWindow {
    pub title: Option<String>,
    pub process_path: Option<String>,
    pub process_name: Option<String>,
    pub app_name: Option<String>,
    pub pid: Option<u32>,
}

pub fn foreground_window() -> Option<ForegroundWindow> {
    window_details(unsafe { GetForegroundWindow() })
}

/// The top-level window under the mouse pointer.
///
/// Scrolling a window that never took keyboard focus is still working in it,
/// so the collector consults this when the pointer is what moved last.
pub fn pointer_window() -> Option<ForegroundWindow> {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return None;
        }
        let hwnd = WindowFromPoint(point);
        if hwnd.0.is_null() {
            return None;
        }
        // WindowFromPoint returns a child control; walk up to its window.
        window_details(GetAncestor(hwnd, GA_ROOT))
    }
}

/// The current mouse position, used to notice pointer movement between polls.
pub fn cursor_position() -> Option<(i32, i32)> {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return None;
        }
        Some((point.x, point.y))
    }
}

fn window_details(hwnd: HWND) -> Option<ForegroundWindow> {
    unsafe {
        if hwnd.0.is_null() {
            return None;
        }

        let title = window_title(hwnd);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let (process_path, process_name) = if pid != 0 {
            match process_image_path(pid) {
                Some(path) => {
                    let name = Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                    (Some(path), name)
                }
                None => (None, None),
            }
        } else {
            (None, None)
        };

        // The application name is the executable stem, e.g. Code.exe → Code.
        let app_name = process_name.as_ref().map(|name| {
            Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone())
        });

        Some(ForegroundWindow {
            title,
            process_path,
            process_name,
            app_name,
            pid: if pid == 0 { None } else { Some(pid) },
        })
    }
}

unsafe fn window_title(hwnd: HWND) -> Option<String> {
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0u16; (length + 1) as usize];
    let written = GetWindowTextW(hwnd, &mut buffer);
    if written <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buffer[..written as usize]);
    if title.trim().is_empty() {
        None
    } else {
        Some(title)
    }
}

unsafe fn process_image_path(pid: u32) -> Option<String> {
    let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buffer = vec![0u16; MAX_PATH as usize];
    let mut size = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(buffer.as_mut_ptr()),
        &mut size,
    );
    let _ = CloseHandle(handle);
    if result.is_err() || size == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..size as usize]))
}

/// Milliseconds since the last keyboard or mouse input (spec §22, §26).
pub fn idle_time_ms() -> Option<i64> {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let now = GetTickCount64();
            // dwTime is a 32-bit tick count; compare within the same 32-bit epoch.
            let last = info.dwTime as u64;
            let now_32 = now & 0xFFFF_FFFF;
            let elapsed = if now_32 >= last {
                now_32 - last
            } else {
                (0x1_0000_0000u64 - last) + now_32
            };
            Some(elapsed as i64)
        } else {
            None
        }
    }
}

/// Whether the interactive session is locked.
///
/// When the workstation is locked the input desktop belongs to the secure
/// (Winlogon) desktop and cannot be opened, which is the documented way to
/// detect lock state without owning a window for session notifications.
pub fn is_locked() -> Option<bool> {
    unsafe {
        match OpenInputDesktop(Default::default(), false, DESKTOP_SWITCHDESKTOP) {
            Ok(desktop) => {
                let _ = CloseDesktop(desktop);
                Some(false)
            }
            Err(_) => Some(true),
        }
    }
}
