use crate::models::{generate_remote_token, AppState, RemotePayload};
use tauri::{AppHandle, Manager, Emitter};
use tiny_http::{Method, Response, Server};
use std::thread;

/// Dateiname gegen Path-Traversal prüfen. Nur flache Namen, keine Separatoren.
pub fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return false;
    }
    if name.starts_with('.') {
        return false;
    }
    // Windows-reservierte Namen + absolute Pfade abweisen
    if name.contains(':') {
        return false;
    }
    true
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
            if let Some(tok) = val.strip_prefix("Bearer ").or_else(|| val.strip_prefix("bearer ")) {
                let t = tok.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
        if header.field.as_str().as_bytes().eq_ignore_ascii_case(b"x-remote-token") {
            let t = header.value.as_str().trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    let url = request.url();
    extract_query_token(url)
}

fn is_loopback_request(request: &tiny_http::Request) -> bool {
    if let Some(addr) = request.remote_addr() {
        return addr.ip().is_loopback();
    }
    // Falls tiny_http keine Remote-Adresse liefert: sicherheitshalber als extern behandeln.
    false
}

/// Schützende Endpunkte: Loopback (Extension/lokal) immer ok, LAN braucht Token.
fn is_authorized(request: &tiny_http::Request, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    if is_loopback_request(request) {
        return true;
    }
    if let Some(t) = request_token(request) {
        return t == token;
    }
    false
}

fn unauthorized() -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string("Unauthorized")
        .with_status_code(401)
        .with_header(
            tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
        )
}

#[tauri::command]
pub fn ss_get_remote_token(state: tauri::State<AppState>) -> Result<String, String> {
    Ok(state.remote_token.lock().unwrap().clone())
}

#[tauri::command]
pub fn ss_get_local_ip() -> Result<String, String> {
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
pub fn ss_start_remote_server(app: AppHandle) -> Result<(), String> {
    // Token einmalig erzeugen (LAN-Schutz). Loopback (Extension) braucht keins.
    {
        let state = app.state::<AppState>();
        let mut tok = state.remote_token.lock().unwrap();
        if tok.is_empty() {
            *tok = generate_remote_token();
        }
    }
    let server_handle = app.clone();
    thread::spawn(move || {
        let addr = "0.0.0.0:3030";

        let server = match Server::http(addr) {
            Ok(s) => s,
            Err(_) => return,
        };

        for mut request in server.incoming_requests() {
            // Token für diesen Request aus State lesen (billig, kurze Lock-Dauer)
            let token = {
                let state = server_handle.state::<AppState>();
                let x = state.remote_token.lock().unwrap().clone();
                x
            };
            // URL ohne Query-String für Routing (Token bleibt in request.url() für Auth prüfbar)
            let full_url = request.url().to_string();
            let path = full_url.split('?').next().unwrap_or("/").to_string();
            let method = request.method().clone();
            let route = (method.clone(), path.clone());
            match route {
                (Method::Get, p) if p == "/" => {
                    // Token serverseitig injizieren (kein Leak über QR-Dienst nötig)
                    let html = include_str!("remote.html").replacen(
                        "__REMOTE_TOKEN__",
                        &token,
                        1,
                    );
                    let response = Response::from_string(html).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"text/html; charset=utf-8"[..],
                        )
                        .unwrap(),
                    );
                    let _ = request.respond(response);
                }
                (Method::Get, p) if p == "/status" => {
                    let state = server_handle.state::<AppState>();
                    let progress = state.current_progress.lock().unwrap().clone();
                    let json =
                        serde_json::to_string(&progress).unwrap_or_else(|_| "null".to_string());
                    let response = Response::from_string(json)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Origin"[..],
                                &b"*"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Get, p) if p == "/remote-config" => {
                    let language = {
                        let state = server_handle.state::<AppState>();
                        let config = state.config.lock().unwrap();
                        config.language.clone()
                    };
                    let json = serde_json::json!({ "language": language }).to_string();
                    let response = Response::from_string(json)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Origin"[..],
                                &b"*"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Get, p) if p == "/queue" => {
                    if !is_authorized(&request, &token) {
                        let _ = request.respond(unauthorized());
                        continue;
                    }
                    let queue = {
                        let state = server_handle.state::<AppState>();
                        let queue = state.remote_queue.lock().unwrap();
                        queue.clone()
                    };
                    let json = serde_json::to_string(&queue).unwrap_or_else(|_| "[]".to_string());
                    let response = Response::from_string(json)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Origin"[..],
                                &b"*"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Post, p) if p == "/send" => {
                    if !is_authorized(&request, &token) {
                        let _ = request.respond(unauthorized());
                        continue;
                    }
                    let mut content = String::new();
                    let _ = request.as_reader().read_to_string(&mut content);

                    if let Ok(payload) = serde_json::from_str::<RemotePayload>(&content) {
                        println!("Remote API received valid payload: {:?}", payload);
                        if let Err(e) =
                            server_handle.emit_to("main", "remote-url-received", payload)
                        {
                            println!("Failed to emit event to main window: {:?}", e);
                        }
                        let _ = request.respond(
                            Response::from_string("OK").with_header(
                                tiny_http::Header::from_bytes(
                                    &b"Access-Control-Allow-Origin"[..],
                                    &b"*"[..],
                                )
                                .unwrap(),
                            ),
                        );
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
                            if let Err(e) = server_handle.emit_to(
                                "main",
                                "remote-url-received",
                                payload.clone(),
                            ) {
                                println!("Failed to emit event (fallback) to main window: {:?}", e);
                            }
                            let _ = request.respond(
                                Response::from_string("OK").with_header(
                                    tiny_http::Header::from_bytes(
                                        &b"Access-Control-Allow-Origin"[..],
                                        &b"*"[..],
                                    )
                                    .unwrap(),
                                ),
                            );
                        } else {
                            let _ = request.respond(
                                Response::from_string("Empty URL")
                                    .with_status_code(400)
                                    .with_header(
                                        tiny_http::Header::from_bytes(
                                            &b"Access-Control-Allow-Origin"[..],
                                            &b"*"[..],
                                        )
                                        .unwrap(),
                                    ),
                            );
                        }
                    }
                }
                (Method::Options, _) => {
                    let response = Response::from_string("")
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Origin"[..],
                                &b"*"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Methods"[..],
                                &b"POST, GET, OPTIONS, DELETE"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Headers"[..],
                                &b"Content-Type, Authorization, X-Remote-Token"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Get, p) if p == "/files" => {
                    if !is_authorized(&request, &token) {
                        let _ = request.respond(unauthorized());
                        continue;
                    }
                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = state.config.lock().unwrap().download_folder.clone();
                        f
                    };
                    
                    let mut files_list: Vec<String> = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(folder) {
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_file() {
                                    if let Some(name) = entry.file_name().to_str() {
                                        if !name.ends_with(".part") && !name.ends_with(".ytdl") {
                                            files_list.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    let json = serde_json::to_string(&files_list).unwrap_or_else(|_| "[]".to_string());
                    let response = Response::from_string(json)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Access-Control-Allow-Origin"[..],
                                &b"*"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (Method::Get, p) if p.starts_with("/download/") => {
                    if !is_authorized(&request, &token) {
                        let _ = request.respond(unauthorized());
                        continue;
                    }
                    let raw = p.trim_start_matches("/download/");
                    // Query war schon abgetrennt (path), trotzdem defensiv splitten
                    let raw = raw.split('?').next().unwrap_or("");
                    let decoded_filename = urlencoding::decode(raw).unwrap_or_else(|_| std::borrow::Cow::Borrowed(raw)).into_owned();
                    if !is_safe_filename(&decoded_filename) {
                        let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400));
                        continue;
                    }
                    
                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = state.config.lock().unwrap().download_folder.clone();
                        f
                    };
                    
                    let file_path = std::path::Path::new(&folder).join(&decoded_filename);
                    
                    if file_path.exists() && file_path.is_file() {
                        if let Ok(file) = std::fs::File::open(&file_path) {
                            let response = Response::from_file(file)
                                .with_header(
                                    tiny_http::Header::from_bytes(
                                        &b"Access-Control-Allow-Origin"[..],
                                        &b"*"[..],
                                    )
                                    .unwrap(),
                                );
                            let _ = request.respond(response);
                        } else {
                            let _ = request.respond(Response::from_string("Cannot read file").with_status_code(500));
                        }
                    } else {
                        let _ = request.respond(Response::from_string("File not found").with_status_code(404));
                    }
                }
                (Method::Delete, p) if p.starts_with("/delete/") => {
                    if !is_authorized(&request, &token) {
                        let _ = request.respond(unauthorized());
                        continue;
                    }
                    let raw = p.trim_start_matches("/delete/");
                    let raw = raw.split('?').next().unwrap_or("");
                    let decoded_filename = urlencoding::decode(raw)
                        .unwrap_or_else(|_| std::borrow::Cow::Borrowed(raw))
                        .into_owned();
                    if !is_safe_filename(&decoded_filename) {
                        let _ = request.respond(Response::from_string("Invalid filename").with_status_code(400));
                        continue;
                    }
                    
                    let folder = {
                        let state = server_handle.state::<AppState>();
                        let f = state.config.lock().unwrap().download_folder.clone();
                        f
                    };
                    
                    let file_path = std::path::Path::new(&folder).join(&decoded_filename);
                    
                    if file_path.exists() && file_path.is_file() {
                        if let Ok(_) = std::fs::remove_file(&file_path) {
                            let response = Response::from_string("OK").with_header(
                                tiny_http::Header::from_bytes(
                                    &b"Access-Control-Allow-Origin"[..],
                                    &b"*"[..],
                                )
                                .unwrap(),
                            );
                            let _ = request.respond(response);
                        } else {
                            let _ = request.respond(Response::from_string("Error").with_status_code(500));
                        }
                    } else {
                        let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
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
