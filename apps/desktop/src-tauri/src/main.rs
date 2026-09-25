// A console window would be pointless for a tray application on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    localtrack_desktop_lib::tune_process();
    localtrack_desktop_lib::run();
}
