use crate::models::{generate_remote_token, lock_unpoisoned, AppState, RemotePayload};
use crate::utils::mime_type_for_filename;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use tauri::{AppHandle, Emitter, Manager};
use tiny_http::{Header, Method, Response, Server, StatusCode};

static SERVER_STARTED: AtomicBool = AtomicBool::new(false);
const MAX_SEND_BODY_BYTES: u64 = 64 * 1024;

/// Dateiname gegen Path-Traversal und Windows-Gerätenamen prüfen. Nur flache Namen, keine Separatoren.
pub fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') || name.contains(':') {
        return false;
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    if name.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return false;
    }
    // Windows-reservierte Gerätenamen abweisen (CON, PRN, AUX, NUL, COM1..9, LPT1..9)
    let stem = name.split('.').next().unwrap_or("");
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return false;
    }
    true
}

fn resolve_download_file(folder: &str, filename: &str) -> Option<PathBuf> {
    if !is_safe_filename(filename) || folder.is_empty() {
        return None;
    }
    let base = std::fs::canonicalize(Path::new(folder)).ok()?;
    let candidate = std::fs::canonicalize(base.join(filename)).ok()?;
    if !candidate.starts_with(&base) || !candidate.is_file() {
        return None;
    }
    Some(candidate)
}

fn ct_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn header_value(request: &tiny_http::Request, name: &[u8]) -> Option<String> {
    for header in request.headers() {
        if header.field.as_str().as_bytes().eq_ignore_ascii_case(name) {
            let val = header.value.as_str().trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn is_trusted_origin(origin: &str, host_header: Option<&str>) -> bool {
    if origin.starts_with("chrome-extension://")
        || origin.starts_with("moz-extension://")
        || origin.starts_with("safari-web-extension://")
    {
        return true;
    }
    if origin == "http://localhost:3030"
        || origin == "http://127.0.0.1:3030"
        || origin == "http://[::1]:3030"
    {
        return true;
    }
    if let Some(host) = host_header {
        if origin == format!("http://{}", host) || origin == format!("https://{}", host) {
            return true;
        }
    }
    false
}

fn apply_cors<R: Read>(
    mut response: Response<R>,
    origin: Option<&str>,
    host: Option<&str>,
) -> Response<R> {
    if let Some(o) = origin {
        if is_trusted_origin(o, host) {
            if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], o.as_bytes()) {
                response = response.with_header(h);
            }
            if let Ok(v) = Header::from_bytes(&b"Vary"[..], &b"Origin"[..]) {
                response = response.with_header(v);
            }
        }
    }
    response
}

fn extract_query_token(url: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if k == "token" && !v.is_empty() {
                return urlencoding::decode(v).ok().map(|s| s.into_owned());
            }
        }
    }
    None
}

fn request_token(request: &tiny_http::Request) -> Option<String> {
    for header in request.headers() {
        let field = header.field.as_str().as_bytes();
        if field.eq_ignore_ascii_case(b"authorization") {
            let val = header.value.as_str();
            if let Some(tok) = val
                .strip_prefix("Bearer ")
                .or_else(|| val.strip_prefix("bearer "))
            {
                let t = tok.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
        if field.eq_ignore_ascii_case(b"x-remote-token") {
            let t = header.value.as_str().trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    extract_query_token(request.url())
}

fn is_loopback_request(request: &tiny_http::Request) -> bool {
    if let Some(addr) = request.remote_addr() {
        return addr.ip().is_loopback();
    }
    // Falls tiny_http keine Remote-Adresse liefert: sicherheitshalber als extern behandeln.
    false
}

/// Schützende Endpunkte:
/// - Leeres Token schlägt geschlossen fehl (fail-closed).
/// - Loopback (Extension/lokal) erlaubt, sofern kein fremder Browser-Origin (CSWSH/CSRF-Schutz!).
/// - LAN erfordert immer ein gültiges Token.
fn is_authorized(
    request: &tiny_http::Request,
    token: &str,
    origin: Option<&str>,
    host: Option<&str>,
) -> bool {
    if token.is_empty() {
        return false;
    }
    if let Some(t) = request_token(request) {
        if ct_eq(&t, token) {
            return true;
        }
    }
    if is_loopback_request(request) {
        match origin {
            None => return true,
            Some(o) => return is_trusted_origin(o, host),
        }
    }
    false
}

fn unauthorized(origin: Option<&str>, host: Option<&str>) -> Response<std::io::Cursor<Vec<u8>>> {
    let resp = Response::from_string("Unauthorized").with_status_code(401);
    apply_cors(resp, origin, host)
}

fn parse_byte_range(range_header: &str, file_len: u64) -> Option<(u64, u64)> {
    let spec = range_header.trim().strip_prefix("bytes=")?;
    // Nur einzelnen Bereich bedienen
    let first = spec.split(',').next()?.trim();
    let (start_str, end_str) = first.split_once('-')?;
    if file_len == 0 {
        return None;
    }
    if start_str.is_empty() {
        // Suffix-Range: bytes=-500
        let suffix: u64 = end_str.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = file_len.saturating_sub(suffix);
        return Some((start, file_len - 1));
    }
    let start: u64 = start_str.parse().ok()?;
    if start >= file_len {
        return None;
    }
    let end = if end_str.is_empty() {
        file_len - 1
    } else {
        end_str.parse::<u64>().ok()?.min(file_len - 1)
    };
    if start > end {
        return None;
    }
    Some((start, end))
}

fn serve_file_request(
    request: tiny_http::Request,
    file_path: PathBuf,
    filename: String,
    origin: Option<String>,
    host: Option<String>,
) {
    let mime = mime_type_for_filename(&filename);
    let range_hdr = header_value(&request, b"range");

    let mut file = match std::fs::File::open(&file_path) {
        Ok(f) => f,
        Err(_) => {
            let resp = apply_cors(
                Response::from_string("Cannot read file").with_status_code(500),
                origin.as_deref(),
                host.as_deref(),
            );
            let _ = request.respond(resp);
            return;
        }
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => {
            let resp = apply_cors(
                Response::from_string("Cannot read file metadata").with_status_code(500),
                origin.as_deref(),
                host.as_deref(),
            );
            let _ = request.respond(resp);
            return;
        }
    };

    if let Some(ref rh) = range_hdr {
        if let Some((start, end)) = parse_byte_range(rh, file_len) {
            if file.seek(SeekFrom::Start(start)).is_ok() {
                let chunk_len = end - start + 1;
                let mut headers = Vec::new();
                if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()) {
                    headers.push(h);
                }
                if let Ok(h) = Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]) {
                    headers.push(h);
                }
                let content_range = format!("bytes {}-{}/{}", start, end, file_len);
                if let Ok(h) = Header::from_bytes(&b"Content-Range"[..], content_range.as_bytes()) {
                    headers.push(h);
                }
                let resp = Response::new(
                    StatusCode(206),
                    headers,
                    file.take(chunk_len),
                    Some(chunk_len as usize),
                    None,
                );
                let resp = apply_cors(resp, origin.as_deref(), host.as_deref());
                let _ = request.respond(resp);
                return;
            }
        } else {
            let content_range = format!("bytes */{}", file_len);
            let mut resp = Response::from_string("Range Not Satisfiable").with_status_code(416);
            if let Ok(h) = Header::from_bytes(&b"Content-Range"[..], content_range.as_bytes()) {
                resp = resp.with_header(h);
            }
            let _ = request.respond(apply_cors(resp, origin.as_deref(), host.as_deref()));
            return;
        }
    }

    let mut resp = Response::from_file(file);
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()) {
        resp = resp.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]) {
        resp = resp.with_header(h);
    }
    let resp = apply_cors(resp, origin.as_deref(), host.as_deref());
    let _ = request.respond(resp);
}

#[tauri::command]
pub fn ss_get_remote_token(state: tauri::State<AppState>) -> Result<String, String> {
    Ok(lock_unpoisoned(&state.remote_token).clone())
}

#[tauri::command]
pub fn ss_get_local_ip() -> Result<String, String> {
    use std::net::UdpSocket;

    // Dummy-Verbindung (sendet kein Paket) um das primäre Netzwerk-Interface zu bestimmen.
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    return Ok(ip.to_string());
                }
            }
        }
    }

    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ss_start_remote_server(app: AppHandle) -> Result<(), String> {
    // Token sicherstellen (LAN-Schutz). Loopback (Extension) braucht keins.
    {
        let state = app.state::<AppState>();
        let mut tok = lock_unpoisoned(&state.remote_token);
        if tok.is_empty() {
            *tok = generate_remote_token();
        }
    }
    if SERVER_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let server_handle = app.clone();
    thread::spawn(move || {
        let addr = "0.0.0.0:3030";

        let server = match Server::http(addr) {
            Ok(s) => s,
            Err(_) => {
                SERVER_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };

        for mut request in server.incoming_requests() {
            let token = {
                let state = server_handle.state::<AppState>();
                let x = lock_unpoisoned(&state.remote_token).clone();
                x
            };
            let origin = header_value(&request, b"origin");
            let host = header_value(&request, b"host");
            let full_url = request.url().to_string();
            let path = full_url.split('?').next().unwrap_or("/").to_string();
            let method = request.method().clone();

            match (method, path.as_str()) {
                (Method::Get, "/") => {
                    // Token serverseitig injizieren (kein Leak über QR-Dienst nötig)
                    let html = include_str!("remote.html").replacen("__REMOTE_TOKEN__", &token, 1);
                    let response = Response::from_string(html).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                            .unwrap(),
                    );
                    let _ = request.respond(response);
                }
                (Method::Get, "/logo.png") => {
                    let bytes = include_bytes!("../icons/128x128.png").to_vec();
                    let response = Response::from_data(bytes)
                        .with_header(
                            Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap(),
                        )
                        .with_header(
                            Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=3600"[..])
                                .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Get, "/status") => {
                    let state = server_handle.state::<AppState>();
                    let progress = lock_unpoisoned(&state.current_progress).clone();
                    let json =
                        serde_json::to_string(&progress).unwrap_or_else(|_| "null".to_string());
                    let response = Response::from_string(json).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    let _ = request.respond(apply_cors(response, origin.as_deref(), host.as_deref()));
                }
                (Method::Get, "/remote-config") => {
                    let language = {
                        let state = server_handle.state::<AppState>();
                        let config = lock_unpoisoned(&state.config);
                        config.language.clone()
                    };
                    let json = serde_json::json!({ "language": language }).to_string();
                    let response = Response::from_string(json).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    let _ = request.respond(apply_cors(response, origin.as_deref(), host.as_deref()));
                }
                (Method::Get, "/queue") => {
                    if !is_authorized(&request, &token, origin.as_deref(), host.as_deref()) {
                        let _ = request.respond(unauthorized(origin.as_deref(), host.as_deref()));
                        continue;
                    }
                    let queue = {
                        let state = server_handle.state::<AppState>();
                        let q = lock_unpoisoned(&state.remote_queue).clone();
                        q
                    };
                    let json = serde_json::to_string(&queue).unwrap_or_else(|_| "[]".to_string());
                    let response = Response::from_string(json).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    let _ = request.respond(apply_cors(response, origin.as_deref(), host.as_deref()));
                }
                (Method::Post, "/send") => {
                    if !is_authorized(&request, &token, origin.as_deref(), host.as_deref()) {
                        let _ = request.respond(unauthorized(origin.as_deref(), host.as_deref()));
                        continue;
                    }
                    let mut content = String::new();
                    let _ = request
                        .as_reader()
                        .take(MAX_SEND_BODY_BYTES)
                        .read_to_string(&mut content);

                    if let Ok(payload) = serde_json::from_str::<RemotePayload>(&content) {
                        if payload.url.trim().is_empty() {
                            let resp = apply_cors(
                                Response::from_string("Empty URL").with_status_code(400),
                                origin.as_deref(),
                                host.as_deref(),
                            );
                            let _ = request.respond(resp);
                            continue;
                        }
                        if let Err(e) =
                            server_handle.emit_to("main", "remote-url-received", payload)
                        {
                            println!("Failed to emit event to main window: {:?}", e);
                        }
                        let resp = apply_cors(
                            Response::from_string("OK"),
                            origin.as_deref(),
                            host.as_deref(),
                        );
                        let _ = request.respond(resp);
                    } else {
                        let url = content.trim().to_string();
                        if !url.is_empty() {
                            let payload = RemotePayload {
                                url,
                                format: None,
                                auto_start: None,
                            };
                            if let Err(e) =
                                server_handle.emit_to("main", "remote-url-received", payload)
                            {
                                println!("Failed to emit event (fallback) to main window: {:?}", e);
                            }
                            let resp = apply_cors(
                                Response::from_string("OK"),
                                origin.as_deref(),
                                host.as_deref(),
                            );
                            let _ = request.respond(resp);
                        } else {
                            let resp = apply_cors(
                                Response::from_string("Empty URL").with_status_code(400),
                                origin.as_deref(),
                                host.as_deref(),
                            );
                            let _ = request.respond(resp);
                        }
                    }
                }
                (Method::Options, _) => {
                    let response = Response::from_string("")
                        .with_header(
                            Header::from_bytes(
                                &b"Access-Control-Allow-Methods"[..],
                                &b"POST, GET, OPTIONS, DELETE"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            Header::from_bytes(
                                &b"Access-Control-Allow-Headers"[..],
                                &b"Content-Type, Authorization, X-Remote-Token"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(apply_cors(response, origin.as_deref(), host.as_deref()));
                }
                (Method::Get, "/files") => {
                    if !is_authorized(&request, &token, origin.as_deref(), host.as_deref()) {
                        let _ = request.respond(unauthorized(origin.as_deref(), host.as_deref()));
                        continue;
                    }
                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = lock_unpoisoned(&state.config).download_folder.clone();
                        f
                    };

                    let mut files_with_time: Vec<(String, std::time::SystemTime)> = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(folder) {
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_file() {
                                    if let Some(name) = entry.file_name().to_str() {
                                        if !name.starts_with('.')
                                            && !name.ends_with(".part")
                                            && !name.ends_with(".ytdl")
                                            && !name.ends_with(".tmp")
                                        {
                                            let modified = metadata
                                                .modified()
                                                .unwrap_or(std::time::UNIX_EPOCH);
                                            files_with_time.push((name.to_string(), modified));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Neueste Downloads zuerst anzeigen
                    files_with_time.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                    let files_list: Vec<String> =
                        files_with_time.into_iter().map(|(n, _)| n).collect();

                    let json =
                        serde_json::to_string(&files_list).unwrap_or_else(|_| "[]".to_string());
                    let response = Response::from_string(json).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    let _ = request.respond(apply_cors(response, origin.as_deref(), host.as_deref()));
                }
                (Method::Get, p) if p.starts_with("/download/") => {
                    if !is_authorized(&request, &token, origin.as_deref(), host.as_deref()) {
                        let _ = request.respond(unauthorized(origin.as_deref(), host.as_deref()));
                        continue;
                    }
                    let raw = p.trim_start_matches("/download/");
                    let raw = raw.split('?').next().unwrap_or("");
                    let decoded_filename = urlencoding::decode(raw)
                        .unwrap_or(std::borrow::Cow::Borrowed(raw))
                        .into_owned();
                    if !is_safe_filename(&decoded_filename) {
                        let resp = apply_cors(
                            Response::from_string("Invalid filename").with_status_code(400),
                            origin.as_deref(),
                            host.as_deref(),
                        );
                        let _ = request.respond(resp);
                        continue;
                    }

                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = lock_unpoisoned(&state.config).download_folder.clone();
                        f
                    };

                    if let Some(file_path) = resolve_download_file(&folder, &decoded_filename) {
                        // In eigenem Thread streamen, damit große Dateien /status & /send nicht blockieren
                        thread::spawn(move || {
                            serve_file_request(request, file_path, decoded_filename, origin, host);
                        });
                    } else {
                        let resp = apply_cors(
                            Response::from_string("File not found").with_status_code(404),
                            origin.as_deref(),
                            host.as_deref(),
                        );
                        let _ = request.respond(resp);
                    }
                }
                (Method::Delete, p) if p.starts_with("/delete/") => {
                    if !is_authorized(&request, &token, origin.as_deref(), host.as_deref()) {
                        let _ = request.respond(unauthorized(origin.as_deref(), host.as_deref()));
                        continue;
                    }
                    let raw = p.trim_start_matches("/delete/");
                    let raw = raw.split('?').next().unwrap_or("");
                    let decoded_filename = urlencoding::decode(raw)
                        .unwrap_or(std::borrow::Cow::Borrowed(raw))
                        .into_owned();
                    if !is_safe_filename(&decoded_filename) {
                        let resp = apply_cors(
                            Response::from_string("Invalid filename").with_status_code(400),
                            origin.as_deref(),
                            host.as_deref(),
                        );
                        let _ = request.respond(resp);
                        continue;
                    }

                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = lock_unpoisoned(&state.config).download_folder.clone();
                        f
                    };

                    if let Some(file_path) = resolve_download_file(&folder, &decoded_filename) {
                        if std::fs::remove_file(&file_path).is_ok() {
                            let resp = apply_cors(
                                Response::from_string("OK"),
                                origin.as_deref(),
                                host.as_deref(),
                            );
                            let _ = request.respond(resp);
                        } else {
                            let resp = apply_cors(
                                Response::from_string("Error").with_status_code(500),
                                origin.as_deref(),
                                host.as_deref(),
                            );
                            let _ = request.respond(resp);
                        }
                    } else {
                        let resp = apply_cors(
                            Response::from_string("Not Found").with_status_code(404),
                            origin.as_deref(),
                            host.as_deref(),
                        );
                        let _ = request.respond(resp);
                    }
                }
                // ─── Team-Modus (P2P) ───────────────────────────────────────
                (Method::Post, p) if p.starts_with("/team/") => {
                    let endpoint = p.trim_start_matches("/team/").to_string();
                    let (state_token, role_active) = {
                        let st = server_handle.state::<AppState>();
                        let team = st.team.lock().unwrap();
                        (team.token.clone(), team.active && team.role == crate::team::TeamRole::Host)
                    };
                    if !team_authorized(&request, &state_token) {
                        let _ = request.respond(unauthorized(None, None));
                        continue;
                    }

                    // Body lesen (JSON oder raw)
                    let mut raw_body: Vec<u8> = Vec::new();
                    let _ = request.as_reader().read_to_end(&mut raw_body);
                    let body_str = String::from_utf8_lossy(&raw_body).to_string();
                    let json: serde_json::Value =
                        serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

                    let st = server_handle.state::<AppState>();

                    match endpoint.as_str() {
                        "join" => {
                            if !role_active {
                                let _ = request.respond(json_response(serde_json::json!({"error":"team inactive"}).to_string()).with_status_code(403));
                                continue;
                            }
                            let token = json.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let team_id = json.get("team_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let addr = json.get("addr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let result = {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_join(&mut team, &token, &team_id, &node_id, &name, &addr)
                            };
                            match result {
                                Ok(snapshot) => {
                                    crate::team::emit_team(&server_handle);
                                    let _ = request.respond(json_response(serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())));
                                }
                                Err(e) => {
                                    let _ = request.respond(json_response(serde_json::json!({"error": e}).to_string()).with_status_code(403));
                                }
                            }
                        }
                        "leave" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_leave(&mut team, &node_id);
                            }
                            crate::team::emit_team(&server_handle);
                            let _ = request.respond(json_response("OK".to_string()));
                        }
                        "playlist" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let source = json.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let entries: Vec<(String, String)> = json.get("entries")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|e| {
                                    let pair = e.as_array()?;
                                    let title = pair.first()?.as_str().unwrap_or("").to_string();
                                    let url = pair.get(1)?.as_str()?.to_string();
                                    Some((title, url))
                                }).collect())
                                .unwrap_or_default();
                            let added = {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_add_playlist(&mut team, &node_id, &source, entries)
                            };
                            crate::team::emit_team(&server_handle);
                            let _ = request.respond(json_response(added.to_string()));
                        }
                        "claim" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let task = {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_claim(&mut team, &node_id)
                            };
                            crate::team::emit_team(&server_handle);
                            let _ = request.respond(json_response(serde_json::to_string(&task).unwrap_or_else(|_| "null".to_string())));
                        }
                        "progress" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let task_id = json.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let percent = json.get("percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_progress(&mut team, &node_id, &task_id, percent);
                            }
                            let _ = request.respond(json_response("OK".to_string()));
                        }
                        "done" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let task_id = json.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let file_name = json.get("file_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_done(&mut team, &node_id, &task_id, &file_name);
                            }
                            crate::team::emit_team(&server_handle);
                            let _ = request.respond(json_response("OK".to_string()));
                        }
                        "fail" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let task_id = json.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let blocked = json.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
                            let error = json.get("error").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_fail(&mut team, &node_id, &task_id, blocked, &error);
                            }
                            crate::team::emit_team(&server_handle);
                            let _ = request.respond(json_response("OK".to_string()));
                        }
                        "heartbeat" => {
                            let node_id = json.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let blocked = json.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
                            {
                                let mut team = st.team.lock().unwrap();
                                crate::team::coord_heartbeat(&mut team, &node_id, blocked);
                            }
                            let _ = request.respond(json_response("OK".to_string()));
                        }
                        "file" => {
                            // fertige Datei vom Member empfangen
                            let query = full_url.split_once('?').map(|x| x.1).unwrap_or("");
                            let mut name = String::new();
                            for pair in query.split('&') {
                                if let Some((k, v)) = pair.split_once('=') {
                                    if k == "name" {
                                        name = urlencoding::decode(v).map(|s| s.into_owned()).unwrap_or_default();
                                    }
                                }
                            }
                            if !is_safe_filename(&name) {
                                let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400));
                                continue;
                            }
                            // Pfad in der bereits gelesenen `raw_body`...
                            let folder = {
                                // bevorzugt Team-Ordner, sonst Download-Ordner
                                let team_folder = st.team.lock().unwrap().folder.clone();
                                if !team_folder.is_empty() { team_folder } else { st.config.lock().unwrap().download_folder.clone() }
                            };
                            let _ = std::fs::create_dir_all(&folder);
                            let dest = std::path::Path::new(&folder).join(&name);
                            // Datei-Upload kommt als raw body; raw_body hat sie bereits
                            let payload = raw_body.clone();
                            match std::fs::write(&dest, payload) {
                                Ok(_) => { let _ = request.respond(json_response("OK".to_string())); }
                                Err(e) => { let _ = request.respond(Response::from_string(format!("write error: {}", e)).with_status_code(500)); }
                            }
                        }
                        _ => { let _ = request.respond(Response::from_string("Not Found").with_status_code(404)); }
                    }
                }
                (Method::Get, p) if p == "/team/state" => {
                    let (state_token, snapshot) = {
                        let st = server_handle.state::<AppState>();
                        let team = st.team.lock().unwrap();
                        (team.token.clone(), team.clone())
                    };
                    if !team_authorized(&request, &state_token) {
                        let _ = request.respond(unauthorized(None, None));
                        continue;
                    }
                    let _ = request.respond(json_response(serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())));
                }
                (Method::Get, p) if p.starts_with("/team/file/") => {
                    let (state_token, folder) = {
                        let st = server_handle.state::<AppState>();
                        let team = st.team.lock().unwrap();
                        let cfg_folder = st.config.lock().unwrap().download_folder.clone();
                        let folder = if !team.folder.is_empty() { team.folder.clone() } else { cfg_folder };
                        (team.token.clone(), folder)
                    };
                    if !team_authorized(&request, &state_token) {
                        let _ = request.respond(unauthorized(None, None));
                        continue;
                    }
                    let raw = p.trim_start_matches("/team/file/").split('?').next().unwrap_or("");
                    let decoded = urlencoding::decode(raw).unwrap_or_else(|_| std::borrow::Cow::Borrowed(raw)).into_owned();
                    if !is_safe_filename(&decoded) {
                        let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400));
                        continue;
                    }
                    let file_path = std::path::Path::new(&folder).join(&decoded);
                    if file_path.exists() && file_path.is_file() {
                        if let Ok(file) = std::fs::File::open(&file_path) {
                            let _ = request.respond(Response::from_file(file));
                        } else {
                            let _ = request.respond(Response::from_string("Cannot read file").with_status_code(500));
                        }
                    } else {
                        let _ = request.respond(Response::from_string("File not found").with_status_code(404));
                    }
                }
                _ => {
                    let _ =
                        request.respond(Response::from_string("Not Found").with_status_code(404));
                }
            }
        }
    });
    Ok(())
}

// ─── Team-Hilfen ─────────────────────────────────────────────────────────────

fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body).with_header(
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
    ).with_header(
        tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
    )
}

fn team_authorized(request: &tiny_http::Request, team_token: &str) -> bool {
    if team_token.is_empty() {
        return false; // Team-Endpunkte immer token-geschützt
    }
    // X-Team-Token Header
    for header in request.headers() {
        if header.field.as_str().as_bytes().eq_ignore_ascii_case(b"x-team-token") {
            let t = header.value.as_str().trim();
            if t == team_token { return true; }
        }
    }
    // Bearer / X-Remote-Token / ?token=
    if let Some(t) = request_token(request) {
        if t == team_token { return true; }
    }
    // token in query (z.B. Datei-Upload)
    if let Some(tok) = extract_query_token(request.url()) {
        if tok == team_token { return true; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filename_rejects_traversal_and_reserved_names() {
        assert!(is_safe_filename("track_01.mp3"));
        assert!(is_safe_filename("Ärzte - Schrei nach Liebe.flac"));
        assert!(!is_safe_filename(""));
        assert!(!is_safe_filename("../secret.txt"));
        assert!(!is_safe_filename("dir/file.mp3"));
        assert!(!is_safe_filename("dir\\file.mp3"));
        assert!(!is_safe_filename("C:file.mp3"));
        assert!(!is_safe_filename(".hidden"));
        assert!(!is_safe_filename("trailing."));
        assert!(!is_safe_filename("trailing "));
        assert!(!is_safe_filename("bad\x07name.mp3"));
        assert!(!is_safe_filename("CON.mp3"));
        assert!(!is_safe_filename("nul"));
        assert!(!is_safe_filename("com1.txt"));
        assert!(!is_safe_filename("LPT9.wav"));
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(ct_eq("abcdef1234", "abcdef1234"));
        assert!(!ct_eq("abcdef1234", "abcdef1235"));
        assert!(!ct_eq("short", "longer"));
    }

    #[test]
    fn trusted_origin_checks_extensions_loopback_and_host() {
        assert!(is_trusted_origin("chrome-extension://abcdefghijklmnop", None));
        assert!(is_trusted_origin("moz-extension://12345678", None));
        assert!(is_trusted_origin("http://localhost:3030", None));
        assert!(is_trusted_origin("http://127.0.0.1:3030", None));
        assert!(is_trusted_origin("http://192.168.1.42:3030", Some("192.168.1.42:3030")));
        assert!(!is_trusted_origin("https://evil.example.com", Some("192.168.1.42:3030")));
        assert!(!is_trusted_origin("http://localhost:8080", None));
    }

    #[test]
    fn byte_range_parsing_handles_standard_and_edge_cases() {
        assert_eq!(parse_byte_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_byte_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_byte_range("bytes=500-5000", 1000), Some((500, 999)));
        assert_eq!(parse_byte_range("bytes=-200", 1000), Some((800, 999)));
        assert_eq!(parse_byte_range("bytes=-5000", 1000), Some((0, 999)));
        assert_eq!(parse_byte_range("bytes=1000-", 1000), None);
        assert_eq!(parse_byte_range("bytes=500-100", 1000), None);
        assert_eq!(parse_byte_range("bytes=0-0", 0), None);
        assert_eq!(parse_byte_range("bytes=-0", 1000), None);
    }

    #[test]
    fn extract_query_token_decodes_value() {
        assert_eq!(
            extract_query_token("/files?foo=1&token=abc%20123&bar=2"),
            Some("abc 123".to_string())
        );
        assert_eq!(extract_query_token("/files?token="), None);
        assert_eq!(extract_query_token("/files"), None);
    }
}
