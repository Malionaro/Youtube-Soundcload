use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// Lock a mutex without panicking if a previous holder panicked (poison recovery).
pub fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

// ─── App State ───────────────────────────────────────────────────────────────

pub struct AppState {
    pub is_downloading: Mutex<bool>,
    pub abort_flag: Mutex<bool>,
    pub pause_flag: Mutex<bool>,
    pub active_download_pids: Arc<Mutex<Vec<u32>>>,
    pub config: Mutex<AppConfig>,
    pub discord_client: Arc<Mutex<Option<DiscordClient>>>,
    pub current_progress: Mutex<Option<DownloadProgress>>,
    pub remote_queue: Mutex<Vec<RemoteQueueItem>>,
    pub remote_token: Mutex<String>,
    pub team: Mutex<crate::team::TeamState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            is_downloading: Mutex::new(false),
            abort_flag: Mutex::new(false),
            pause_flag: Mutex::new(false),
            active_download_pids: Arc::new(Mutex::new(Vec::new())),
            config: Mutex::new(AppConfig::default()),
            discord_client: Arc::new(Mutex::new(None)),
            current_progress: Mutex::new(None),
            remote_queue: Mutex::new(Vec::new()),
            remote_token: Mutex::new(generate_remote_token()), team: Mutex::new(crate::team::TeamState::default()),
        }
    }
}

pub fn generate_remote_token() -> String {
    // OS-Entropie via std::collections::hash_map::RandomState (ohne extra Crate) + Zeit/PID/Counter
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut h1 = RandomState::new().build_hasher();
    h1.write_u128(now);
    h1.write_u64(pid);
    h1.write_u64(c);
    let part1 = h1.finish();

    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(part1);
    h2.write_u64(c.wrapping_add(1));
    h2.write_u128(now.rotate_left(37));
    let part2 = h2.finish();

    format!("{:016x}{:016x}", part1, part2)
}

pub struct DiscordClient {
    pub client: discord_rich_presence::DiscordIpcClient,
    pub start_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_download_folder")]
    pub download_folder: String,
    #[serde(default)]
    pub cookies_path: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub disable_changelog: bool,
    #[serde(default = "default_auto_url_detection")]
    pub auto_url_detection: bool,
    #[serde(default = "default_true")]
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
    #[serde(default = "default_sponsorblock_mode")]
    pub sponsorblock_mode: String,
    #[serde(default = "default_sponsorblock_categories")]
    pub sponsorblock_categories: String,
    #[serde(default = "default_gpu_encoder")]
    pub gpu_encoder: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_true")]
    pub auto_scroll_tracks: bool,
}

pub fn default_download_folder() -> String {
    dirs_next()
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

pub fn default_language() -> String {
    "en".to_string()
}

pub fn default_after_download() -> String {
    "nothing".to_string()
}

pub fn default_sponsorblock_mode() -> String {
    "off".to_string()
}

pub fn default_sponsorblock_categories() -> String {
    "sponsor,selfpromo,interaction".to_string()
}

pub fn default_gpu_encoder() -> String {
    "auto".to_string()
}

pub fn default_format() -> String {
    "mp3".to_string()
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
            discord_rpc: true,
            accent_color: None,
            custom_background: None,
            quality: "best".to_string(),
            auto_scroll_log: true,
            eco_mode: true,
            auto_tagging: true,
            after_download: "nothing".to_string(),
            sponsorblock_mode: "off".to_string(),
            sponsorblock_categories: "sponsor,selfpromo,interaction".to_string(),
            gpu_encoder: "auto".to_string(),
            format: "mp3".to_string(),
            auto_scroll_tracks: true,
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
pub struct RemoteQueueItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub status: String,
    pub added_at: i64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_token_is_32_hex_chars_and_unique() {
        let t1 = generate_remote_token();
        let t2 = generate_remote_token();
        assert_eq!(t1.len(), 32);
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t1, t2);
    }

    #[test]
    fn app_config_deserializes_with_defaults() {
        let cfg: AppConfig = serde_json::from_str("{}").expect("empty JSON should deserialize");
        assert_eq!(cfg.format, "mp3");
        assert_eq!(cfg.quality, "best");
        assert_eq!(cfg.gpu_encoder, "auto");
        assert_eq!(cfg.sponsorblock_mode, "off");
        assert!(cfg.auto_tagging);
        assert!(cfg.auto_scroll_log);
        assert!(cfg.auto_scroll_tracks);
    }
}
