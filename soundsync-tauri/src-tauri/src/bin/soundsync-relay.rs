// soundsync-relay — kostenloser, selbst gehosteter Koordinator/Relay für
// SoundSync Team-Mode (P2P). Läuft auf einem Server / Raspberry Pi.
//
// Beide PCs verbinden sich NUR AUSGEHEND zu diesem Relay (kein Port-Forwarding
// auf den PCs). Der Relay ist Koordinator: zusammengeführte Playlist, Verteilung
// der Tracks, Failover bei YouTube-Sperre/Ausfall, Sammeln der fertigen Dateien.
//
//   SOUNDSYNC_RELAY_TOKEN=secret   (optional; sonst wird ein Token erzeugt + ausgegeben)
//   SOUNDSYNC_TEAM_ID=myteam       (optional; sonst "team")
//   SOUNDSYNC_RELAY_DIR=/var/lib/soundsync-relay   (Sammelordner für Dateien)
//   PORT=3030                      (optional; Port auf dem der Relay lauscht)
//
// App auf PC A und PC B: "Beitreten" mit dem ausgegebenen soundsync-team:// Link.
// Eigene Playlists je PC beisteuern -> gemeinsame Liste wird gemeinsam geladen.

use soundsync_tauri_lib::server::is_safe_filename;
use soundsync_tauri_lib::team::{
    coord_add_playlist, coord_claim, coord_done, coord_fail, coord_heartbeat, coord_join,
    coord_leave, coord_progress, coord_sweep, TeamRole, TeamState,
};
use soundsync_tauri_lib::models::generate_remote_token;
use std::sync::{Arc, Mutex};
use std::thread;
use tiny_http::{Method, Response, Server};

fn json(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
}

fn req_token(request: &tiny_http::Request) -> Option<String> {
    for header in request.headers() {
        let f = header.field.as_str().as_bytes();
        if f.eq_ignore_ascii_case(b"x-team-token") || f.eq_ignore_ascii_case(b"x-remote-token") {
            let t = header.value.as_str().trim();
            if !t.is_empty() { return Some(t.to_string()); }
        }
        if f.eq_ignore_ascii_case(b"authorization") {
            let v = header.value.as_str();
            if let Some(tok) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
                let t = tok.trim();
                if !t.is_empty() { return Some(t.to_string()); }
            }
        }
    }
    // ?token=
    if let Some(q) = request.url().split_once('?').map(|x| x.1) {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "token" && !v.is_empty() {
                    return Some(urlencoding::decode(v).map(|s| s.into_owned()).unwrap_or_else(|_| v.to_string()));
                }
            }
        }
    }
    None
}

fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3030);
    let token = std::env::var("SOUNDSYNC_RELAY_TOKEN").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(generate_remote_token);
    let team_id = std::env::var("SOUNDSYNC_TEAM_ID").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(|| "team".to_string());
    let share_dir = std::env::var("SOUNDSYNC_RELAY_DIR").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(|| "./relay-data".to_string());
    let _ = std::fs::create_dir_all(&share_dir);

    let advertised = std::env::var("SOUNDSYNC_ADVERTISED_ADDR").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("127.0.0.1:{}", port));

    let state = Arc::new(Mutex::new(TeamState {
        active: true,
        role: TeamRole::Host,
        node_id: "relay".to_string(),
        node_name: "soundsync-relay".to_string(),
        token: token.clone(),
        team_id: team_id.clone(),
        self_addr: advertised.clone(),
        host_addr: advertised.clone(),
        members: Vec::new(),
        tasks: Vec::new(),
        format: String::new(),
        quality: String::new(),
        folder: share_dir.clone(),
        share_files: true,
        blocked_until: 0,
        worker_running: false,
        last_error: String::new(),
    }));

    // Watchdog: verwaiste/offline Aufgaben neu verteilen
    {
        let state = state.clone();
        thread::spawn(move || loop {
            thread::sleep(std::time::Duration::from_millis(5_000));
            let mut team = state.lock().unwrap();
            coord_sweep(&mut team);
        });
    }

    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => { eprintln!("kann {} nicht binden: {}", addr, e); std::process::exit(1); }
    };

    let link = format!("soundsync-team://{}/{}?token={}", advertised, team_id, token);
    println!("soundsync-relay läuft auf {}", addr);
    println!("Sammelordner: {}", share_dir);
    println!("Token: {}", token);
    println!("Einladungs-Link (auf BEIDEN PCs einfügen -> Beitreten):\n  {}", link);
    println!("Kein Port-Forwarding auf den PCs nötig; nur dieser Server muss erreichbar sein.");

    let share_dir = Arc::new(share_dir);

    for mut request in server.incoming_requests() {
        let state = state.clone();
        let share_dir = share_dir.clone();
        let token = token.clone();
        let full_url = request.url().to_string();
        let path = full_url.split('?').next().unwrap_or("/").to_string();
        let method = request.method().clone();

        // CORS preflight
        if method == Method::Options {
            let _ = request.respond(
                Response::from_string("")
                    .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                    .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"POST, GET, OPTIONS"[..]).unwrap())
                    .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization, X-Team-Token, X-Remote-Token"[..]).unwrap()),
            );
            continue;
        }

        // Body lesen (JSON oder raw)
        let mut raw_body: Vec<u8> = Vec::new();
        let _ = request.as_reader().read_to_end(&mut raw_body);
        let body_str = String::from_utf8_lossy(&raw_body).to_string();
        let json_body: serde_json::Value = serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

        // Health ohne Auth
        if method == Method::Get && path == "/health" {
            let _ = request.respond(json("{\"ok\":true}".to_string()));
            continue;
        }

        // Auth
        let authorized = req_token(&request).map(|t| t == token).unwrap_or(false)
            || json_body.get("token").and_then(|v| v.as_str()).map(|t| t == token).unwrap_or(false);
        if !authorized {
            let _ = request.respond(Response::from_string("Unauthorized").with_status_code(401));
            continue;
        }

        match (method.clone(), path.as_str()) {
            (Method::Get, "/team/state") => {
                let team = state.lock().unwrap().clone();
                let _ = request.respond(json(serde_json::to_string(&team).unwrap_or_else(|_| "{}".to_string())));
            }
            (Method::Post, "/team/join") => {
                let token = json_body.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let team_id = json_body.get("team_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = json_body.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let addr = json_body.get("addr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let result = { let mut team = state.lock().unwrap(); coord_join(&mut team, &token, &team_id, &node_id, &name, &addr) };
                match result {
                    Ok(snap) => { let _ = request.respond(json(serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_string()))); }
                    Err(e) => { let _ = request.respond(json(serde_json::json!({"error":e}).to_string()).with_status_code(403)); }
                }
            }
            (Method::Post, "/team/leave") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                { let mut team = state.lock().unwrap(); coord_leave(&mut team, &node_id); }
                let _ = request.respond(json("OK".to_string()));
            }
            (Method::Post, "/team/playlist") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let source = json_body.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let entries: Vec<(String, String)> = json_body.get("entries").and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|e| {
                        let p = e.as_array()?;
                        let title = p.first()?.as_str().unwrap_or("").to_string();
                        let url = p.get(1)?.as_str()?.to_string();
                        Some((title, url))
                    }).collect()).unwrap_or_default();
                let added = { let mut team = state.lock().unwrap(); coord_add_playlist(&mut team, &node_id, &source, entries) };
                let _ = request.respond(json(added.to_string()));
            }
            (Method::Post, "/team/claim") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let task = { let mut team = state.lock().unwrap(); coord_claim(&mut team, &node_id) };
                let _ = request.respond(json(serde_json::to_string(&task).unwrap_or_else(|_| "null".to_string())));
            }
            (Method::Post, "/team/progress") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let task_id = json_body.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let percent = json_body.get("percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                { let mut team = state.lock().unwrap(); coord_progress(&mut team, &node_id, &task_id, percent); }
                let _ = request.respond(json("OK".to_string()));
            }
            (Method::Post, "/team/done") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let task_id = json_body.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let file_name = json_body.get("file_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                { let mut team = state.lock().unwrap(); coord_done(&mut team, &node_id, &task_id, &file_name); }
                let _ = request.respond(json("OK".to_string()));
            }
            (Method::Post, "/team/fail") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let task_id = json_body.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let blocked = json_body.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
                let error = json_body.get("error").and_then(|v| v.as_str()).unwrap_or("").to_string();
                { let mut team = state.lock().unwrap(); coord_fail(&mut team, &node_id, &task_id, blocked, &error); }
                let _ = request.respond(json("OK".to_string()));
            }
            (Method::Post, "/team/heartbeat") => {
                let node_id = json_body.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let blocked = json_body.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
                { let mut team = state.lock().unwrap(); coord_heartbeat(&mut team, &node_id, blocked); }
                let _ = request.respond(json("OK".to_string()));
            }
            (Method::Post, "/team/file") => {
                // fertige Datei empfangen
                let name = full_url.split_once('?').map(|x| x.1).unwrap_or("").split('&')
                    .find_map(|pair| pair.split_once('=').filter(|(k, _)| *k == "name").map(|(_, v)| v.to_string()))
                    .and_then(|v| urlencoding::decode(&v).ok().map(|s| s.into_owned())).unwrap_or_default();
                if !is_safe_filename(&name) { let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400)); continue; }
                let dest = std::path::Path::new(&*share_dir).join(&name);
                match std::fs::write(&dest, &raw_body) {
                    Ok(_) => { let _ = request.respond(json("OK".to_string())); }
                    Err(e) => { let _ = request.respond(Response::from_string(format!("write error: {}", e)).with_status_code(500)); }
                }
            }
            (Method::Get, p) if p.starts_with("/team/file/") => {
                let raw = p.trim_start_matches("/team/file/").split('?').next().unwrap_or("");
                let decoded = urlencoding::decode(raw).map(|s| s.into_owned()).unwrap_or_else(|_| raw.to_string());
                if !is_safe_filename(&decoded) { let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400)); continue; }
                let file_path = std::path::Path::new(&*share_dir).join(&decoded);
                if file_path.exists() && file_path.is_file() {
                    match std::fs::File::open(&file_path) {
                        Ok(f) => { let _ = request.respond(Response::from_file(f)); }
                        Err(_) => { let _ = request.respond(Response::from_string("Cannot read file").with_status_code(500)); }
                    }
                } else { let _ = request.respond(Response::from_string("File not found").with_status_code(404)); }
            }
            _ => { let _ = request.respond(Response::from_string("Not Found").with_status_code(404)); }
        }
    }
}
// (kein zusätzlicher Shim nötig)
