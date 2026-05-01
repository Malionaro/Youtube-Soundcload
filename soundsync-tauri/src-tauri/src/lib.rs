use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ─── App State ───────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppState {
    pub is_downloading: Mutex<bool>,
    pub abort_flag: Mutex<bool>,
    pub config: Mutex<AppConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub download_folder: String,
    pub cookies_path: String,
    pub language: String,
    #[serde(default)]
    pub disable_changelog: bool,
    #[serde(default = "default_auto_url_detection")]
    pub auto_url_detection: bool,
}

fn default_auto_url_detection() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs_next().unwrap_or_else(|| PathBuf::from("."));
        Self {
            download_folder: home.to_string_lossy().to_string(),
            cookies_path: String::new(),
            language: "en".to_string(),
            disable_changelog: false,
            auto_url_detection: true,
        }
    }
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

// ─── Data Structures ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub status: String, // "downloading", "converting", "finished", "error"
    pub percent: f64,
    pub speed: String,
    pub eta: String,
    pub title: String,
    pub current: usize,
    pub total: usize,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertProgress {
    pub status: String,
    pub percent: f64,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub format: String,
    pub folder: String,
    pub cookies_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertRequest {
    pub input_path: String,
    pub output_format: String,
    pub quality: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCheckResult {
    pub ffmpeg_installed: bool,
    pub ffmpeg_version: String,
    pub ytdlp_installed: bool,
    pub ytdlp_version: String,
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
fn load_config(state: State<AppState>) -> Result<AppConfig, String> {
    let config_path = get_config_path();
    if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        let config: AppConfig = serde_json::from_str(&content).unwrap_or_default();
        let mut state_config = state.config.lock().unwrap();
        *state_config = config.clone();
        Ok(config)
    } else {
        let config = AppConfig::default();
        let mut state_config = state.config.lock().unwrap();
        *state_config = config.clone();
        Ok(config)
    }
}

#[tauri::command]
fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    let config_path = get_config_path();
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&config_path, json).map_err(|e| e.to_string())?;
    let mut state_config = state.config.lock().unwrap();
    *state_config = config;
    Ok(())
}

#[tauri::command]
fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.get_text().map_err(|e| e.to_string())
}

#[tauri::command]
fn check_system() -> SystemCheckResult {
    let (ffmpeg_installed, ffmpeg_version) = check_tool("ffmpeg", &["-version"]);
    let (ytdlp_installed, ytdlp_version) = check_tool("yt-dlp", &["--version"]);

    SystemCheckResult {
        ffmpeg_installed,
        ffmpeg_version,
        ytdlp_installed,
        ytdlp_version,
    }
}

#[tauri::command]
async fn get_playlist_info(
    url: String,
    cookies_path: Option<String>,
) -> Result<PlaylistInfo, String> {
    let mut args = vec![
        "--flat-playlist".to_string(),
        "--dump-json".to_string(),
        "--no-warnings".to_string(),
        "-i".to_string(),
        "--no-color".to_string(),
    ];

    if let Some(ref cp) = cookies_path {
        if !cp.is_empty() && std::path::Path::new(cp).exists() {
            args.push("--cookies".to_string());
            args.push(cp.clone());
        }
    }

    args.push(url.clone());

    let output = tokio::task::spawn_blocking(move || Command::new("yt-dlp").args(&args).output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("yt-dlp not found: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && stdout.is_empty() {
        return Err(format!("yt-dlp error: {}", stderr));
    }

    let mut entries = Vec::new();
    let mut playlist_title = String::from("Downloads");

    for (idx, line) in stdout.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(info) = serde_json::from_str::<serde_json::Value>(line) {
            let title = info
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("Track {}", idx + 1))
                .to_string();

            let entry_url = info
                .get("url")
                .or_else(|| info.get("webpage_url"))
                .or_else(|| info.get("original_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let thumbnail = info
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    info.get("thumbnails")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.last())
                        .and_then(|v| v.get("url"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });

            let duration = info.get("duration").and_then(|v| v.as_f64());

            if idx == 0 {
                if let Some(pt) = info.get("playlist_title").and_then(|v| v.as_str()) {
                    playlist_title = pt.to_string();
                }
            }

            entries.push(PlaylistEntry {
                title,
                url: entry_url,
                thumbnail,
                duration,
                index: idx + 1,
            });
        }
    }

    let total = entries.len();
    Ok(PlaylistInfo {
        title: playlist_title,
        entries,
        total,
    })
}

#[tauri::command]
async fn download_track(
    app: AppHandle,
    request: DownloadRequest,
    state: State<'_, AppState>,
    track_index: usize,
    total_tracks: usize,
    track_title: String,
) -> Result<String, String> {
    // Check if aborted
    {
        let abort = state.abort_flag.lock().unwrap();
        if *abort {
            return Err("Cancelled".to_string());
        }
    }

    let app_handle = app.clone();
    let folder = request.folder.clone();
    let format = request.format.clone();
    let url = request.url.clone();
    let cookies = request.cookies_path.clone();

    // Determine if audio or video format
    let audio_formats = [
        "mp3", "m4a", "wav", "flac", "aac", "ogg", "opus", "wma", "alac", "aiff",
    ];
    let is_audio = audio_formats.contains(&format.as_str());

    let mut args: Vec<String> = vec![
        "--newline".to_string(),
        "--no-color".to_string(),
        "--progress".to_string(),
        "--progress-template".to_string(),
        "%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s".to_string(),
        "-o".to_string(),
        format!("{}/%(title)s.%(ext)s", folder),
        "--extractor-args".to_string(),
        "youtube:player_client=android".to_string(), // Bypass some DRM
    ];

    if is_audio {
        let codec = match format.as_str() {
            "ogg" => "vorbis",
            _ => &format,
        };
        args.extend([
            "-f".to_string(),
            "bestaudio/best".to_string(),
            "-x".to_string(),
            "--audio-format".to_string(),
            codec.to_string(),
            "--audio-quality".to_string(),
            "0".to_string(),
        ]);
    } else {
        args.extend([
            "-f".to_string(),
            "bestvideo+bestaudio/best".to_string(),
            "--merge-output-format".to_string(),
            format.clone(),
        ]);
    }

    if let Some(ref cp) = cookies {
        if !cp.is_empty() && std::path::Path::new(cp).exists() {
            args.extend(["--cookies".to_string(), cp.clone()]);
        }
    }

    args.push(url.clone());

    // Spawn download in background
    let result = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        use std::process::Stdio;

        #[allow(unused_mut)]
        let mut cmd = Command::new("yt-dlp");
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Err(format!("Failed to start yt-dlp: {}", e));
            }
        };

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            let mut current_title = track_title.clone();

            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim();

                    // Parse download destination
                    if trimmed.starts_with("[download] Destination:") {
                        current_title = trimmed
                            .replace("[download] Destination:", "")
                            .trim()
                            .to_string();
                        if let Some(name) = std::path::Path::new(&current_title).file_stem() {
                            current_title = name.to_string_lossy().to_string();
                        }
                    }

                    // Parse progress
                    if trimmed.contains('%') && trimmed.contains('|') {
                        let parts: Vec<&str> = trimmed.split('|').collect();
                        if parts.len() >= 3 {
                            let percent_str = parts[0].trim().replace('%', "");
                            let percent: f64 = percent_str.trim().parse().unwrap_or(0.0);
                            let speed = parts[1].trim().to_string();
                            let eta = parts[2].trim().to_string();

                            let _ = app_handle.emit(
                                "download-progress",
                                DownloadProgress {
                                    status: "downloading".to_string(),
                                    percent,
                                    speed,
                                    eta,
                                    title: current_title.clone(),
                                    current: track_index,
                                    total: total_tracks,
                                    filename: current_title.clone(),
                                },
                            );
                        }
                    }

                    // Track converting
                    if trimmed.starts_with("[ExtractAudio]") || trimmed.starts_with("[Merger]") {
                        let _ = app_handle.emit(
                            "download-progress",
                            DownloadProgress {
                                status: "converting".to_string(),
                                percent: 100.0,
                                speed: String::new(),
                                eta: String::new(),
                                title: current_title.clone(),
                                current: track_index,
                                total: total_tracks,
                                filename: current_title.clone(),
                            },
                        );
                    }

                    // Emit log line
                    let _ = app_handle.emit("download-log", trimmed.to_string());
                }
            }
        }

        // Check stderr for errors
        if let Some(stderr) = child.stderr.take() {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim();
                    let _ = app_handle.emit("download-log", trimmed.to_string());
                }
            }
        }

        let status = child.wait();
        match status {
            Ok(s) if s.success() => Ok("Success".to_string()),
            Ok(s) => Err(format!(
                "yt-dlp exited with code: {}",
                s.code().unwrap_or(-1)
            )),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}

#[tauri::command]
async fn cancel_download(state: State<'_, AppState>) -> Result<(), String> {
    let mut abort = state.abort_flag.lock().unwrap();
    *abort = true;
    let mut downloading = state.is_downloading.lock().unwrap();
    *downloading = false;
    Ok(())
}

#[tauri::command]
async fn convert_file(app: AppHandle, request: ConvertRequest) -> Result<String, String> {
    let input = request.input_path.clone();
    let output_format = request.output_format.clone();
    let quality = request.quality.clone();

    let input_path = std::path::Path::new(&input);
    if !input_path.exists() {
        return Err("Input file does not exist".to_string());
    }

    let stem = input_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let parent = input_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let output_path = format!("{}/{}_converted.{}", parent, stem, output_format);

    let mut args: Vec<String> = vec!["-i".to_string(), input.clone(), "-y".to_string()];

    // Quality mapping
    match quality.as_str() {
        "low" => {
            args.extend(["-b:a".to_string(), "96k".to_string()]);
        }
        "medium" => {
            args.extend(["-b:a".to_string(), "192k".to_string()]);
        }
        "high" => {
            args.extend(["-b:a".to_string(), "320k".to_string()]);
        }
        "max" => {
            args.extend([
                "-b:a".to_string(),
                "500k".to_string(),
                "-b:v".to_string(),
                "8000k".to_string(),
            ]);
        }
        _ => {}
    }

    args.push(output_path.clone());

    let app_handle = app.clone();

    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        use std::process::Stdio;

        #[allow(unused_mut)]
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = app_handle.emit("convert-error", format!("FFmpeg not found: {}", e));
                return;
            }
        };

        // FFmpeg outputs progress to stderr
        if let Some(stderr) = child.stderr.take() {
            let reader = std::io::BufReader::new(stderr);
            let mut duration_secs: f64 = 0.0;

            for line in reader.lines() {
                if let Ok(line) = line {
                    // Parse duration
                    if line.contains("Duration:") {
                        if let Some(dur) = parse_ffmpeg_duration(&line) {
                            duration_secs = dur;
                        }
                    }

                    // Parse time progress
                    if line.contains("time=") {
                        if let Some(current) = parse_ffmpeg_time(&line) {
                            let percent = if duration_secs > 0.0 {
                                (current / duration_secs * 100.0).min(100.0)
                            } else {
                                0.0
                            };

                            let _ = app_handle.emit(
                                "convert-progress",
                                ConvertProgress {
                                    status: "converting".to_string(),
                                    percent,
                                    filename: stem.clone(),
                                },
                            );
                        }
                    }

                    let _ = app_handle.emit("convert-log", line);
                }
            }
        }

        let status = child.wait();
        match status {
            Ok(s) if s.success() => {
                let _ = app_handle.emit("convert-finished", output_path);
            }
            _ => {
                let _ = app_handle.emit("convert-error", "Conversion failed");
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok("Conversion started".to_string())
}

#[tauri::command]
async fn install_ffmpeg() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("winget")
            .args([
                "install",
                "--id=Gyan.FFmpeg",
                "-e",
                "--silent",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ])
            .output()
            .map_err(|e| format!("winget not available: {}", e))?;

        if output.status.success() {
            Ok("FFmpeg installed successfully".to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("sudo")
            .args(["apt-get", "install", "-y", "ffmpeg"])
            .output()
            .map_err(|e| format!("Failed: {}", e))?;

        if output.status.success() {
            Ok("FFmpeg installed successfully".to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("brew")
            .args(["install", "ffmpeg"])
            .output()
            .map_err(|e| format!("Homebrew not available: {}", e))?;

        if output.status.success() {
            Ok("FFmpeg installed successfully".to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

#[tauri::command]
async fn install_ytdlp() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        // Install or upgrade yt-dlp using winget
        let output = Command::new("winget")
            .args([
                "install",
                "--id=yt-dlp.yt-dlp",
                "-e",
                "--silent",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ])
            .output()
            .map_err(|e| format!("winget not available: {}", e))?;

        if output.status.success()
            || String::from_utf8_lossy(&output.stdout).contains("already installed")
        {
            Ok("yt-dlp installed successfully".to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Auto-install for yt-dlp is currently only supported on Windows".to_string())
    }
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn get_config_path() -> PathBuf {
    let mut path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("config.json");
    path
}

fn check_tool(name: &str, args: &[&str]) -> (bool, String) {
    match Command::new(name).args(args).output() {
        Ok(output) => {
            let version_text = String::from_utf8_lossy(&output.stdout);
            let first_line = version_text.lines().next().unwrap_or("unknown").to_string();
            (output.status.success(), first_line)
        }
        Err(_) => (false, String::new()),
    }
}

fn parse_ffmpeg_duration(line: &str) -> Option<f64> {
    // Parse "Duration: HH:MM:SS.ms"
    if let Some(pos) = line.find("Duration:") {
        let rest = &line[pos + 9..];
        let parts: Vec<&str> = rest.trim().split(',').collect();
        if let Some(time_str) = parts.first() {
            return parse_time_to_seconds(time_str.trim());
        }
    }
    None
}

fn parse_ffmpeg_time(line: &str) -> Option<f64> {
    // Parse "time=HH:MM:SS.ms"
    if let Some(pos) = line.find("time=") {
        let rest = &line[pos + 5..];
        let time_str: String = rest
            .chars()
            .take_while(|c| *c != ' ' && *c != '\r' && *c != '\n')
            .collect();
        return parse_time_to_seconds(&time_str);
    }
    None
}

fn parse_time_to_seconds(time_str: &str) -> Option<f64> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() == 3 {
        let hours: f64 = parts[0].parse().ok()?;
        let minutes: f64 = parts[1].parse().ok()?;
        let seconds: f64 = parts[2].parse().ok()?;
        Some(hours * 3600.0 + minutes * 60.0 + seconds)
    } else {
        None
    }
}

use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

// Icon updated: 2026-05-01
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ✅ WebView2 Logging deaktivieren (muss VOR Builder passieren!)
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-logging --log-level=3"
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
                .text("show_ui", "UI öffnen")
                .separator()
                .text("quit", "Beenden")
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show_ui" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
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
            read_clipboard_text,
            check_system,
            get_playlist_info,
            download_track,
            cancel_download,
            convert_file,
            install_ffmpeg,
            install_ytdlp,
            open_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
