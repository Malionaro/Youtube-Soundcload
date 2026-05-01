use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use std::thread;
use tiny_http::{Server, Response, Method};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ─── App State ───────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppState {
    pub is_downloading: Mutex<bool>,
    pub abort_flag: Mutex<bool>,
    pub active_download_pids: Arc<Mutex<Vec<u32>>>,
    pub config: Mutex<AppConfig>,
    pub discord_client: Mutex<Option<DiscordClient>>,
    pub current_progress: Mutex<Option<DownloadProgress>>,
}

pub struct DiscordClient {
    pub client: discord_rich_presence::DiscordIpcClient,
    pub start_time: i64,
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
    #[serde(default)]
    pub discord_rpc: bool,
    pub accent_color: Option<String>,
    pub custom_background: Option<String>,
    #[serde(default = "default_quality")]
    pub quality: String,
}

fn default_auto_url_detection() -> bool {
    true
}

fn default_quality() -> String {
    "best".to_string()
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
            discord_rpc: false,
            accent_color: None,
            custom_background: None,
            quality: "best".to_string(),
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
pub struct RemotePayload {
    pub url: String,
    pub format: Option<String>,
    #[serde(rename = "autoStart")]
    pub auto_start: Option<bool>,
}

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
    pub quality: String,
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
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
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
async fn update_discord_presence(
    state: State<'_, AppState>,
    details: String,
    state_msg: String,
) -> Result<(), String> {
    use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

    let discord_rpc_enabled = {
        state.config.lock().unwrap().discord_rpc
    };

    if !discord_rpc_enabled {
        // If disabled, ensure client is closed
        let mut client_lock = state.discord_client.lock().unwrap();
        if let Some(mut client) = client_lock.take() {
            let _ = client.client.close();
        }
        return Ok(());
    }

    let mut client_lock = state.discord_client.lock().unwrap();

    // Initialize if needed
    if client_lock.is_none() {
        // DiscordIpcClient::new returns the client directly, not a Result
        let mut client = DiscordIpcClient::new("1334907994863079434");
        if let Err(e) = client.connect() {
            return Err(e.to_string());
        }

        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        *client_lock = Some(DiscordClient { client, start_time });
    }

    if let Some(discord) = client_lock.as_mut() {
        use discord_rich_presence::activity;

        let assets = activity::Assets::new().large_image("logo");
        let activity = activity::Activity::new()
            .details(&details)
            .state(&state_msg)
            .assets(assets)
            .timestamps(activity::Timestamps::new().start(discord.start_time));

        if let Err(e) = discord.client.set_activity(activity) {
            *client_lock = None; // Reset the client on error so it can reconnect next time
            return Err(e.to_string());
        }
    }

    Ok(())
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
        "--extractor-args".to_string(),
        "youtube:player_client=android".to_string(),
    ];

    if let Some(ref cp) = cookies_path {
        if !cp.is_empty() && std::path::Path::new(cp).exists() {
            args.push("--cookies".to_string());
            args.push(cp.clone());
        }
    }

    args.push(url.clone());

    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("yt-dlp");
        cmd.args(&args);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        cmd.output()
    })
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
async fn reset_download_cancel(state: State<'_, AppState>) -> Result<(), String> {
    let mut abort = state.abort_flag.lock().unwrap();
    *abort = false;
    let mut downloading = state.is_downloading.lock().unwrap();
    *downloading = true;
    Ok(())
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
    let quality = request.quality.clone();
    let active_download_pids = state.active_download_pids.clone();

    // Determine if audio or video format
    let audio_formats = [
        "mp3", "m4a", "wav", "flac", "aac", "ogg", "opus", "wma", "alac", "aiff",
    ];
    let is_audio = audio_formats.contains(&format.as_str());

    let mut args: Vec<String> = vec![
        "--newline".to_string(),
        "--no-color".to_string(),
        "--progress".to_string(),
        "-o".to_string(),
        format!("{}/%(title)s.%(ext)s", folder),
        "--extractor-args".to_string(),
        "youtube:player_client=android".to_string(), // Bypass some DRM
        "--postprocessor-args".to_string(),
        "ffmpeg:-threads 0".to_string(),
    ];

    if is_audio {
        let codec = match format.as_str() {
            "ogg" => "vorbis",
            _ => &format,
        };
        
        let audio_quality = match quality.as_str() {
            "best" => "0",
            "good" => "2",
            "worst" => "9",
            _ => "0",
        };

        args.extend([
            "-f".to_string(),
            "bestaudio/best".to_string(),
            "-x".to_string(),
            "--audio-format".to_string(),
            codec.to_string(),
            "--audio-quality".to_string(),
            audio_quality.to_string(),
        ]);
    } else {
        let video_format = match quality.as_str() {
            "4k" => "bestvideo[height<=2160]+bestaudio/best",
            "1080p" => "bestvideo[height<=1080]+bestaudio/best",
            "720p" => "bestvideo[height<=720]+bestaudio/best",
            "480p" => "bestvideo[height<=480]+bestaudio/best",
            "worst" => "worstvideo+worstaudio/worst",
            _ => "bestvideo+bestaudio/best",
        };
        
        args.extend([
            "-f".to_string(),
            video_format.to_string(),
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
        let child_id = child.id();
        {
            let mut pids = active_download_pids.lock().unwrap();
            pids.push(child_id);
        }

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            let mut current_title = track_title.clone();
            let mut _final_path = String::new();

            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim();

                    // Parse download destination
                    if trimmed.starts_with("[download] Destination:") {
                        _final_path = trimmed
                            .replace("[download] Destination:", "")
                            .trim()
                            .to_string();
                        if let Some(name) = std::path::Path::new(&_final_path).file_stem() {
                            current_title = name.to_string_lossy().to_string();
                        }
                    } else if trimmed.starts_with("[ExtractAudio] Destination:") {
                        _final_path = trimmed
                            .replace("[ExtractAudio] Destination:", "")
                            .trim()
                            .to_string();
                    }
                    
                    // Also check for existing file
                    if trimmed.contains("has already been downloaded") {
                         // Extract path between [download] and has already...
                         if let Some(start) = trimmed.find(" ") {
                             if let Some(end) = trimmed.find(" has already") {
                                 _final_path = trimmed[start..end].trim().to_string();
                             }
                         }
                         // Emit 100% progress immediately for existing files
                         let progress = DownloadProgress {
                            status: "downloading".to_string(),
                            percent: 100.0,
                            speed: "Skipped (Exists)".to_string(),
                            eta: "0s".to_string(),
                            title: current_title.clone(),
                            current: track_index,
                            total: total_tracks,
                            filename: current_title.clone(),
                        };
                        let _ = app_handle.emit("download-progress", progress);
                    }

                    // Parse progress from default yt-dlp output
                    if trimmed.starts_with("[download]") && trimmed.contains('%') {
                        if let Some(percent_idx) = trimmed.find('%') {
                            let prefix = &trimmed[..percent_idx];
                            // Extract just the numbers before the %
                            let percent_str: String = prefix.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                            let percent: f64 = percent_str.parse().unwrap_or(0.0);
                            
                            // Extract speed and ETA
                            let mut speed = String::new();
                            let mut eta = String::new();
                            
                            if let Some(at_idx) = trimmed.find(" at ") {
                                if let Some(eta_idx) = trimmed.find(" ETA ") {
                                    if at_idx + 4 < eta_idx {
                                        speed = trimmed[at_idx + 4..eta_idx].trim().to_string();
                                        eta = trimmed[eta_idx + 5..].trim().to_string();
                                    }
                                } else {
                                    speed = trimmed[at_idx + 4..].trim().to_string();
                                }
                            }

                            let progress = DownloadProgress {
                                status: "downloading".to_string(),
                                percent,
                                speed,
                                eta,
                                title: current_title.clone(),
                                current: track_index,
                                total: total_tracks,
                                filename: current_title.clone(),
                            };

                            let _ = app_handle.emit("download-progress", progress.clone());
                            {
                                let state = app_handle.state::<AppState>();
                                *state.current_progress.lock().unwrap() = Some(progress);
                            }
                        }
                    }

                    // Track converting
                    if (trimmed.starts_with("[ExtractAudio]") || trimmed.starts_with("[Merger]") || trimmed.starts_with("[VideoConvertor]") || trimmed.starts_with("[Fixup"))
                       && !trimmed.contains("Not converting") {
                        let progress = DownloadProgress {
                            status: "converting".to_string(),
                            percent: 100.0,
                            speed: String::new(),
                            eta: String::new(),
                            title: current_title.clone(),
                            current: track_index,
                            total: total_tracks,
                            filename: current_title.clone(),
                        };
                        let _ = app_handle.emit("download-progress", progress.clone());
                        {
                            let state = app_handle.state::<AppState>();
                            *state.current_progress.lock().unwrap() = Some(progress);
                        }
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
        {
            let mut pids = active_download_pids.lock().unwrap();
            pids.retain(|pid| *pid != child_id);
        }
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

    {
        let state_val = app.state::<AppState>();
        *state_val.current_progress.lock().unwrap() = None;
    }

    result
}

#[tauri::command]
async fn cancel_download(state: State<'_, AppState>) -> Result<(), String> {
    let mut abort = state.abort_flag.lock().unwrap();
    *abort = true;
    let mut downloading = state.is_downloading.lock().unwrap();
    *downloading = false;

    let pids = {
        let mut active = state.active_download_pids.lock().unwrap();
        let pids = active.clone();
        active.clear();
        pids
    };

    for pid in pids {
        kill_process_tree(pid);
    }

    Ok(())
}

fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .output();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
    }
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

    let mut args: Vec<String> = vec![
        "-hwaccel".to_string(),
        "auto".to_string(),
        "-i".to_string(),
        input.clone(),
        "-threads".to_string(),
        "0".to_string(),
        "-y".to_string(),
    ];

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
        let mut cmd = Command::new("winget");
        cmd.args([
            "install",
            "--id=Gyan.FFmpeg",
            "-e",
            "--silent",
            "--accept-source-agreements",
            "--accept-package-agreements",
        ])
        .creation_flags(0x08000000);

        let output = cmd
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
        let mut cmd = Command::new("winget");
        cmd.args([
            "install",
            "--id=yt-dlp.yt-dlp",
            "-e",
            "--silent",
            "--accept-source-agreements",
            "--accept-package-agreements",
        ])
        .creation_flags(0x08000000);

        let output = cmd
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
#[tauri::command]
fn ss_get_local_ip() -> Result<String, String> {
    use std::net::UdpSocket;
    
    // Wir versuchen eine Verbindung zu einer Dummy-Adresse aufzubauen (8.8.8.8),
    // um herauszufinden, welches Interface das System für den Netzwerkverkehr nutzt.
    // Es wird kein tatsächliches Paket gesendet.
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect("8.8.8.8:80").map_err(|e| e.to_string())?;
    let local_addr = socket.local_addr().map_err(|e| e.to_string())?;
    
    let ip = local_addr.ip();
    
    // Falls wir doch auf einer Hamachi/Virtual-IP (26.x.x.x oder ähnlich) landen,
    // oder falls die Methode fehlschlägt, nutzen wir local_ip() als Fallback.
    if ip.is_loopback() || ip.is_unspecified() {
         return local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .map_err(|e| e.to_string());
    }

    Ok(ip.to_string())
}

#[tauri::command]
fn ss_start_remote_server(app: AppHandle) -> Result<(), String> {
    let server_handle = app.clone();
    thread::spawn(move || {
        let addr = "0.0.0.0:3030";

        let server = match Server::http(addr) {
            Ok(s) => s,
            Err(_) => return,
        };

        for mut request in server.incoming_requests() {
            match (request.method(), request.url()) {
                (&Method::Get, "/") => {
                    let html = include_str!("remote.html");
                    let response = Response::from_string(html)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                    let _ = request.respond(response);
                }
                (&Method::Get, "/status") => {
                    let state = server_handle.state::<AppState>();
                    let progress = state.current_progress.lock().unwrap().clone();
                    let json = serde_json::to_string(&progress).unwrap_or_else(|_| "null".to_string());
                    let response = Response::from_string(json)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                    let _ = request.respond(response);
                }
                (&Method::Post, "/send") => {
                    let mut content = String::new();
                    let _ = request.as_reader().read_to_string(&mut content);

                    if let Ok(payload) = serde_json::from_str::<RemotePayload>(&content) {
                        println!("Remote API received valid payload: {:?}", payload);
                        if let Err(e) = server_handle.emit_to("main", "remote-url-received", payload) {
                            println!("Failed to emit event to main window: {:?}", e);
                        }
                        let _ = request.respond(Response::from_string("OK")
                            .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()));
                    } else {
                        println!("Remote API failed to parse JSON, falling back to simple text. Content: {}", content);
                        // Fallback for simple string URL
                        let url = content.trim().to_string();
                        if !url.is_empty() {
                            let payload = RemotePayload {
                                url,
                                format: None,
                                auto_start: None,
                            };
                            if let Err(e) = server_handle.emit_to("main", "remote-url-received", payload.clone()) {
                                println!("Failed to emit event (fallback) to main window: {:?}", e);
                            }
                            let _ = request.respond(Response::from_string("OK")
                                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()));
                        } else {
                            let _ = request.respond(Response::from_string("Empty URL").with_status_code(400)
                                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()));
                        }
                    }
                }
                (&Method::Options, _) => {
                    let response = Response::from_string("")
                        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"POST, GET, OPTIONS"[..]).unwrap())
                        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..]).unwrap());
                    let _ = request.respond(response);
                }
                _ => {
                    let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
                }
            }
        }
    });
    Ok(())
}
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
            read_clipboard_text,
            reset_download_cancel,
            check_system,
            get_playlist_info,
            download_track,
            cancel_download,
            convert_file,
            install_ffmpeg,
            install_ytdlp,
            open_folder,
            update_discord_presence,
            ss_get_local_ip,
            ss_start_remote_server
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
