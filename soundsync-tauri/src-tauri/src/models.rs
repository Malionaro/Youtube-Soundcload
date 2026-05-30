use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    #[serde(default = "default_true")]
    pub auto_scroll_log: bool,
    #[serde(default = "default_true")]
    pub eco_mode: bool,
    #[serde(default = "default_true")]
    pub auto_tagging: bool,
    #[serde(default = "default_after_download")]
    pub after_download: String,
}

pub fn default_after_download() -> String {
    "nothing".to_string()
}

pub fn default_auto_url_detection() -> bool {
    true
}

pub fn default_true() -> bool {
    true
}

pub fn default_quality() -> String {
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
            auto_scroll_log: true,
            eco_mode: true,
            auto_tagging: true,
            after_download: "nothing".to_string(),
        }
    }
}

pub fn dirs_next() -> Option<PathBuf> {
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
    pub pot_provider_installed: bool,
    pub pot_provider_status: String,
}
