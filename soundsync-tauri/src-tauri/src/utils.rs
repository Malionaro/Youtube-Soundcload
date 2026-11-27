use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .output();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Erst Kinder beenden (pkill -P), dann Prozess selbst. Fehler ignorieren (best-effort).
        let _ = Command::new("pkill")
            .args(["-TERM", "-P", &pid.to_string()])
            .output();
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
    }
}

pub fn command_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(target_os = "windows")]
pub fn winget_output_means_installed(output: &str) -> bool {
    let normalized = output.to_lowercase();
    normalized.contains("already installed")
        || normalized.contains("no available upgrade")
        || normalized.contains("kein verfügbares upgrade")
        || normalized.contains("kein verfugbares upgrade")
        || normalized.contains("no newer package versions are available")
        || normalized.contains("keine neueren paketversionen verfügbar")
        || normalized.contains("keine neueren paketversionen verfugbar")
}

#[cfg(target_os = "windows")]
pub fn format_winget_error(package: &str, code: Option<i32>, output: &str) -> String {
    let details = output.trim();
    let code_text = code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    if details.is_empty() {
        format!("Installation failed for {package}: winget exited with code {code_text}.")
    } else {
        format!(
            "Installation failed for {package}: winget exited with code {code_text}.\n{details}"
        )
    }
}

pub fn app_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join("com.malo.soundsync.downloader.v2"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|p| PathBuf::from(p).join("Library/Application Support/com.malo.soundsync.downloader.v2"))
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("soundsync-downloader"));
            }
        }
        std::env::var("HOME")
            .ok()
            .map(|p| PathBuf::from(p).join(".config/soundsync-downloader"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub fn get_config_path() -> PathBuf {
    // 1. Bevorzugt: beschreibbares App-Config-Verzeichnis (funktioniert auch unter Program Files).
    if let Some(dir) = app_config_dir() {
        // Nur nutzen wenn wir es anlegen/beschreiben können
        if std::fs::create_dir_all(&dir).is_ok() {
            return dir.join("config.json");
        }
    }
    // 2. Fallback: neben der Exe (portabler Modus / dev)
    let mut path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("config.json");
    path
}

pub fn check_tool(name: &str, args: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(resolve_tool(name));
    cmd.args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    match cmd.output() {
        Ok(output) => {
            // stdout bevorzugen, stderr als Fallback (manche Tools schreiben Version nach stderr)
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            let first_line = combined.lines().next().unwrap_or("unknown").to_string();
            (output.status.success(), first_line)
        }
        Err(_) => (false, String::new()),
    }
}

pub fn parse_ffmpeg_duration(line: &str) -> Option<f64> {
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

pub fn parse_ffmpeg_time(line: &str) -> Option<f64> {
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

pub fn parse_time_to_seconds(time_str: &str) -> Option<f64> {
    let trimmed = time_str.trim();
    if trimmed.starts_with('-') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 3 {
        let hours: f64 = parts[0].parse().ok()?;
        let minutes: f64 = parts[1].parse().ok()?;
        let seconds: f64 = parts[2].parse().ok()?;
        if !hours.is_finite()
            || !minutes.is_finite()
            || !seconds.is_finite()
            || hours.is_sign_negative()
            || minutes.is_sign_negative()
            || seconds.is_sign_negative()
        {
            return None;
        }
        Some(hours * 3600.0 + minutes * 60.0 + seconds)
    } else {
        None
    }
}

/// Single-pass HTML entity decoder (avoids double-decoding `&amp;lt;`).
pub fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp_idx) = rest.find('&') {
        out.push_str(&rest[..amp_idx]);
        let after_amp = &rest[amp_idx + 1..];
        if let Some(semi_idx) = after_amp.find(';') {
            if semi_idx > 0 && semi_idx <= 12 {
                let entity = &after_amp[..semi_idx];
                let decoded: Option<char> = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    s if s.starts_with("#x") || s.starts_with("#X") => {
                        u32::from_str_radix(&s[2..], 16).ok().and_then(char::from_u32)
                    }
                    s if s.starts_with('#') => {
                        s[1..].parse::<u32>().ok().and_then(char::from_u32)
                    }
                    _ => None,
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    rest = &after_amp[semi_idx + 1..];
                    continue;
                }
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

pub fn mime_type_for_filename(name: &str) -> &'static str {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "aiff" => "audio/aiff",
        "wma" => "audio/x-ms-wma",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "3gp" => "video/3gpp",
        _ => "application/octet-stream",
    }
}

pub fn is_video_format(fmt: &str) -> bool {
    matches!(
        fmt.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "avi" | "mov" | "flv" | "wmv" | "3gp"
    )
}

pub fn is_lossless_audio_format(fmt: &str) -> bool {
    matches!(
        fmt.to_ascii_lowercase().as_str(),
        "wav" | "flac" | "alac" | "aiff"
    )
}

pub fn sanitize_sponsorblock_categories(raw: &str) -> String {
    let cleaned: Vec<String> = raw
        .split(',')
        .map(|part| {
            part.trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        })
        .filter(|part| !part.is_empty())
        .collect();

    if cleaned.is_empty() {
        "sponsor,selfpromo,interaction".to_string()
    } else {
        cleaned.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_html_entities_single_pass_prevents_double_decode() {
        assert_eq!(decode_html_entities("Rock &amp; Roll"), "Rock & Roll");
        assert_eq!(decode_html_entities("&amp;lt;script&amp;gt;"), "&lt;script&gt;");
        assert_eq!(decode_html_entities("Don&#39;t &quot;Stop&quot; &#x266A;"), "Don't \"Stop\" ♪");
        assert_eq!(decode_html_entities("no &unknown; entity"), "no &unknown; entity");
    }

    #[test]
    fn ffmpeg_duration_and_time_parsing() {
        assert_eq!(
            parse_ffmpeg_duration("  Duration: 01:02:03.50, start: 0.000000, bitrate: 320 kb/s"),
            Some(3723.5)
        );
        assert_eq!(
            parse_ffmpeg_time("size= 1024kB time=00:01:30.25 bitrate= 192.0kbits/s"),
            Some(90.25)
        );
        assert_eq!(parse_time_to_seconds("N/A"), None);
        assert_eq!(parse_time_to_seconds("-00:01:00"), None);
        assert_eq!(parse_time_to_seconds("00:NaN:00"), None);
    }

    #[test]
    fn mime_and_format_helpers() {
        assert_eq!(mime_type_for_filename("song.MP3"), "audio/mpeg");
        assert_eq!(mime_type_for_filename("clip.mp4"), "video/mp4");
        assert_eq!(mime_type_for_filename("song.flac"), "audio/flac");
        assert_eq!(mime_type_for_filename("unknown.xyz"), "application/octet-stream");
        assert!(is_video_format("MKV"));
        assert!(!is_video_format("mp3"));
        assert!(is_lossless_audio_format("FLAC"));
        assert!(!is_lossless_audio_format("mp3"));
    }

    #[test]
    fn sponsorblock_categories_sanitized() {
        assert_eq!(
            sanitize_sponsorblock_categories(" sponsor , selfpromo;rm -rf / , intro "),
            "sponsor,selfpromorm-rf,intro"
        );
        assert_eq!(
            sanitize_sponsorblock_categories("   ,,,   "),
            "sponsor,selfpromo,interaction"
        );
    }
}

// ─── Tool-Auflösung + winget-Erkennung (robuster ohne winget) ────────────────

/// Ordner für automatisch heruntergeladene Tools (ffmpeg/yt-dlp).
pub fn tools_dir() -> PathBuf {
    app_config_dir().unwrap_or_else(|| PathBuf::from(".")).join("bin")
}

fn path_sep() -> &'static str { if cfg!(windows) { ";" } else { ":" } }

/// `tools_dir` an den Anfang von PATH setzen (für Kind-Prozesse + check_tool).
pub fn add_tools_to_path() {
    let dir = tools_dir();
    let _ = std::fs::create_dir_all(&dir);
    let dir_str = dir.to_string_lossy().to_string();
    let current = std::env::var("PATH").unwrap_or_default();
    if !current.split(path_sep()).any(|p| PathBuf::from(p) == dir) {
        std::env::set_var("PATH", format!("{}{}{}", dir_str, path_sep(), current));
    }
}

/// Tool auflösen: erst `tools_dir`, sonst über PATH.
pub fn resolve_tool(name: &str) -> PathBuf {
    add_tools_to_path();
    let dir = tools_dir();
    let exe = if cfg!(windows) { format!("{}.exe", name) } else { name.to_string() };
    let local = dir.join(&exe);
    if local.exists() { local } else { PathBuf::from(name) }
}

#[cfg(target_os = "windows")]
pub fn winget_available() -> bool {
    Command::new("winget").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
pub fn winget_available() -> bool { false }
