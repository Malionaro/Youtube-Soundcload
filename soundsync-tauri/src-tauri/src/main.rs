// Prevents additional console window on Windows, DO NOT REMOVE!!
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    soundsync_tauri_lib::run()
}
