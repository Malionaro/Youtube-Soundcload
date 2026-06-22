pub mod models;
pub mod spotify;
pub mod applemusic;
pub mod server;
pub mod utils;
pub mod commands;

use models::AppState;
use server::{ss_start_remote_server, ss_get_local_ip};
use commands::*;

use tauri::{
    Emitter,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ✅ WebView2 Logging deaktivieren (muss VOR Builder passieren!)
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-logging --log-level=3",
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            // Tray Icon laden
            let icon_bytes = include_bytes!("../icons/32x32.png");
            let icon = tauri::image::Image::from_bytes(icon_bytes)?;
            let tray_menu = MenuBuilder::new(app)
                .text("status", "Status: Bereit")
                .separator()
                .text("show_ui", "UI öffnen")
                .text("open_folder", "Download-Ordner öffnen")
                .text("tray_pause_resume", "Pause/Fortsetzen")
                .text("tray_cancel", "Download abbrechen")
                .separator()
                .text("quit", "Beenden")
                .build()?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .tooltip("SoundSync Downloader - Bereit")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "status" => {}
                    "show_ui" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "open_folder" => {
                        let folder_path = {
                            let state = app.state::<AppState>();
                            let config = state.config.lock().unwrap();
                            config.download_folder.clone()
                        };
                        if !folder_path.is_empty() {
                            let _ = open_folder(folder_path);
                        }
                    }
                    "tray_pause_resume" => {
                        let _ = app.emit("tray-pause-resume", ());
                    }
                    "tray_cancel" => {
                        let _ = app.emit("tray-cancel-download", ());
                    }
                    "quit" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.destroy();
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button,
                        button_state,
                        ..
                    } = event
                    {
                        if button_state == MouseButtonState::Up {
                            match button {
                                MouseButton::Left => {
                                    // Fenster anzeigen
                                    if let Some(window) =
                                        tray.app_handle().get_webview_window("main")
                                    {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                }
                                MouseButton::Right => {
                                    // The native tray menu opens on right click.
                                }
                                _ => {}
                            }
                        }
                    }
                })
                .build(app)?;

            let _ = ss_start_remote_server(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // ✅ Kein async mehr → verhindert Chromium Fehler
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            update_remote_queue,
            read_clipboard_text,
            reset_download_cancel,
            check_system,
            get_playlist_info,
            download_track,
            cancel_download,
            pause_download,
            resume_download,
            convert_file,
            install_ffmpeg,
            install_ytdlp,
            update_ytdlp,
            install_pot_provider,
            open_folder,
            update_discord_presence,
            ss_get_local_ip,
            ss_start_remote_server,
            download_and_install_update,
            search_videos,
            search_playlists,
            get_trending_videos,
            import_playlist_tracks,
            execute_after_download_action,
            update_tray_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
