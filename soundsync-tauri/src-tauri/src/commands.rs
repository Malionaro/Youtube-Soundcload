use crate::models::*;
use crate::utils::*;
use crate::spotify::*;
use crate::applemusic::*;

use tauri::{AppHandle, Emitter, State, Manager};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const DISCORD_CLIENT_ID: &str = "1510224184335405118";

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[tauri::command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, String> {
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
pub fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
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
pub fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.get_text().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_discord_presence(
    state: State<'_, AppState>,
    details: String,
    state_msg: String,
) -> Result<(), String> {
    use discord_rich_presence::DiscordIpc;

    let discord_rpc_enabled = { state.config.lock().unwrap().discord_rpc };
    let details = sanitize_discord_text(&details, "SoundSync Downloader");
    let state_msg = sanitize_discord_text(&state_msg, "Idle");

    if !discord_rpc_enabled || details.is_empty() || state_msg.is_empty() {
        let mut client_lock = state.discord_client.lock().unwrap();
        if let Some(mut client) = client_lock.take() {
            let _ = client.client.close();
        }
        return Ok(());
    }

    let mut client_lock = state.discord_client.lock().unwrap();

    if client_lock.is_none() {
        *client_lock = Some(connect_discord_client()?);
    }

    if let Some(discord) = client_lock.as_mut() {
        use discord_rich_presence::activity;

        let assets = activity::Assets::new().large_image("logo");
        let buttons = vec![activity::Button::new(
            "Download",
            "https://github.com/Malionaro/Johann-Youtube-Soundcload",
        )];
        let activity = activity::Activity::new()
            .details(&details)
            .state(&state_msg)
            .assets(assets)
            .buttons(buttons)
            .timestamps(activity::Timestamps::new().start(discord.start_time));

        if let Err(e) = discord.client.set_activity(activity) {
            let first_error = e.to_string();
            *client_lock = Some(connect_discord_client()?);

            if let Some(discord) = client_lock.as_mut() {
                let assets = activity::Assets::new().large_image("logo");
                let buttons = vec![activity::Button::new(
                    "Download",
                    "https://github.com/Malionaro/Johann-Youtube-Soundcload",
                )];
                let activity = activity::Activity::new()
                    .details(&details)
                    .state(&state_msg)
                    .assets(assets)
                    .buttons(buttons)
                    .timestamps(activity::Timestamps::new().start(discord.start_time));

                if let Err(e) = discord.client.set_activity(activity) {
                    *client_lock = None;
                    return Err(format!("{}; reconnect failed: {}", first_error, e));
                }
            }
        }
    }

    Ok(())
}

fn connect_discord_client() -> Result<DiscordClient, String> {
    use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

    validate_discord_application_id()?;

    let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID);
    client.connect().map_err(|e| e.to_string())?;

    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    Ok(DiscordClient { client, start_time })
}

fn validate_discord_application_id() -> Result<(), String> {
    let url = format!(
        "https://discord.com/api/v10/oauth2/applications/{}/rpc",
        DISCORD_CLIENT_ID
    );

    let result = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .call();

    match result {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(404, response)) => {
            let body = response.into_string().unwrap_or_default();
            if body.contains("Unknown Application") {
                Err(format!(
                    "Discord App-ID {} ist unbekannt. Rich Presence braucht eine gültige Discord Developer Application Client ID.",
                    DISCORD_CLIENT_ID
                ))
            } else {
                Err(format!("Discord App-ID {} wurde nicht gefunden.", DISCORD_CLIENT_ID))
            }
        }
        Err(ureq::Error::Status(status, _)) => Err(format!(
            "Discord App-ID {} konnte nicht geprüft werden (HTTP {}).",
            DISCORD_CLIENT_ID, status
        )),
        Err(_) => Ok(()),
    }
}

fn sanitize_discord_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let source = if trimmed.is_empty() { fallback } else { trimmed };
    source.chars().take(128).collect()
}

#[tauri::command]
pub fn check_system() -> SystemCheckResult {
    let (ffmpeg_installed, ffmpeg_version) = check_tool("ffmpeg", &["-version"]);
    let (ytdlp_installed, ytdlp_version) = check_tool("yt-dlp", &["--version"]);
    let (pot_provider_installed, pot_provider_status) = check_pot_provider_files();

    SystemCheckResult {
        ffmpeg_installed,
        ffmpeg_version,
        ytdlp_installed,
        ytdlp_version,
        pot_provider_installed,
        pot_provider_status,
    }
}

#[cfg(target_os = "windows")]
fn check_pot_provider_files() -> (bool, String) {
    let appdata = match std::env::var("APPDATA") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return (false, "APPDATA not found".to_string()),
    };
    let home = match std::env::var("USERPROFILE") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return (false, "USERPROFILE not found".to_string()),
    };

    let plugin_zip = appdata
        .join("yt-dlp-plugins")
        .join("bgutil-ytdlp-pot-provider.zip");
    let provider_script = home
        .join("bgutil-ytdlp-pot-provider")
        .join("server")
        .join("build")
        .join("generate_once.js");
    let (node_available, node_version) = check_tool("node", &["--version"]);

    if plugin_zip.exists() && provider_script.exists() && node_available {
        (
            true,
            format!(
                "Active: Plugin installed, provider script built, Node {}",
                node_version
            ),
        )
    } else {
        let mut missing = Vec::new();
        if !plugin_zip.exists() {
            missing.push("Plugin ZIP");
        }
        if !provider_script.exists() {
            missing.push("Provider Script");
        }
        if !node_available {
            missing.push("Node.js");
        }
        (
            false,
            format!("Not set up: {} missing", missing.join(", ")),
        )
    }
}

#[cfg(not(target_os = "windows"))]
fn check_pot_provider_files() -> (bool, String) {
    (
        false,
        "Auto-setup for PO-Token Provider is currently only supported on Windows".to_string(),
    )
}

#[tauri::command]
pub async fn get_playlist_info(
    url: String,
    cookies_path: Option<String>,
) -> Result<PlaylistInfo, String> {
    let mut resolved_url = url.clone();

    if resolved_url.contains("spotify.com") {
        if resolved_url.contains("/playlist/") || resolved_url.contains("/album/") {
            return resolve_spotify_playlist(&resolved_url).await;
        } else {
            resolved_url = resolve_spotify_url(&resolved_url).await?;
        }
    } else if resolved_url.contains("music.apple.com") {
        return resolve_apple_music_playlist(&resolved_url).await;
    }
    let mut args = vec![
        "--flat-playlist".to_string(),
        "--dump-json".to_string(),
        "--no-warnings".to_string(),
        "-i".to_string(),
        "--no-color".to_string(),
        "--windows-filenames".to_string(),
        "--extractor-args".to_string(),
        "youtube:player_client=android".to_string(),
    ];

    if let Some(ref cp) = cookies_path {
        if !cp.is_empty() && std::path::Path::new(cp).exists() {
            args.push("--cookies".to_string());
            args.push(cp.clone());
        }
    }

    args.push(resolved_url.clone());

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
pub async fn search_videos(query: String) -> Result<PlaylistInfo, String> {
    let resolved_url = format!("ytsearch20:{}", query);
    get_playlist_info(resolved_url, None).await
}

#[tauri::command]
pub async fn search_playlists(query: String) -> Result<PlaylistInfo, String> {
    let resolved_url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAw%253D%253D",
        urlencoding::encode(&query)
    );
    get_playlist_info(resolved_url, None).await
}

#[tauri::command]
pub async fn get_trending_videos() -> Result<PlaylistInfo, String> {
    // YouTube Music Trending Charts (Top 100 Music Videos Global)
    let url = "https://www.youtube.com/playlist?list=PL4fGSI1pDJn6jXS_Tv_N9B8Z0HTRVJE0m".to_string();
    get_playlist_info(url, None).await
}

#[tauri::command]
pub async fn reset_download_cancel(state: State<'_, AppState>) -> Result<(), String> {
    let mut abort = state.abort_flag.lock().unwrap();
    *abort = false;
    let mut downloading = state.is_downloading.lock().unwrap();
    *downloading = true;
    Ok(())
}

#[tauri::command]
pub async fn download_track(
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
        "--windows-filenames".to_string(),
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

    // AI Tagging & Metadata
    let auto_tagging = { state.config.lock().unwrap().auto_tagging };
    if auto_tagging {
        args.extend([
            "--embed-metadata".to_string(),
            "--embed-thumbnail".to_string(),
            "--convert-thumbnails".to_string(),
            "jpg".to_string(),
        ]);
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

        let mut downloaded_paths: Vec<String> = Vec::new();
        let mut already_downloaded = false;

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
                        if !downloaded_paths.contains(&_final_path) {
                            downloaded_paths.push(_final_path.clone());
                        }
                    } else if trimmed.starts_with("[ExtractAudio] Destination:") {
                        _final_path = trimmed
                            .replace("[ExtractAudio] Destination:", "")
                            .trim()
                            .to_string();
                        if !downloaded_paths.contains(&_final_path) {
                            downloaded_paths.push(_final_path.clone());
                        }
                    } else if trimmed.starts_with("[Merger] Merging formats into \"") {
                        let path = trimmed.replace("[Merger] Merging formats into \"", "");
                        let path = path.trim_end_matches('"').to_string();
                        if !downloaded_paths.contains(&path) {
                            downloaded_paths.push(path);
                        }
                    }

                    // Also check for existing file
                    if trimmed.contains("has already been downloaded") {
                        // Extract path between [download] and has already...
                        if let Some(start) = trimmed.find(" ") {
                            if let Some(end) = trimmed.find(" has already") {
                                _final_path = trimmed[start..end].trim().to_string();
                                if !downloaded_paths.contains(&_final_path) {
                                    downloaded_paths.push(_final_path.clone());
                                }
                            }
                        }
                        already_downloaded = true;
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
                            let percent_str: String = prefix
                                .chars()
                                .filter(|c| c.is_ascii_digit() || *c == '.')
                                .collect();
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
                    if (trimmed.starts_with("[ExtractAudio]")
                        || trimmed.starts_with("[Merger]")
                        || trimmed.starts_with("[VideoConvertor]")
                        || trimmed.starts_with("[Fixup"))
                        && !trimmed.contains("Not converting")
                    {
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

                    // Emit log line (skip frequent progress updates to prevent lag/memory issues)
                    if !(trimmed.starts_with("[download]") && trimmed.contains('%')) {
                        let _ = app_handle.emit("download-log", trimmed.to_string());
                    }
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

        let is_aborted = {
            let state_val = app_handle.state::<AppState>();
            let abort = *state_val.abort_flag.lock().unwrap();
            abort
        };

        if is_aborted && !already_downloaded {
            for path in &downloaded_paths {
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_file(format!("{}.part", path));
                let _ = std::fs::remove_file(format!("{}.ytdl", path));
            }
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
pub async fn cancel_download(state: State<'_, AppState>) -> Result<(), String> {
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

#[tauri::command]
pub async fn convert_file(app: AppHandle, request: ConvertRequest) -> Result<String, String> {
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

    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let output_path = if input_ext.eq_ignore_ascii_case(&output_format) {
        format!("{}/{}_converted.{}", parent, stem, output_format)
    } else {
        format!("{}/{}.{}", parent, stem, output_format)
    };

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
pub async fn install_ffmpeg() -> Result<String, String> {
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

        let output_text = command_output_text(&output);
        if output.status.success() || winget_output_means_installed(&output_text) {
            Ok("FFmpeg ist bereits installiert oder wurde erfolgreich installiert.".to_string())
        } else {
            Err(format_winget_error(
                "FFmpeg",
                output.status.code(),
                &output_text,
            ))
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
pub async fn install_ytdlp() -> Result<String, String> {
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

        let output_text = command_output_text(&output);
        if output.status.success() || winget_output_means_installed(&output_text) {
            Ok("yt-dlp installed successfully".to_string())
        } else {
            Err(format_winget_error(
                "yt-dlp",
                output.status.code(),
                &output_text,
            ))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Auto-install for yt-dlp is currently only supported on Windows".to_string())
    }
}

#[tauri::command]
pub async fn install_pot_provider() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
$ErrorActionPreference = 'Stop'
$logFile = Join-Path $env:TEMP 'soundsync-pot-provider-install.log'
"SoundSync PO-Token-Provider Setup gestartet: $(Get-Date)" | Out-File -FilePath $logFile -Encoding UTF8
Write-Output "Setup-Log: $logFile"

function Add-NodePath {
  $nodeDir = Join-Path $env:ProgramFiles 'nodejs'
  if (Test-Path $nodeDir) {
    $env:Path = "$env:Path;$nodeDir"
  }
}

Write-Output 'Pruefe Node.js...'
Add-NodePath
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  Write-Output 'Node.js fehlt. Installiere OpenJS.NodeJS.LTS per winget...'
  winget install --id=OpenJS.NodeJS.LTS -e --silent --accept-source-agreements --accept-package-agreements
  Add-NodePath
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw 'Node.js wurde installiert, ist aber in diesem Prozess noch nicht im PATH. Bitte App oder Windows neu starten und Setup erneut ausfuehren.'
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  Add-NodePath
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw 'npm wurde nicht gefunden. Bitte Node.js LTS installieren oder App neu starten.'
}

$pluginDir = Join-Path $env:APPDATA 'yt-dlp-plugins'
New-Item -ItemType Directory -Force -Path $pluginDir | Out-Null
$pluginZip = Join-Path $pluginDir 'bgutil-ytdlp-pot-provider.zip'
$release = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/Brainicism/bgutil-ytdlp-pot-provider/releases/latest'
$tag = $release.tag_name
if (-not $tag) {
  throw 'Neueste bgutil Release konnte nicht ermittelt werden.'
}
Write-Output "Lade yt-dlp Plugin Release $tag nach $pluginZip ..."
Invoke-WebRequest -UseBasicParsing 'https://github.com/Brainicism/bgutil-ytdlp-pot-provider/releases/latest/download/bgutil-ytdlp-pot-provider.zip' -OutFile $pluginZip

$providerDir = Join-Path $env:USERPROFILE 'bgutil-ytdlp-pot-provider'
$tmpRoot = Join-Path $env:TEMP ('soundsync-bgutil-' + [guid]::NewGuid().ToString())
$sourceZip = Join-Path $tmpRoot 'source.zip'
New-Item -ItemType Directory -Force -Path $tmpRoot | Out-Null
Write-Output "Lade Provider-Quellcode nach $tmpRoot ..."
Invoke-WebRequest -UseBasicParsing "https://github.com/Brainicism/bgutil-ytdlp-pot-provider/archive/refs/tags/$tag.zip" -OutFile $sourceZip
Expand-Archive -Force -Path $sourceZip -DestinationPath $tmpRoot
$extracted = Get-ChildItem $tmpRoot -Directory | Where-Object { $_.Name -like 'bgutil-ytdlp-pot-provider-*' } | Select-Object -First 1
if (-not $extracted) {
  throw 'Provider-Quellcode konnte nicht entpackt werden.'
}
if (Test-Path $providerDir) {
  Remove-Item -LiteralPath $providerDir -Recurse -Force
}
Move-Item -LiteralPath $extracted.FullName -Destination $providerDir

$serverDir = Join-Path $providerDir 'server'
Write-Output "Installiere Provider-Abhaengigkeiten in $serverDir ..."
Push-Location $serverDir
npm install
Write-Output 'Baue Provider-Script...'
npx tsc
Pop-Location

$scriptPath = Join-Path $serverDir 'build\generate_once.js'
if (-not (Test-Path $scriptPath)) {
  throw "Provider-Script wurde nicht erzeugt: $scriptPath"
}

Remove-Item -LiteralPath $tmpRoot -Recurse -Force
Write-Output 'PO-Token-Provider wurde eingerichtet.'
"SoundSync PO-Token-Provider Setup abgeschlossen: $(Get-Date)" | Add-Content -Path $logFile -Encoding UTF8
"#;

        fn ps_quote(value: &str) -> String {
            format!("'{}'", value.replace('\'', "''"))
        }

        let temp_dir = std::env::temp_dir();
        let unique = format!(
            "soundsync-pot-provider-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let install_script_path = temp_dir.join(format!("{unique}.ps1"));
        let launcher_script_path = temp_dir.join(format!("{unique}-launcher.ps1"));
        let result_path = temp_dir.join(format!("{unique}.result.txt"));
        let log_path = temp_dir.join("soundsync-pot-provider-install.log");

        let wrapped_script = format!(
            r#"$ErrorActionPreference = 'Stop'
$resultPath = {result_path}
$logFile = {log_path}
try {{
{script}
  'OK' | Set-Content -Path $resultPath -Encoding UTF8
  exit 0
}} catch {{
  "FEHLER: $($_.Exception.Message)" | Add-Content -Path $logFile -Encoding UTF8
  $_ | Out-String | Add-Content -Path $logFile -Encoding UTF8
  "ERROR: $($_.Exception.Message)" | Set-Content -Path $resultPath -Encoding UTF8
  exit 1
}}
"#,
            result_path = ps_quote(&result_path.to_string_lossy()),
            log_path = ps_quote(&log_path.to_string_lossy()),
            script = script
        );

        fs::write(&install_script_path, wrapped_script)
            .map_err(|e| format!("Installationsskript konnte nicht geschrieben werden: {}", e))?;

        let install_script_string = install_script_path.to_string_lossy().to_string();
        let launcher_script_string = launcher_script_path.to_string_lossy().to_string();
        let result_string = result_path.to_string_lossy().to_string();

        let launcher = format!(
            r#"$ErrorActionPreference = 'Stop'
$installScript = {install_script}
$resultPath = {result_path}
Write-Output 'Windows fragt jetzt nach Administratorrechten fuer das PO-Token-Provider Setup.'
$proc = Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File', $installScript) -Verb RunAs -Wait -PassThru
if ($null -eq $proc.ExitCode) {{
  exit 1
}}
if (Test-Path $resultPath) {{
  Get-Content -Path $resultPath
}}
exit $proc.ExitCode
"#,
            install_script = ps_quote(&install_script_string),
            result_path = ps_quote(&result_string),
        );
        fs::write(&launcher_script_path, launcher)
            .map_err(|e| format!("Launcher-Skript konnte nicht geschrieben werden: {}", e))?;

        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &launcher_script_string,
        ])
        .creation_flags(0x08000000);

        let output = cmd
            .output()
            .map_err(|e| format!("PowerShell konnte nicht gestartet werden: {}", e))?;

        let launcher_output = command_output_text(&output);
        let install_log = fs::read_to_string(&log_path).unwrap_or_default();
        let output_text = [
            launcher_output.trim(),
            install_log.trim(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

        let _ = fs::remove_file(&install_script_path);
        let _ = fs::remove_file(&launcher_script_path);
        let _ = fs::remove_file(&result_path);

        if output.status.success() {
            Ok(format!(
                "PO-Token-Provider erfolgreich eingerichtet.\n{}",
                output_text
            ))
        } else {
            Err(format!(
                "PO-Token-Provider Setup fehlgeschlagen.\n{}",
                output_text
            ))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Auto-Setup für PO-Token-Provider ist derzeit nur unter Windows unterstützt.".to_string())
    }
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&path)
            .creation_flags(0x08000000)
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

#[tauri::command]
pub async fn download_and_install_update(
    app: AppHandle,
    download_url: String,
    filename: String,
) -> Result<String, String> {
    use std::io::{Read, Write};

    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(&filename);

    // Download the installer
    let client = std::thread::spawn(move || -> Result<PathBuf, String> {
        let resp = ureq::get(&download_url)
            .call()
            .map_err(|e: ureq::Error| format!("Download failed: {}", e))?;

        let mut file =
            fs::File::create(&file_path).map_err(|e| format!("Failed to create file: {}", e))?;

        let mut reader = resp.into_reader();
        let mut buffer = [0u8; 8192];
        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| format!("Read error: {}", e))?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])
                .map_err(|e| format!("Write error: {}", e))?;
        }

        Ok(file_path)
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())??;

    // Launch the installer
    #[cfg(target_os = "windows")]
    {
        let path_str = client.to_string_lossy().to_string();
        if path_str.ends_with(".msi") {
            Command::new("msiexec")
                .args(["/i", &path_str, "/passive"])
                .creation_flags(0x08000000)
                .spawn()
                .map_err(|e| format!("Failed to start installer: {}", e))?;
        } else {
            Command::new(&path_str)
                .creation_flags(0x08000000)
                .spawn()
                .map_err(|e| format!("Failed to start installer: {}", e))?;
        }
    }

    // Exit the app after a short delay so the installer can replace the files
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        handle.exit(0);
    });

    Ok("Installer started".to_string())
}

#[tauri::command]
pub async fn import_playlist_tracks(url: String) -> Result<PlaylistInfo, String> {
    if url.contains("spotify.com") {
        if url.contains("/playlist/") || url.contains("/album/") {
            return resolve_spotify_playlist(&url).await;
        } else {
            return Err("Please enter a link to a Spotify playlist or album.".to_string());
        }
    } else if url.contains("music.apple.com") {
        return resolve_apple_music_playlist(&url).await;
    }
    Err("Unsupported URL. Please enter a Spotify or Apple Music playlist.".to_string())
}

#[tauri::command]
pub fn execute_after_download_action(action: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        match action.as_str() {
            "shutdown" => {
                Command::new("shutdown")
                    .args(["/s", "/t", "30"])
                    .creation_flags(0x08000000)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            "lock" => {
                Command::new("rundll32.exe")
                    .args(["user32.dll,LockWorkStation"])
                    .creation_flags(0x08000000)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }
    Ok(())
}
