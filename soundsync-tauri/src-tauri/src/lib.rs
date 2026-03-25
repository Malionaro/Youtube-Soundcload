use std::process::Command;
use serde::{Serialize, Deserialize};
use tauri::{AppHandle, Emitter, Window};

#[derive(Serialize, Deserialize, Clone)]
struct Progress {
    percentage: f64,
    speed: String,
    eta: String,
    status: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Metadata {
    title: String,
    thumbnail: String,
    duration: String,
    uploader: String,
}

#[tauri::command]
async fn get_metadata(url: String) -> Result<Metadata, String> {
    let output = Command::new("yt-dlp")
        .args(["--json-extract", "--skip-download", &url])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    
    Ok(Metadata {
        title: json["title"].as_str().unwrap_or("Unknown").to_string(),
        thumbnail: json["thumbnail"].as_str().unwrap_or("").to_string(),
        duration: json["duration_string"].as_str().unwrap_or("00:00").to_string(),
        uploader: json["uploader"].as_str().unwrap_or("Unknown").to_string(),
    })
}

#[tauri::command]
async fn download_track(
    window: Window,
    url: String,
    format: String,
    path: String,
) -> Result<String, String> {
    let mut args = vec![
        "-f", "bestaudio/best", 
        "--extract-audio", 
        "--audio-format", &format,
        "--newline",
        "--progress-template", "{\"percentage\":\"%(progress._percent_str)s\",\"speed\":\"%(progress._speed_str)s\",\"eta\":\"%(progress._eta_str)s\"}"
    ];
    
    // Auto-detect FFmpeg from parent directory if it exists
    // The Python project had it in ../ffmpeg/bin/ffmpeg.exe
    // Let's assume the user wants to use that one.
    let ffmpeg_path = "e:\\win11 stuff\\PYTHON\\Youtube-Soundcload\\ffmpeg\\bin\\ffmpeg.exe";
    if std::path::Path::new(ffmpeg_path).exists() {
        args.push("--ffmpeg-location");
        args.push(ffmpeg_path);
    }

    args.push("-o");
    let output_template = format!("{}/%(title)s.%(ext)s", path);
    args.push(&output_template);
    args.push(&url);

    let mut child = Command::new("yt-dlp")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().unwrap();
    let reader = std::io::BufReader::new(stdout);
    use std::io::BufRead;

    for line in reader.lines() {
        if let Ok(l) = line {
            if l.starts_with('{') {
                if let Ok(p_json) = serde_json::from_str::<serde_json::Value>(&l) {
                    let pct_str = p_json["percentage"].as_str().unwrap_or("0%").replace('%', "").trim().to_string();
                    if let Ok(pct) = pct_str.parse::<f64>() {
                        let _ = window.emit("download-progress", Progress {
                            percentage: pct,
                            speed: p_json["speed"].as_str().unwrap_or("").to_string(),
                            eta: p_json["eta"].as_str().unwrap_or("").to_string(),
                            status: "Downloading".to_string(),
                        });
                    }
                }
            }
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("yt-dlp failed".to_string());
    }

    Ok("Download complete".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_metadata, download_track])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
