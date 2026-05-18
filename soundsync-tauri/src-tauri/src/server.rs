use crate::models::{AppState, RemotePayload};
use tauri::{AppHandle, Manager, Emitter};
use tiny_http::{Method, Response, Server};
use std::thread;

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
    let server_handle = app.clone();
    thread::spawn(move || {
        let addr = "0.0.0.0:3030";

        let server = match Server::http(addr) {
            Ok(s) => s,
            Err(_) => return,
        };

        for mut request in server.incoming_requests() {
            match (request.method(), request.url()) {
                (&Method::Get, "/") => {
                    let html = include_str!("remote.html");
                    let response = Response::from_string(html).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"text/html; charset=utf-8"[..],
                        )
                        .unwrap(),
                    );
                    let _ = request.respond(response);
                }
                (&Method::Get, "/status") => {
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
                (&Method::Post, "/send") => {
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
                (&Method::Options, _) => {
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
                                &b"Content-Type"[..],
                            )
                            .unwrap(),
                        );
                    let _ = request.respond(response);
                }
                (&Method::Get, "/files") => {
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
                (&Method::Get, url) if url.starts_with("/download/") => {
                    let filename = url.replace("/download/", "");
                    let decoded_filename = urlencoding::decode(&filename).unwrap_or_else(|_| std::borrow::Cow::Borrowed(&filename)).into_owned();
                    
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
                (&Method::Delete, url) if url.starts_with("/delete/") => {
                    let filename = url.replace("/delete/", "");
                    let decoded_filename = urlencoding::decode(&filename)
                        .unwrap_or_else(|_| std::borrow::Cow::Borrowed(&filename))
                        .into_owned();
                    
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
