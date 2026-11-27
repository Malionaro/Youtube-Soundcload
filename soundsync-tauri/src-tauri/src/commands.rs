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

#[cfg(not(target_os = "windows"))]
fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {} >/dev/null 2>&1", name)])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn linux_install_command(package: &str) -> Result<Vec<String>, String> {
    let candidates: [(&str, Vec<&str>); 4] = [
        ("apt-get", vec!["sudo", "apt-get", "install", "-y", package]),
        ("dnf", vec!["sudo", "dnf", "install", "-y", package]),
        ("pacman", vec!["sudo", "pacman", "-S", "--noconfirm", package]),
        ("zypper", vec!["sudo", "zypper", "--non-interactive", "install", package]),
    ];

    for (bin, args) in candidates {
        if command_exists(bin) {
            return Ok(args.into_iter().map(String::from).collect());
        }
    }

    Err("No supported Linux package manager found. Supported: apt-get, dnf, pacman, zypper.".to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_shell_install(label: &str, command: &str) -> Result<String, String> {
    let output = Command::new("sh")
        .args(["-c", command])
        .output()
        .map_err(|e| format!("Failed to start installer for {}: {}", label, e))?;

    let output_text = command_output_text(&output);
    if output.status.success() {
        Ok(format!("{} installed successfully.\n{}", label, output_text).trim().to_string())
    } else if output_text.trim().is_empty() {
        Err(format!("{} installation failed with exit code {:?}.", label, output.status.code()))
    } else {
        Err(format!("{} installation failed.\n{}", label, output_text))
    }
}

#[tauri::command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, String> {
    let config_path = get_config_path();
    if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => {
                let mut state_config = lock_unpoisoned(&state.config);
                *state_config = config.clone();
                Ok(config)
            }
            Err(e) => {
                // Korruptes JSON sichern statt still zu verwerfen
                let backup = config_path.with_extension("json.corrupt.bak");
                let _ = fs::write(&backup, &content);
                let config = AppConfig::default();
                let mut state_config = lock_unpoisoned(&state.config);
                *state_config = config.clone();
                Err(format!(
                    "Config korrupt (Backup: {}): {}. Defaults geladen.",
                    backup.to_string_lossy(),
                    e
                ))
            }
        }
    } else {
        let config = AppConfig::default();
        let mut state_config = lock_unpoisoned(&state.config);
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
    let mut state_config = lock_unpoisoned(&state.config);
    *state_config = config;
    Ok(())
}

#[tauri::command]
pub fn update_remote_queue(state: State<AppState>, queue: Vec<RemoteQueueItem>) -> Result<(), String> {
    let mut remote_queue = lock_unpoisoned(&state.remote_queue);
    *remote_queue = queue;
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
    let discord_rpc_enabled = { lock_unpoisoned(&state.config).discord_rpc };
    let should_clear = !discord_rpc_enabled || (details.trim().is_empty() && state_msg.trim().is_empty());
    let details = sanitize_discord_text(&details, "SoundSync Downloader");
    let state_msg = sanitize_discord_text(&state_msg, "Idle");
    let discord_arc = state.discord_client.clone();

    tokio::task::spawn_blocking(move || {
        use discord_rich_presence::{activity, DiscordIpc};

        if should_clear {
            let mut client_lock = lock_unpoisoned(&discord_arc);
            if let Some(mut client) = client_lock.take() {
                let _ = client.client.close();
            }
            return Ok(());
        }

        let mut client_lock = lock_unpoisoned(&discord_arc);

        if client_lock.is_none() {
            *client_lock = Some(connect_discord_client()?);
        }

        if let Some(discord) = client_lock.as_mut() {
            let assets = activity::Assets::new().large_image("logo");
            let buttons = vec![activity::Button::new(
                "Download",
                "https://github.com/Malionaro/Youtube-Soundcload",
            )];
            let act = activity::Activity::new()
                .details(&details)
                .state(&state_msg)
                .assets(assets)
                .buttons(buttons)
                .timestamps(activity::Timestamps::new().start(discord.start_time));

            if let Err(e) = discord.client.set_activity(act) {
                let first_error = e.to_string();
                *client_lock = Some(connect_discord_client()?);

                if let Some(discord) = client_lock.as_mut() {
                    let assets = activity::Assets::new().large_image("logo");
                    let buttons = vec![activity::Button::new(
                        "Download",
                        "https://github.com/Malionaro/Youtube-Soundcload",
                    )];
                    let act = activity::Activity::new()
                        .details(&details)
                        .state(&state_msg)
                        .assets(assets)
                        .buttons(buttons)
                        .timestamps(activity::Timestamps::new().start(discord.start_time));

                    if let Err(e) = discord.client.set_activity(act) {
                        *client_lock = None;
                        return Err(format!("{}; reconnect failed: {}", first_error, e));
                    }
                }
            }
        }

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn connect_discord_client() -> Result<DiscordClient, String> {
    use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

    validate_discord_application_id()?;

    let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID);
    client.connect().map_err(|e| e.to_string())?;

    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(DiscordClient { client, start_time })
}

fn validate_discord_application_id() -> Result<(), String> {
    use std::sync::OnceLock;
    static VALIDATED: OnceLock<Result<(), String>> = OnceLock::new();

    VALIDATED
        .get_or_init(|| {
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
        })
        .clone()
}

fn sanitize_discord_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let source = if trimmed.is_empty() { fallback } else { trimmed };
    source.chars().take(128).collect()
}

#[tauri::command]
pub async fn check_system() -> Result<SystemCheckResult, String> {
    tokio::task::spawn_blocking(move || {
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
    })
    .await
    .map_err(|e| e.to_string())
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
    let home = match std::env::var("HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return (false, "HOME not found".to_string()),
    };
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));

    let plugin_zip = config_home
        .join("yt-dlp")
        .join("plugins")
        .join("bgutil-ytdlp-pot-provider.zip");
    let provider_script = data_home
        .join("soundsync")
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
    ];
    #[cfg(target_os = "windows")]
    args.push("--windows-filenames".to_string());

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

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(info) = serde_json::from_str::<serde_json::Value>(line) {
            let next_idx = entries.len();
            let title = playlist_entry_title(&info, next_idx);
            let entry_url = playlist_entry_url(&info);
            if entry_url.is_empty() {
                continue;
            }

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

            if next_idx == 0 {
                if let Some(pt) = info.get("playlist_title").and_then(|v| v.as_str()) {
                    if !pt.trim().is_empty() {
                        playlist_title = pt.to_string();
                    }
                }
            }

            entries.push(PlaylistEntry {
                title,
                url: entry_url,
                thumbnail,
                duration,
                index: next_idx + 1,
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

fn playlist_entry_title(info: &serde_json::Value, idx: usize) -> String {
    for key in ["title", "fulltitle", "track", "alt_title", "display_id"] {
        if let Some(value) = info.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    for key in ["webpage_url", "original_url", "url"] {
        if let Some(value) = info.get(key).and_then(|v| v.as_str()) {
            if let Some(name) = value.trim_end_matches('/').rsplit('/').next() {
                let name = name.replace(['-', '_'], " ");
                let trimmed = name.trim();
                if !trimmed.is_empty() && !trimmed.starts_with("http") {
                    return trimmed.to_string();
                }
            }
        }
    }

    format!("Track {}", idx + 1)
}

fn playlist_entry_url(info: &serde_json::Value) -> String {
    for key in ["webpage_url", "original_url", "url"] {
        if let Some(value) = info.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                return trimmed.to_string();
            }
        }
    }

    // Flat YouTube playlist entries often emit just the video ID in `url`/`id`
    let candidate = info
        .get("url")
        .or_else(|| info.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let ie_key = info
        .get("ie_key")
        .or_else(|| info.get("extractor"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !candidate.is_empty()
        && !candidate.contains('/')
        && ie_key.to_ascii_lowercase().contains("youtube")
    {
        return format!("https://www.youtube.com/watch?v={}", candidate);
    }

    candidate.to_string()
}

#[tauri::command]
pub async fn search_videos(query: String) -> Result<PlaylistInfo, String> {
    let resolved_url = format!("ytsearch20:{}", query);
    get_playlist_info(resolved_url, None).await
}

#[tauri::command]
pub async fn search_playlists(query: String) -> Result<PlaylistInfo, String> {
    let resolved_url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAw%3D%3D",
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
    *lock_unpoisoned(&state.abort_flag) = false;
    *lock_unpoisoned(&state.pause_flag) = false;
    *lock_unpoisoned(&state.is_downloading) = true;
    Ok(())
}

#[tauri::command]
pub async fn finish_download(state: State<'_, AppState>) -> Result<(), String> {
    *lock_unpoisoned(&state.is_downloading) = false;
    *lock_unpoisoned(&state.pause_flag) = false;
    *lock_unpoisoned(&state.current_progress) = None;
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
        let abort = lock_unpoisoned(&state.abort_flag);
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

    let output_template = PathBuf::from(&folder)
        .join("%(title)s.%(ext)s")
        .to_string_lossy()
        .to_string();

    let mut args: Vec<String> = vec![
        "--newline".to_string(),
        "--no-color".to_string(),
        "--progress".to_string(),
        "-o".to_string(),
        output_template,
        "--retries".to_string(),
        "10".to_string(),
        "--fragment-retries".to_string(),
        "10".to_string(),
        "--postprocessor-args".to_string(),
        "ffmpeg:-threads 0".to_string(),
    ];
    #[cfg(target_os = "windows")]
    args.push("--windows-filenames".to_string());

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
        } else if !cp.is_empty() {
            let _ = app.emit(
                "download-log",
                format!("WARNING: Cookies-Datei nicht gefunden, wird ignoriert: {}", cp),
            );
        }
    }

    // AI Tagging & Metadata
    let auto_tagging = { lock_unpoisoned(&state.config).auto_tagging };
    if auto_tagging {
        args.push("--embed-metadata".to_string());
        // Thumbnails cannot be embedded into these containers; yt-dlp fails otherwise
        let no_thumb_formats = ["wav", "aiff", "aac", "wma", "alac"];
        if !no_thumb_formats.contains(&format.as_str()) {
            args.extend([
                "--embed-thumbnail".to_string(),
                "--convert-thumbnails".to_string(),
                "jpg".to_string(),
            ]);
        }
    }

    // SponsorBlock (nur YouTube / ytsearch, nur wenn nicht "off")
    {
        let cfg = lock_unpoisoned(&state.config);
        let mode = cfg.sponsorblock_mode.clone();
        let cats = cfg.sponsorblock_categories.clone();
        drop(cfg);
        let is_youtube = url.contains("youtube.com")
            || url.contains("youtu.be")
            || url.contains("music.youtube.com")
            || url.starts_with("ytsearch");
        if is_youtube && (mode == "mark" || mode == "remove") {
            let safe_cats = sanitize_sponsorblock_categories(&cats);
            if mode == "mark" {
                args.extend(["--sponsorblock-mark".to_string(), safe_cats]);
            } else {
                args.extend(["--sponsorblock-remove".to_string(), safe_cats]);
            }
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
            let mut pids = lock_unpoisoned(&active_download_pids);
            pids.push(child_id);
        }

        let mut downloaded_paths: Vec<String> = Vec::new();
        let mut already_downloaded = false;

        // Drain stderr in parallel; reading it only after stdout can deadlock
        // when yt-dlp fills the stderr pipe buffer with warnings.
        let stderr_handle = child.stderr.take().map(|stderr| {
            let app_handle = app_handle.clone();
            std::thread::spawn(move || {
                let mut tail: Vec<String> = Vec::new();
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    let _ = app_handle.emit("download-log", trimmed.to_string());
                    if !trimmed.is_empty() {
                        tail.push(trimmed.to_string());
                        if tail.len() > 8 {
                            tail.remove(0);
                        }
                    }
                }
                tail
            })
        });

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            let mut current_title = track_title.clone();
            let mut _final_path = String::new();

            for line in reader.lines().map_while(Result::ok) {
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
                    if let (Some(start), Some(end)) = (trimmed.find(' '), trimmed.find(" has already")) {
                        if start < end {
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

                // Parse progress from default yt-dlp output (use token directly before `%` to avoid corrupting percent with fragment counts)
                if trimmed.starts_with("[download]") && trimmed.contains('%') {
                    if let Some(percent_idx) = trimmed.find('%') {
                        let prefix = &trimmed[..percent_idx];
                        let percent: f64 = prefix
                            .split_whitespace()
                            .last()
                            .map(|tok| tok.trim_matches(|c: char| !c.is_ascii_digit() && c != '.'))
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0)
                            .clamp(0.0, 100.0);

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
                            *lock_unpoisoned(&state.current_progress) = Some(progress);
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
                        *lock_unpoisoned(&state.current_progress) = Some(progress);
                    }
                }

                // Emit log line (skip frequent progress updates to prevent lag/memory issues)
                if !(trimmed.starts_with("[download]") && trimmed.contains('%')) {
                    let _ = app_handle.emit("download-log", trimmed.to_string());
                }
            }
        }

        let status = child.wait();
        let stderr_tail: Vec<String> = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        let no_active_downloads_left = {
            let mut pids = lock_unpoisoned(&active_download_pids);
            pids.retain(|pid| *pid != child_id);
            pids.is_empty()
        };

        let is_aborted = {
            let state_val = app_handle.state::<AppState>();
            let abort = *lock_unpoisoned(&state_val.abort_flag);
            abort
        };

        if is_aborted && !already_downloaded {
            for path in &downloaded_paths {
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_file(format!("{}.part", path));
                let _ = std::fs::remove_file(format!("{}.ytdl", path));
            }
        }
        if no_active_downloads_left {
            let state_val = app_handle.state::<AppState>();
            *lock_unpoisoned(&state_val.current_progress) = None;
        }
        match status {
            // Rückgabe = finaler Pfad (falls bekannt) statt nur "Success"
            Ok(s) if s.success() => Ok(downloaded_paths
                .last()
                .cloned()
                .unwrap_or_else(|| "Success".to_string())),
            Ok(s) => {
                let detail = if stderr_tail.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", stderr_tail.join("\n"))
                };
                Err(format!(
                    "yt-dlp exited with code: {}{}",
                    s.code().unwrap_or(-1),
                    detail
                ))
            }
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}

#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>) -> Result<(), String> {
    *lock_unpoisoned(&state.abort_flag) = true;
    *lock_unpoisoned(&state.pause_flag) = false;
    *lock_unpoisoned(&state.is_downloading) = false;
    *lock_unpoisoned(&state.current_progress) = None;

    let pids = {
        let mut active = lock_unpoisoned(&state.active_download_pids);
        let pids = active.clone();
        active.clear();
        pids
    };

    tokio::task::spawn_blocking(move || {
        for pid in pids {
            kill_process_tree(pid);
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(target_os = "windows")]
mod win_process {
    use std::collections::VecDeque;
    use std::ffi::c_void;

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const PROCESS_SUSPEND_RESUME: u32 = 0x0800;
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    type NtProcessFn = unsafe extern "system" fn(*mut c_void) -> i32;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> *mut c_void;
        fn Process32FirstW(h_snapshot: *mut c_void, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(h_snapshot: *mut c_void, lppe: *mut ProcessEntry32W) -> i32;
        fn OpenProcess(
            dw_desired_access: u32,
            b_inherit_handle: i32,
            dw_process_id: u32,
        ) -> *mut c_void;
        fn CloseHandle(h_object: *mut c_void) -> i32;
        fn GetModuleHandleA(lp_module_name: *const u8) -> *mut c_void;
        fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const u8) -> *const c_void;
    }

    fn collect_process_tree(root_pid: u32) -> Vec<u32> {
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if !snap.is_null() && snap != INVALID_HANDLE_VALUE {
                let mut entry: ProcessEntry32W = std::mem::zeroed();
                entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
                if Process32FirstW(snap, &mut entry) != 0 {
                    loop {
                        pairs.push((entry.th32_process_id, entry.th32_parent_process_id));
                        if Process32NextW(snap, &mut entry) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(snap);
            }
        }

        let mut result = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(root_pid);
        while let Some(pid) = queue.pop_front() {
            if !result.contains(&pid) {
                result.push(pid);
                for &(child, parent) in &pairs {
                    if parent == pid && !result.contains(&child) {
                        queue.push_back(child);
                    }
                }
            }
        }
        result
    }

    pub fn set_process_tree_paused(root_pid: u32, paused: bool) -> Result<(), String> {
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast::<u8>());
            if ntdll.is_null() {
                return Err("ntdll.dll could not be located".to_string());
            }
            let proc_name = if paused {
                c"NtSuspendProcess"
            } else {
                c"NtResumeProcess"
            };
            let func_ptr = GetProcAddress(ntdll, proc_name.as_ptr().cast::<u8>());
            if func_ptr.is_null() {
                return Err("NtSuspendProcess/NtResumeProcess not available".to_string());
            }
            let nt_fn: NtProcessFn = std::mem::transmute(func_ptr);

            let pids = collect_process_tree(root_pid);
            let mut root_error: Option<String> = None;

            for pid in pids {
                let handle = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
                if handle.is_null() {
                    if pid == root_pid {
                        root_error = Some(format!("Could not open process PID {}", pid));
                    }
                    continue;
                }
                let status = nt_fn(handle);
                CloseHandle(handle);
                if status != 0 && pid == root_pid {
                    root_error = Some(format!(
                        "Failed to {} PID {} (NTSTATUS 0x{:08x})",
                        if paused { "suspend" } else { "resume" },
                        pid,
                        status as u32
                    ));
                }
            }

            if let Some(err) = root_error {
                Err(err)
            } else {
                Ok(())
            }
        }
    }
}

fn set_download_process_paused(pid: u32, paused: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win_process::set_process_tree_paused(pid, paused)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let signal = if paused { "-STOP" } else { "-CONT" };
        let script = format!("pkill {signal} -P {pid} 2>/dev/null || true; kill {signal} {pid}");
        let output = Command::new("sh")
            .args(["-c", &script])
            .output()
            .map_err(|e| format!("pause/resume command could not be started: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_output_text(&output))
        }
    }
}

#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>) -> Result<(), String> {
    {
        let downloading = lock_unpoisoned(&state.is_downloading);
        if !*downloading {
            return Err("No active download to pause.".to_string());
        }
    }

    let pids = lock_unpoisoned(&state.active_download_pids).clone();
    if pids.is_empty() {
        return Err("No active download process to pause.".to_string());
    }

    let errors = tokio::task::spawn_blocking(move || {
        let mut errs = Vec::new();
        for pid in pids {
            if let Err(e) = set_download_process_paused(pid, true) {
                errs.push(format!("PID {pid}: {e}"));
            }
        }
        errs
    })
    .await
    .map_err(|e| e.to_string())?;

    if errors.is_empty() {
        *lock_unpoisoned(&state.pause_flag) = true;
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[tauri::command]
pub async fn resume_download(state: State<'_, AppState>) -> Result<(), String> {
    let pids = lock_unpoisoned(&state.active_download_pids).clone();
    if pids.is_empty() {
        return Err("No paused download process to resume.".to_string());
    }

    let errors = tokio::task::spawn_blocking(move || {
        let mut errs = Vec::new();
        for pid in pids {
            if let Err(e) = set_download_process_paused(pid, false) {
                errs.push(format!("PID {pid}: {e}"));
            }
        }
        errs
    })
    .await
    .map_err(|e| e.to_string())?;

    if errors.is_empty() {
        *lock_unpoisoned(&state.pause_flag) = false;
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[tauri::command]
pub async fn convert_file(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConvertRequest,
) -> Result<String, String> {
    let input = request.input_path.clone();
    let output_format = request.output_format.trim().to_ascii_lowercase();
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
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let output_filename = if input_ext.eq_ignore_ascii_case(&output_format) {
        format!("{}_converted.{}", stem, output_format)
    } else {
        format!("{}.{}", stem, output_format)
    };
    let output_path = parent.join(output_filename).to_string_lossy().to_string();

    let is_video = is_video_format(&output_format);
    let is_lossless = is_lossless_audio_format(&output_format);
    let gpu_encoder = { lock_unpoisoned(&state.config).gpu_encoder.clone() };

    let mut args: Vec<String> = Vec::new();
    if is_video && gpu_encoder != "off" {
        args.extend(["-hwaccel".to_string(), "auto".to_string()]);
    }
    args.extend([
        "-i".to_string(),
        input.clone(),
        "-threads".to_string(),
        "0".to_string(),
        "-y".to_string(),
    ]);

    if !is_video {
        // Reines Audio-Ausgabeformat: keine Video-Spur neu encodieren (verhindert GPU-Encoder-Fehler!)
        args.push("-vn".to_string());
    } else if gpu_encoder != "auto" && gpu_encoder != "off" {
        match gpu_encoder.as_str() {
            "nvenc_h264" => args.extend([
                "-c:v".to_string(),
                "h264_nvenc".to_string(),
                "-cq".to_string(),
                "23".to_string(),
            ]),
            "nvenc_hevc" => args.extend([
                "-c:v".to_string(),
                "hevc_nvenc".to_string(),
                "-cq".to_string(),
                "23".to_string(),
            ]),
            "qsv_h264" => args.extend([
                "-c:v".to_string(),
                "h264_qsv".to_string(),
                "-global_quality".to_string(),
                "25".to_string(),
            ]),
            "qsv_hevc" => args.extend([
                "-c:v".to_string(),
                "hevc_qsv".to_string(),
                "-global_quality".to_string(),
                "25".to_string(),
            ]),
            "amf_h264" => args.extend([
                "-c:v".to_string(),
                "h264_amf".to_string(),
                "-quality".to_string(),
                "balanced".to_string(),
            ]),
            "amf_hevc" => args.extend([
                "-c:v".to_string(),
                "hevc_amf".to_string(),
                "-quality".to_string(),
                "balanced".to_string(),
            ]),
            "videotoolbox_h264" => args.extend([
                "-c:v".to_string(),
                "h264_videotoolbox".to_string(),
                "-q:v".to_string(),
                "65".to_string(),
            ]),
            "videotoolbox_hevc" => args.extend([
                "-c:v".to_string(),
                "hevc_videotoolbox".to_string(),
                "-q:v".to_string(),
                "65".to_string(),
            ]),
            _ => {}
        }
    }

    // Quality mapping (Audio-Bitrate nur bei verlustbehafteten Formaten, Video-Bitrate nur bei Video)
    if !is_lossless {
        match quality.as_str() {
            "low" => args.extend(["-b:a".to_string(), "96k".to_string()]),
            "medium" => args.extend(["-b:a".to_string(), "192k".to_string()]),
            "high" => args.extend(["-b:a".to_string(), "320k".to_string()]),
            "max" => {
                args.extend(["-b:a".to_string(), "320k".to_string()]);
                if is_video && (gpu_encoder == "auto" || gpu_encoder == "off") {
                    args.extend(["-b:v".to_string(), "8000k".to_string()]);
                }
            }
            _ => {}
        }
    }

    args.push(output_path.clone());

    let app_handle = app.clone();
    let active_pids = state.active_download_pids.clone();

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::io::Read;
        use std::process::Stdio;

        #[allow(unused_mut)]
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("FFmpeg not found: {}", e);
                let _ = app_handle.emit("convert-error", msg.clone());
                return Err(msg);
            }
        };

        let child_id = child.id();
        {
            lock_unpoisoned(&active_pids).push(child_id);
        }

        let mut stderr_tail: Vec<String> = Vec::new();

        // FFmpeg writes progress updates separated by `\r` (not `\n`), so split on both `\r` and `\n`.
        if let Some(mut stderr) = child.stderr.take() {
            let mut buf = [0u8; 4096];
            let mut line_buf: Vec<u8> = Vec::with_capacity(256);
            let mut duration_secs: f64 = 0.0;

            let mut handle_line = |raw: &str| {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return;
                }
                if trimmed.contains("Duration:") {
                    if let Some(dur) = parse_ffmpeg_duration(trimmed) {
                        duration_secs = dur;
                    }
                }
                if trimmed.contains("time=") {
                    if let Some(current) = parse_ffmpeg_time(trimmed) {
                        let percent = if duration_secs > 0.0 {
                            (current / duration_secs * 100.0).clamp(0.0, 100.0)
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
                if !trimmed.starts_with("frame=") && !trimmed.starts_with("size=") {
                    stderr_tail.push(trimmed.to_string());
                    if stderr_tail.len() > 8 {
                        stderr_tail.remove(0);
                    }
                    let _ = app_handle.emit("convert-log", trimmed.to_string());
                }
            };

            loop {
                match stderr.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        for &b in &buf[..n] {
                            if b == b'\r' || b == b'\n' {
                                if !line_buf.is_empty() {
                                    let line = String::from_utf8_lossy(&line_buf).into_owned();
                                    line_buf.clear();
                                    handle_line(&line);
                                }
                            } else {
                                line_buf.push(b);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if !line_buf.is_empty() {
                let line = String::from_utf8_lossy(&line_buf).into_owned();
                handle_line(&line);
            }
        }

        let status = child.wait();
        {
            lock_unpoisoned(&active_pids).retain(|pid| *pid != child_id);
        }

        match status {
            Ok(s) if s.success() => {
                let _ = app_handle.emit("convert-finished", output_path.clone());
                Ok(output_path)
            }
            Ok(s) => {
                let detail = if stderr_tail.is_empty() {
                    format!("FFmpeg conversion failed (exit code {:?})", s.code())
                } else {
                    format!(
                        "FFmpeg conversion failed (exit code {:?}):\n{}",
                        s.code(),
                        stderr_tail.join("\n")
                    )
                };
                let _ = app_handle.emit("convert-error", detail.clone());
                Err(detail)
            }
            Err(e) => {
                let msg = format!("FFmpeg wait error: {}", e);
                let _ = app_handle.emit("convert-error", msg.clone());
                Err(msg)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(result)
}

#[tauri::command]
pub async fn install_ffmpeg() -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
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
            let command = linux_install_command("ffmpeg")?.join(" ");
            run_shell_install("FFmpeg", &command)
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
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_ytdlp() -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            for action in ["upgrade", "install"] {
                let mut cmd = Command::new("winget");
                cmd.args([
                    action,
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
                    return Ok("yt-dlp installed successfully".to_string());
                }
                if action == "upgrade" {
                    let lower = output_text.to_lowercase();
                    if lower.contains("no installed package found")
                        || lower.contains("kein installiertes paket")
                        || lower.contains("not found")
                        || lower.contains("nicht gefunden")
                    {
                        continue;
                    }
                    if lower.contains("no available upgrade")
                        || lower.contains("kein verfügbares upgrade")
                        || lower.contains("kein verfugbares upgrade")
                        || lower.contains("no newer package versions")
                    {
                        return Ok("yt-dlp is up to date".to_string());
                    }
                    continue;
                }
                return Err(format_winget_error(
                    "yt-dlp",
                    output.status.code(),
                    &output_text,
                ));
            }
            Err("yt-dlp installation failed: winget upgrade+install both failed.".to_string())
        }

        #[cfg(target_os = "linux")]
        {
            let command = if command_exists("pipx") {
                "pipx install yt-dlp || pipx upgrade yt-dlp".to_string()
            } else if command_exists("python3") {
                "python3 -m pip install --user -U yt-dlp".to_string()
            } else {
                linux_install_command("yt-dlp")?.join(" ")
            };
            run_shell_install("yt-dlp", &command)
        }

        #[cfg(target_os = "macos")]
        {
            let command = if command_exists("brew") {
                "brew install yt-dlp || brew upgrade yt-dlp".to_string()
            } else if command_exists("python3") {
                "python3 -m pip install --user -U yt-dlp".to_string()
            } else {
                return Err(
                    "Homebrew or python3 is required to install yt-dlp automatically on macOS."
                        .to_string(),
                );
            };
            run_shell_install("yt-dlp", &command)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_ytdlp() -> Result<String, String> {
    let self_update = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--update");
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);

        match cmd.output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Some(if stdout.is_empty() {
                    "yt-dlp is up to date".to_string()
                } else {
                    stdout
                })
            }
            _ => None,
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Some(msg) = self_update {
        return Ok(msg);
    }

    install_ytdlp().await
}

#[tauri::command]
pub async fn install_pot_provider() -> Result<String, String> {
    tokio::task::spawn_blocking(install_pot_provider_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn install_pot_provider_sync() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        fn ps_quote(value: &str) -> String {
            format!("'{}'", value.replace('\'', "''"))
        }

        // Capture current user's profile paths BEFORE UAC elevation so an elevated admin token
        // still installs into the active user's APPDATA / USERPROFILE.
        let target_appdata = std::env::var("APPDATA").unwrap_or_default();
        let target_userprofile = std::env::var("USERPROFILE").unwrap_or_default();

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

$effectiveAppData = if ($targetAppData) { $targetAppData } else { $env:APPDATA }
$effectiveUserProfile = if ($targetUserProfile) { $targetUserProfile } else { $env:USERPROFILE }
$pluginDir = Join-Path $effectiveAppData 'yt-dlp-plugins'
New-Item -ItemType Directory -Force -Path $pluginDir | Out-Null
$pluginZip = Join-Path $pluginDir 'bgutil-ytdlp-pot-provider.zip'
$release = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/Brainicism/bgutil-ytdlp-pot-provider/releases/latest'
$tag = $release.tag_name
if (-not $tag) {
  throw 'Neueste bgutil Release konnte nicht ermittelt werden.'
}
Write-Output "Lade yt-dlp Plugin Release $tag nach $pluginZip ..."
Invoke-WebRequest -UseBasicParsing 'https://github.com/Brainicism/bgutil-ytdlp-pot-provider/releases/latest/download/bgutil-ytdlp-pot-provider.zip' -OutFile $pluginZip

$providerDir = Join-Path $effectiveUserProfile 'bgutil-ytdlp-pot-provider'
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
$targetAppData = {target_appdata}
$targetUserProfile = {target_userprofile}
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
            target_appdata = ps_quote(&target_appdata),
            target_userprofile = ps_quote(&target_userprofile),
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let bootstrap = if cfg!(target_os = "linux") {
            r#"
if ! command -v curl >/dev/null 2>&1; then
  if command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y curl
  elif command -v dnf >/dev/null 2>&1; then sudo dnf install -y curl
  elif command -v pacman >/dev/null 2>&1; then sudo pacman -S --noconfirm curl
  elif command -v zypper >/dev/null 2>&1; then sudo zypper --non-interactive install curl
  fi
fi
if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1 || ! command -v python3 >/dev/null 2>&1; then
  if command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y nodejs npm python3
  elif command -v dnf >/dev/null 2>&1; then sudo dnf install -y nodejs npm python3
  elif command -v pacman >/dev/null 2>&1; then sudo pacman -S --noconfirm nodejs npm python
  elif command -v zypper >/dev/null 2>&1; then sudo zypper --non-interactive install nodejs npm python3
  fi
fi
"#
        } else {
            r#"
if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew is required for automatic Node.js setup on macOS." >&2
  exit 1
fi
if ! command -v curl >/dev/null 2>&1; then brew install curl; fi
if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then brew install node; fi
if ! command -v python3 >/dev/null 2>&1; then brew install python; fi
"#
        };

        let script = format!(
            r#"set -e
{bootstrap}
if ! command -v curl >/dev/null 2>&1; then echo "curl is required." >&2; exit 1; fi
if ! command -v node >/dev/null 2>&1; then echo "Node.js is required." >&2; exit 1; fi
if ! command -v npm >/dev/null 2>&1; then echo "npm is required." >&2; exit 1; fi
if ! command -v python3 >/dev/null 2>&1; then echo "python3 is required." >&2; exit 1; fi

CONFIG_HOME="${{XDG_CONFIG_HOME:-$HOME/.config}}"
DATA_HOME="${{XDG_DATA_HOME:-$HOME/.local/share}}"
PLUGIN_DIR="$CONFIG_HOME/yt-dlp/plugins"
PROVIDER_DIR="$DATA_HOME/soundsync/bgutil-ytdlp-pot-provider"
TMP_ROOT="$(mktemp -d)"
mkdir -p "$PLUGIN_DIR" "$DATA_HOME/soundsync"

TAG="$(curl -fsSL https://api.github.com/repos/Brainicism/bgutil-ytdlp-pot-provider/releases/latest | node -e "let data='';process.stdin.on('data',c=>data+=c);process.stdin.on('end',()=>console.log(JSON.parse(data).tag_name||''));")"
if [ -z "$TAG" ]; then echo "Could not detect latest bgutil release." >&2; exit 1; fi

echo "Downloading yt-dlp bgutil plugin $TAG..."
curl -fL "https://github.com/Brainicism/bgutil-ytdlp-pot-provider/releases/latest/download/bgutil-ytdlp-pot-provider.zip" -o "$PLUGIN_DIR/bgutil-ytdlp-pot-provider.zip"

echo "Downloading provider source $TAG..."
curl -fL "https://github.com/Brainicism/bgutil-ytdlp-pot-provider/archive/refs/tags/$TAG.zip" -o "$TMP_ROOT/source.zip"
python3 - "$TMP_ROOT/source.zip" "$TMP_ROOT" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as z:
    z.extractall(sys.argv[2])
PY

rm -rf "$PROVIDER_DIR"
EXTRACTED="$(find "$TMP_ROOT" -maxdepth 1 -type d -name 'bgutil-ytdlp-pot-provider-*' | head -n 1)"
if [ -z "$EXTRACTED" ]; then echo "Provider source could not be extracted." >&2; exit 1; fi
mv "$EXTRACTED" "$PROVIDER_DIR"

echo "Installing provider dependencies..."
cd "$PROVIDER_DIR/server"
npm install
npx tsc

test -f "$PROVIDER_DIR/server/build/generate_once.js"
rm -rf "$TMP_ROOT"
echo "PO-Token-Provider setup completed."
"#
        );

        run_shell_install("PO-Token-Provider", &script)
    }
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let dir_path = std::path::Path::new(&path);
    if !dir_path.exists() || !dir_path.is_dir() {
        return Err(format!("Ordner existiert nicht: {}", path));
    }
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
    #[allow(unused_variables)] app: AppHandle,
    download_url: String,
    filename: String,
) -> Result<String, String> {
    use std::io::{Read, Write};

    if !crate::server::is_safe_filename(&filename) {
        return Err("Invalid update filename".to_string());
    }
    let allowed_url = download_url.starts_with("https://github.com/Malionaro/Youtube-Soundcload/")
        || download_url.starts_with("https://objects.githubusercontent.com/")
        || download_url.starts_with("https://release-assets.githubusercontent.com/");
    if !allowed_url {
        return Err("Untrusted update download URL".to_string());
    }

    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(&filename);

    // Download the installer without blocking Tokio worker thread
    let client = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        let resp = ureq::get(&download_url)
            .timeout(std::time::Duration::from_secs(120))
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
    .await
    .map_err(|e| format!("Download task failed: {}", e))??;

    // Launch the installer
    #[cfg(target_os = "windows")]
    {
        let path_str = client.to_string_lossy().to_string();
        if path_str.to_ascii_lowercase().ends_with(".msi") {
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
        // Exit the app after a short delay so the installer can replace the files (nur Windows!)
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            handle.exit(0);
        });

        Ok("Installer started".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Kein stilles Beenden ohne Installation: Datei zum manuellen Installieren anbieten.
        let path_str = client.to_string_lossy().to_string();
        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(&path_str).spawn();
        #[cfg(target_os = "linux")]
        let _ = Command::new("xdg-open")
            .arg(
                client
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(path_str.clone()),
            )
            .spawn();
        Ok(format!(
            "Update heruntergeladen ({}). Bitte manuell installieren – die App wurde NICHT beendet.",
            path_str
        ))
    }
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
    #[cfg(target_os = "linux")]
    {
        match action.as_str() {
            "shutdown" => {
                Command::new("systemctl")
                    .args(["poweroff"])
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            "lock" => {
                // loginctl funktioniert auf den meisten Desktops (GNOME/KDE/systemd)
                Command::new("loginctl")
                    .args(["lock-session"])
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }
    #[cfg(target_os = "macos")]
    {
        match action.as_str() {
            "shutdown" => {
                Command::new("osascript")
                    .args(["-e", "tell app \"System Events\" to shut down"])
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            "lock" => {
                Command::new("/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession")
                    .args(["-suspend"])
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let _ = action;
    Ok(())
}

#[tauri::command]
pub fn update_tray_status(app: AppHandle, status: String) -> Result<(), String> {
    // Tooltip-Länge begrenzen (Windows schneidet bei ~128 Zeichen ab)
    let short: String = status.chars().take(100).collect();
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_tooltip(Some(format!("SoundSync Downloader - {}", short)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playlist_entry_url_reconstructs_youtube_flat_id() {
        let flat = json!({ "id": "dQw4w9WgXcQ", "ie_key": "Youtube" });
        assert_eq!(
            playlist_entry_url(&flat),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );

        let full = json!({ "webpage_url": "https://soundcloud.com/artist/track" });
        assert_eq!(
            playlist_entry_url(&full),
            "https://soundcloud.com/artist/track"
        );
    }

    #[test]
    fn playlist_entry_title_falls_back_cleanly() {
        let with_title = json!({ "title": "  My Song  " });
        assert_eq!(playlist_entry_title(&with_title, 0), "My Song");

        let from_slug = json!({ "webpage_url": "https://soundcloud.com/a/cool_track-name/" });
        assert_eq!(playlist_entry_title(&from_slug, 2), "cool track name");

        let empty = json!({});
        assert_eq!(playlist_entry_title(&empty, 4), "Track 5");
    }

    #[test]
    fn sanitize_discord_text_truncates_and_falls_back() {
        assert_eq!(sanitize_discord_text("   ", "Fallback"), "Fallback");
        let long = "ä".repeat(200);
        assert_eq!(sanitize_discord_text(&long, "Fallback").chars().count(), 128);
    }
}
