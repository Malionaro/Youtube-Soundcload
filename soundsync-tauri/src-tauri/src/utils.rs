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

pub fn get_config_path() -> PathBuf {
    let mut path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("config.json");
    path
}

pub fn check_tool(name: &str, args: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(name);
    cmd.args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    match cmd.output() {
        Ok(output) => {
            let version_text = String::from_utf8_lossy(&output.stdout);
            let first_line = version_text.lines().next().unwrap_or("unknown").to_string();
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
