// ─── P2P Team-Download ("Team-Modus") ─────────────────────────────────────────
//
// Zwei (oder mehr) PCs verbinden sich direkt miteinander – kostenlos, ohne
// zentralen Server. Jede App betreibt bereits einen kleinen LAN-HTTP-Server
// (Port 3030, siehe `server.rs`). Auf dessen Basis bauen wir ein Team-Protokoll:
//
//   * Pairing über einen generierten Link:  soundsync-team://IP:PORT/TEAM_ID?token=XXX
//   * Jeder Peer darf eine *eigene* Playlist beisteuern; daraus wird eine
//     gemeinsame, zusammengeführte Liste (Deduplizierung über die URL).
//   * Die gemeinsame Liste wird kooperativ heruntergeladen ("Work-Stealing"):
//     wer frei ist, holt sich das nächste noch offene Stück.
//   * Failover: meldet ein PC eine YouTube-Sperre (HTTP 429 / Rate-Limit /
//     Bot-Check / 403) oder fällt aus (kein Heartbeat mehr), werden seine noch
//     nicht fertigen Aufgaben automatisch neu in die Warteschlange gestellt
//     und vom anderen PC übernommen. Ein gesperrter PC pausiert kurz und
//     reiht sich danach wieder ein.
//
// Einer der PCs ist "Host"/Koordinator und hält den kanonischen Zustand; der
// andere ist "Member" und spricht per HTTP mit dem Host. Beide arbeiten aktiv
// als Worker. Alles läuft über dasselbe Token wie der Paarungs-Link.

use crate::models::{generate_remote_token, AppState, DownloadRequest};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

pub const TEAM_PORT: u16 = 3030;
const SCHEME: &str = "soundsync-team";
const HEARTBEAT_MS: u64 = 5_000;
const MEMBER_OFFLINE_MS: u64 = 30_000;
const TASK_STALE_MS: u64 = 300_000; // 5 Min ohne Fortschritt -> neu verteilen
const BLOCKED_COOLDOWN_MS: u64 = 120_000; // gesperrter PC pausiert 2 Min
const MAX_ATTEMPTS: u32 = 4;
const CLAIM_IDLE_MS: u64 = 700;

// ─── Datentypen ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    #[default]
    None,
    Host,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub node_id: String,
    pub name: String,
    pub addr: String,
    pub last_seen: u64,
    pub blocked: bool,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub id: String,
    pub url: String,
    pub title: String,
    /// node_id, der diese Playlist beigesteuert hat
    pub origin: String,
    /// Name/URL der Quell-Playlist
    pub source: String,
    /// "queued" | "assigned" | "downloading" | "done" | "failed"
    pub status: String,
    pub assigned_to: String,
    pub attempts: u32,
    pub last_update: u64,
    pub percent: f64,
    pub error: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamState {
    pub active: bool,
    pub role: TeamRole,
    pub node_id: String,
    pub node_name: String,
    pub token: String,
    pub team_id: String,
    pub self_addr: String,
    pub host_addr: String,
    pub members: Vec<TeamMember>,
    pub tasks: Vec<TeamTask>,
    pub format: String,
    pub quality: String,
    pub folder: String,
    pub share_files: bool,
    /// Wir selbst sind von YouTube gesperrt (pausieren bis hierhin)
    pub blocked_until: u64,
    pub worker_running: bool,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInvite {
    pub link: String,
    pub token: String,
    pub addr: String,
    pub team_id: String,
    pub node_id: String,
}

// ─── Zeit ────────────────────────────────────────────────────────────────────

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn short_id() -> String {
    generate_remote_token().chars().take(16).collect()
}

// ─── Koordinator-Logik (läuft auf dem Host) ──────────────────────────────────

pub fn coord_join(
    team: &mut TeamState,
    token: &str,
    team_id: &str,
    node_id: &str,
    name: &str,
    addr: &str,
) -> Result<TeamState, String> {
    if team.role != TeamRole::Host || !team.active {
        return Err("Team ist nicht aktiv".to_string());
    }
    if token != team.token {
        return Err("Falsches Token".to_string());
    }
    if !team_id.is_empty() && team_id != team.team_id {
        return Err("Falsche Team-ID".to_string());
    }
    if node_id.is_empty() || node_id == team.node_id {
        return Err("Ungültige Node-ID".to_string());
    }

    let now = now_ms();
    if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) {
        m.name = name.to_string();
        m.addr = addr.to_string();
        m.last_seen = now;
        m.online = true;
    } else {
        team.members.push(TeamMember {
            node_id: node_id.to_string(),
            name: name.to_string(),
            addr: addr.to_string(),
            last_seen: now,
            blocked: false,
            online: true,
        });
    }
    Ok(team.clone())
}

pub fn coord_leave(team: &mut TeamState, node_id: &str) {
    // Mitglied entfernen + seine offenen Aufgaben neu verteilen
    requeue_node(team, node_id);
    team.members.retain(|m| m.node_id != node_id);
}

pub fn coord_heartbeat(team: &mut TeamState, node_id: &str, blocked: bool) {
    let now = now_ms();
    if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) {
        m.last_seen = now;
        m.online = true;
        m.blocked = blocked;
    }
    // Lebenszeichen halten zugewiesene Aufgaben "frisch" (gegen Stale-Requeue)
    for t in team.tasks.iter_mut() {
        if t.assigned_to == node_id && (t.status == "assigned" || t.status == "downloading") {
            t.last_update = now;
        }
    }
}

pub fn coord_add_playlist(
    team: &mut TeamState,
    origin: &str,
    source: &str,
    entries: Vec<(String, String)>,
) -> usize {
    let now = now_ms();
    let mut added = 0usize;
    for (title, url) in entries {
        let url = url.trim().to_string();
        if url.is_empty() {
            continue;
        }
        // Deduplizieren über die ganze kombinierte Liste
        if team.tasks.iter().any(|t| t.url == url) {
            continue;
        }
        let title = if title.trim().is_empty() {
            url.clone()
        } else {
            title.trim().to_string()
        };
        team.tasks.push(TeamTask {
            id: short_id(),
            url,
            title,
            origin: origin.to_string(),
            source: source.to_string(),
            status: "queued".to_string(),
            assigned_to: String::new(),
            attempts: 0,
            last_update: now,
            percent: 0.0,
            error: String::new(),
            file_name: String::new(),
        });
        added += 1;
    }
    added
}

fn node_can_take_work(team: &TeamState, node_id: &str) -> bool {
    if node_id == team.node_id {
        return now_ms() >= team.blocked_until;
    }
    team.members
        .iter()
        .find(|m| m.node_id == node_id)
        .map(|m| m.online && !m.blocked)
        .unwrap_or(false)
}

pub fn coord_claim(team: &mut TeamState, node_id: &str) -> Option<TeamTask> {
    // erst verwaiste Aufgaben zurückholen
    coord_sweep(team);

    if !node_can_take_work(team, node_id) {
        return None;
    }
    let now = now_ms();
    // älteste offene Aufgabe zuerst; niemals eine Aufgabe an denselben,
    // gerade gesperrten Node wiedergeben
    let idx = team
        .tasks
        .iter()
        .position(|t| t.status == "queued")?;
    let task = &mut team.tasks[idx];
    task.status = "assigned".to_string();
    task.assigned_to = node_id.to_string();
    task.attempts += 1;
    task.last_update = now;
    task.percent = 0.0;
    task.error.clear();
    Some(task.clone())
}

pub fn coord_progress(team: &mut TeamState, node_id: &str, task_id: &str, percent: f64) {
    let now = now_ms();
    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        // nur der zugewiesene Worker darf updaten
        if t.assigned_to == node_id {
            t.status = "downloading".to_string();
            t.percent = percent.clamp(0.0, 100.0);
            t.last_update = now;
        }
    }
}

pub fn coord_done(team: &mut TeamState, node_id: &str, task_id: &str, file_name: &str) {
    let now = now_ms();
    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        if t.assigned_to == node_id || t.assigned_to.is_empty() {
            t.status = "done".to_string();
            t.percent = 100.0;
            t.file_name = file_name.to_string();
            t.error.clear();
            t.last_update = now;
        }
    }
}

pub fn coord_fail(team: &mut TeamState, node_id: &str, task_id: &str, blocked: bool, error: &str) {
    let now = now_ms();

    if blocked {
        // gesperrten Node markieren + seine offenen Aufgaben weitergeben
        if node_id == team.node_id {
            team.blocked_until = now + BLOCKED_COOLDOWN_MS;
        }
        if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) {
            m.blocked = true;
        }
        requeue_node(team, node_id);
    }

    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        if t.status == "done" {
            return;
        }
        t.error = error.chars().take(400).collect();
        t.assigned_to.clear();
        t.last_update = now;
        if t.attempts >= MAX_ATTEMPTS {
            t.status = "failed".to_string();
        } else {
            // für den anderen PC wieder freigeben (Failover)
            t.status = "queued".to_string();
        }
    }

    // falls dieser Track schon anderweitig fertig ist, nichts weiter tun
}

/// Aufgaben eines Nodes zurück in die Warteschlange (PC aus / gesperrt).
fn requeue_node(team: &mut TeamState, node_id: &str) {
    let now = now_ms();
    for t in team.tasks.iter_mut() {
        if t.assigned_to == node_id && (t.status == "assigned" || t.status == "downloading") {
            t.assigned_to.clear();
            t.status = "queued".to_string();
            t.last_update = now;
            // Versuch zurückgeben, weil der Node nicht schuld ist
            t.attempts = t.attempts.saturating_sub(1);
        }
    }
}

/// Verwaiste / hängende Aufgaben neu verteilen + Mitglieder-Status pflegen.
pub fn coord_sweep(team: &mut TeamState) {
    let now = now_ms();

    // Mitglieder offline markieren + blockierten Status erneuern
    for m in team.members.iter_mut() {
        if m.online && now.saturating_sub(m.last_seen) > MEMBER_OFFLINE_MS {
            m.online = false;
        }
    }
    // eigener Block abklingen
    if team.blocked_until != 0 && now >= team.blocked_until {
        team.blocked_until = 0;
    }
    // blockierte Member wieder freigeben, wenn sie wieder heartbeaten -> in coord_heartbeat

    // Offline- / gesperrte Nodes ihre Aufgaben abnehmen
    let offline: Vec<String> = team
        .members
        .iter()
        .filter(|m| !m.online || m.blocked)
        .map(|m| m.node_id.clone())
        .collect();
    let self_blocked = now < team.blocked_until;
    for node in offline {
        requeue_node(team, &node);
    }
    if self_blocked {
        let self_id = team.node_id.clone();
        requeue_node(team, &self_id);
    }

    // hängende Aufgaben (kein Fortschritt) neu verteilen
    for t in team.tasks.iter_mut() {
        if (t.status == "assigned" || t.status == "downloading")
            && now.saturating_sub(t.last_update) > TASK_STALE_MS
        {
            t.assigned_to.clear();
            t.status = "queued".to_string();
            t.last_update = now;
            t.attempts = t.attempts.saturating_sub(1);
        }
    }
}

pub fn team_all_done(team: &TeamState) -> bool {
    !team.tasks.is_empty()
        && team
            .tasks
            .iter()
            .all(|t| t.status == "done" || t.status == "failed")
}

// ─── YouTube-Sperre erkennen ─────────────────────────────────────────────────

pub fn is_blocked_error(err: &str) -> bool {
    let e = err.to_lowercase();
    const MARKERS: &[&str] = &[
        "429",
        "too many requests",
        "rate limit",
        "ratelimit",
        "rate-limit",
        "http error 403",
        "http error 429",
        "http error 503",
        "blocked",
        "you have been blocked",
        "sign in to confirm",
        "confirm you're not a bot",
        "confirm you are not a bot",
        "not a bot",
        "captcha",
        "your ip",
        "ip address",
        "this ip",
        "temporarily blocked",
        "unusual traffic",
        "verify you are human",
    ];
    MARKERS.iter().any(|m| e.contains(m))
}

// ─── Hilfen für State-Zugriff ────────────────────────────────────────────────

fn team_mutex(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn emit_team(app: &AppHandle) {
    let snapshot = {
        let st = team_mutex(app);
        let team = st.team.lock().unwrap();
        team.clone()
    };
    let _ = app.emit("team-state-changed", snapshot);
}

fn is_host(app: &AppHandle) -> bool {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    team.role == TeamRole::Host
}

fn self_node_id(app: &AppHandle) -> String {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    team.node_id.clone()
}

fn host_http_base(app: &AppHandle) -> Option<String> {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    if team.role == TeamRole::Host {
        None
    } else {
        Some(normalize_base(&team.host_addr))
    }
}

/// Adresse normalisieren: http/https-Scheme erhalten, sonst http:// voranstellen, trailing / weg.
pub fn normalize_base(addr: &str) -> String {
    let a = addr.trim().trim_end_matches('/').to_string();
    if a.starts_with("http://") || a.starts_with("https://") {
        a
    } else {
        format!("http://{}", a)
    }
}

fn team_token(app: &AppHandle) -> String {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    team.token.clone()
}

// ─── HTTP zum Host (Member-Seite) ────────────────────────────────────────────

async fn http_post_json(url: String, token: String, body: serde_json::Value) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let resp = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("X-Team-Token", &token)
            .timeout(std::time::Duration::from_secs(15))
            .send_string(&body.to_string());
        match resp {
            Ok(r) => r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, r)) => {
                Err(format!("HTTP {}: {}", code, r.into_string().unwrap_or_default()))
            }
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn http_get_json(url: String, token: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let resp = ureq::get(&url)
            .set("X-Team-Token", &token)
            .timeout(std::time::Duration::from_secs(15))
            .call();
        match resp {
            Ok(r) => r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, r)) => {
                Err(format!("HTTP {}: {}", code, r.into_string().unwrap_or_default()))
            }
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Worker-Aktionen (Host direkt, Member per HTTP) ──────────────────────────

async fn worker_claim(app: &AppHandle, node_id: &str) -> Option<TeamTask> {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        let t = coord_claim(&mut team, node_id);
        drop(team);
        emit_team(app);
        t
    } else {
        let base = host_http_base(app)?;
        let token = team_token(app);
        let url = format!("{}/team/claim", base);
        let body = serde_json::json!({ "node_id": node_id });
        let resp = http_post_json(url, token, body).await.ok()?;
        serde_json::from_str::<Option<TeamTask>>(&resp).ok().flatten()
    }
}

async fn worker_heartbeat(app: &AppHandle, node_id: &str, blocked: bool) {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        coord_heartbeat(&mut team, node_id, blocked);
        // Host selbst als Mitglied pflegen
        let self_id = team.node_id.clone();
        let self_name = team.node_name.clone();
        let self_addr = team.self_addr.clone();
        if !team.members.iter().any(|m| m.node_id == self_id) {
            team.members.push(TeamMember {
                node_id: self_id.clone(),
                name: self_name,
                addr: self_addr,
                last_seen: now_ms(),
                blocked,
                online: true,
            });
        } else if let Some(m) = team.members.iter_mut().find(|m| m.node_id == self_id) {
            m.last_seen = now_ms();
            m.online = true;
            m.blocked = blocked;
        }
        // eigener Block
        let now = now_ms();
        let self_blocked = now < team.blocked_until;
        if let Some(m) = team.members.iter_mut().find(|m| m.node_id == self_id) {
            m.blocked = blocked || self_blocked;
        }
    } else if let Some(base) = host_http_base(app) {
        let token = team_token(app);
        let url = format!("{}/team/heartbeat", base);
        let body = serde_json::json!({ "node_id": node_id, "blocked": blocked });
        let _ = http_post_json(url, token, body).await;
    }
}

async fn worker_progress(app: &AppHandle, node_id: &str, task_id: &str, percent: f64) {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        coord_progress(&mut team, node_id, task_id, percent);
    } else if let Some(base) = host_http_base(app) {
        let token = team_token(app);
        let url = format!("{}/team/progress", base);
        let body = serde_json::json!({ "node_id": node_id, "task_id": task_id, "percent": percent });
        let _ = http_post_json(url, token, body).await;
    }
}

async fn worker_done(app: &AppHandle, node_id: &str, task_id: &str, file_name: &str) {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        coord_done(&mut team, node_id, task_id, file_name);
    } else if let Some(base) = host_http_base(app) {
        let token = team_token(app);
        let url = format!("{}/team/done", base);
        let body = serde_json::json!({ "node_id": node_id, "task_id": task_id, "file_name": file_name });
        let _ = http_post_json(url, token, body).await;
    }
    emit_team(app);
}

async fn worker_fail(app: &AppHandle, node_id: &str, task_id: &str, blocked: bool, error: &str) {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        coord_fail(&mut team, node_id, task_id, blocked, error);
    } else if let Some(base) = host_http_base(app) {
        let token = team_token(app);
        let url = format!("{}/team/fail", base);
        let body = serde_json::json!({
            "node_id": node_id, "task_id": task_id, "blocked": blocked, "error": error
        });
        let _ = http_post_json(url, token, body).await;
    }
    emit_team(app);
}

async fn worker_all_done(app: &AppHandle) -> bool {
    if is_host(app) {
        let st = team_mutex(app);
        let mut team = st.team.lock().unwrap();
        coord_sweep(&mut team);
        team_all_done(&team)
    } else if let Some(base) = host_http_base(app) {
        let token = team_token(app);
        let url = format!("{}/team/state", base);
        if let Ok(resp) = http_get_json(url, token).await {
            if let Ok(team) = serde_json::from_str::<TeamState>(&resp) {
                return team_all_done(&team);
            }
        }
        false
    } else {
        false
    }
}

/// Fertige Datei zum Host hochladen (gemeinsame Ausgabe).
async fn upload_file_to_host(app: &AppHandle, node_id: &str, path: &str) {
    // nur Member laden hoch; Host hat es lokal schon
    if is_host(app) {
        return;
    }
    let Some(base) = host_http_base(app) else { return };
    let token = team_token(app);
    let file_name = match std::path::Path::new(path).file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_string(),
        None => return,
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    let url = format!(
        "{}/team/file?name={}&node={}&token={}",
        base,
        urlencoding::encode(&file_name),
        urlencoding::encode(node_id),
        urlencoding::encode(&token),
    );
    let _ = tokio::task::spawn_blocking(move || {
        let _ = ureq::post(&url)
            .set("Content-Type", "application/octet-stream")
            .timeout(std::time::Duration::from_secs(120))
            .send_bytes(&bytes);
    })
    .await;
}

// ─── Worker-Loop ─────────────────────────────────────────────────────────────

fn team_download_settings(app: &AppHandle) -> (String, String, String, Option<String>, String, bool) {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    let cfg = st.config.lock().unwrap();
    let format = if team.format.is_empty() { "mp3".to_string() } else { team.format.clone() };
    let quality = if team.quality.is_empty() { "best".to_string() } else { team.quality.clone() };
    let folder = if !team.folder.is_empty() {
        team.folder.clone()
    } else {
        cfg.download_folder.clone()
    };
    let cookies = if cfg.cookies_path.is_empty() { None } else { Some(cfg.cookies_path.clone()) };
    let node_id = team.node_id.clone();
    let share = team.share_files;
    (format, quality, folder, cookies, node_id, share)
}

fn team_active(app: &AppHandle) -> bool {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    team.active && team.worker_running
}

fn self_blocked(app: &AppHandle) -> bool {
    let st = team_mutex(app);
    let team = st.team.lock().unwrap();
    now_ms() < team.blocked_until
}

async fn run_one_task(app: &AppHandle, node_id: &str, task: &TeamTask, total: usize) {
    let (format, quality, folder, cookies, _nid, share) = team_download_settings(app);

    let req = DownloadRequest {
        url: task.url.clone(),
        format,
        folder,
        cookies_path: cookies,
        quality,
    };

    let _ = app.emit("team-task-started", serde_json::json!({
        "task_id": task.id, "title": task.title, "node_id": node_id
    }));

    // Status -> downloading
    worker_progress(app, node_id, &task.id, 0.0).await;

    let total = total.max(1);
    // Laufindex bestmöglich ableiten (nur für Anzeige)
    let track_index = 1usize;

    let result = crate::commands::download_track(
        app.clone(),
        req,
        team_mutex(app),
        track_index,
        total,
        task.title.clone(),
    )
    .await;

    if !team_active(app) {
        return;
    }

    match result {
        Ok(out_path) => {
            let file_name = std::path::Path::new(&out_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            // progress=100 melden
            worker_progress(app, node_id, &task.id, 100.0).await;
            worker_done(app, node_id, &task.id, &file_name).await;
            if share && !out_path.is_empty() {
                upload_file_to_host(app, node_id, &out_path).await;
            }
            let _ = app.emit("team-task-done", serde_json::json!({
                "task_id": task.id, "title": task.title, "node_id": node_id, "file_name": file_name
            }));
        }
        Err(e) => {
            if !team_active(app) {
                return;
            }
            let blocked = is_blocked_error(&e);
            if blocked {
                // selbst als gesperrt markieren + kurz pausieren
                let st = team_mutex(app);
                let mut team = st.team.lock().unwrap();
                team.blocked_until = now_ms() + BLOCKED_COOLDOWN_MS;
                team.last_error = e.chars().take(400).collect();
                drop(team);
                let _ = app.emit("team-blocked", serde_json::json!({
                    "node_id": node_id, "reason": e.chars().take(400).collect::<String>()
                }));
            }
            worker_fail(app, node_id, &task.id, blocked, &e).await;
            let _ = app.emit("team-task-failed", serde_json::json!({
                "task_id": task.id, "title": task.title, "node_id": node_id,
                "blocked": blocked, "error": e.chars().take(400).collect::<String>()
            }));
        }
    }
}

async fn worker_loop(app: AppHandle) {
    let node_id = self_node_id(&app);

    // nicht-abbrechende Downloads zurücksetzen
    {
        let st = app.state::<AppState>();
        *st.abort_flag.lock().unwrap() = false;
        *st.pause_flag.lock().unwrap() = false;
        *st.is_downloading.lock().unwrap() = true;
    }

    // initialer Sweep + Heartbeat
    if is_host(&app) {
        let st = app.state::<AppState>();
        let mut team = st.team.lock().unwrap();
        coord_sweep(&mut team);
    }

    let mut last_heartbeat = 0u64;

    while team_active(&app) {
        let now = now_ms();
        let blocked = self_blocked(&app);

        if now.saturating_sub(last_heartbeat) >= HEARTBEAT_MS {
            worker_heartbeat(&app, &node_id, blocked).await;
            last_heartbeat = now;
        }

        // Host: periodisch verwaiste Aufgaben neu verteilen
        if is_host(&app) {
            let st = app.state::<AppState>();
            let mut team = st.team.lock().unwrap();
            coord_sweep(&mut team);
        }

        // gesperrter PC holt keine neuen Aufgaben
        if blocked {
            if worker_all_done(&app).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(CLAIM_IDLE_MS * 2)).await;
            continue;
        }

        let total = {
            let st = app.state::<AppState>();
            let team = st.team.lock().unwrap();
            team.tasks.len()
        };

        match worker_claim(&app, &node_id).await {
            Some(task) => {
                run_one_task(&app, &node_id, &task, total).await;
                emit_team(&app);
            }
            None => {
                if worker_all_done(&app).await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(CLAIM_IDLE_MS)).await;
            }
        }
    }

    // Aufräumen
    {
        let st = app.state::<AppState>();
        let mut team = st.team.lock().unwrap();
        team.worker_running = false;
        *st.is_downloading.lock().unwrap() = false;
    }
    let _ = app.emit("team-worker-stopped", serde_json::json!({ "node_id": node_id }));
    emit_team(&app);
}

// ─── Watchdog (Host) ─────────────────────────────────────────────────────────

pub fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(5_000));
        let is_host_active = {
            let st = app.state::<AppState>();
            let team = st.team.lock().unwrap();
            team.active && team.role == TeamRole::Host
        };
        if is_host_active {
            let st = app.state::<AppState>();
            let mut team = st.team.lock().unwrap();
            coord_sweep(&mut team);
            drop(team);
            emit_team(&app);
        }
    });
}

// ─── Tauri-Commands ──────────────────────────────────────────────────────────

/// Eigene PC-Adresse (IP:Port) für die Paarung.
fn self_addr() -> String {
    let ip = crate::server::ss_get_local_ip().unwrap_or_else(|_| "127.0.0.1".to_string());
    format!("{}:{}", ip, TEAM_PORT)
}

fn default_node_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "PC".to_string())
}

#[tauri::command]
pub fn team_create(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    name: Option<String>,
    format: Option<String>,
    quality: Option<String>,
    folder: Option<String>,
    share_files: Option<bool>,
    advertised_addr: Option<String>,
) -> Result<TeamInvite, String> {
    let node_id = short_id();
    let token = generate_remote_token();
    let team_id = short_id();
    // Adresse, unter der Member den Host erreichen. Default = eigene LAN-IP:3030.
    // Über einen eigenen Relay/Tunnel (z.B. Raspberry Pi + SSH/frp/Rathole)
    // kann hier die Pi-Adresse angegeben werden -> Link zeigt dann dorthin.
    let addr = advertised_addr
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(self_addr);
    let node_name = name.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_node_name);

    let download_folder = {
        let cfg = state.config.lock().unwrap();
        cfg.download_folder.clone()
    };

    {
        let mut team = state.team.lock().unwrap();
        *team = TeamState {
            active: true,
            role: TeamRole::Host,
            node_id: node_id.clone(),
            node_name: node_name.clone(),
            token: token.clone(),
            team_id: team_id.clone(),
            self_addr: addr.clone(),
            host_addr: addr.clone(),
            members: vec![TeamMember {
                node_id: node_id.clone(),
                name: node_name,
                addr: addr.clone(),
                last_seen: now_ms(),
                blocked: false,
                online: true,
            }],
            tasks: Vec::new(),
            format: format.unwrap_or_default(),
            quality: quality.unwrap_or_default(),
            folder: folder.filter(|s| !s.is_empty()).unwrap_or(download_folder),
            share_files: share_files.unwrap_or(true),
            blocked_until: 0,
            worker_running: false,
            last_error: String::new(),
        };
    }

    emit_team(&app);

    let link = format!(
        "{}://{}/{}?token={}",
        SCHEME, addr, team_id, token
    );

    Ok(TeamInvite {
        link,
        token,
        addr,
        team_id,
        node_id,
    })
}

/// Paarungs-Link zerlegen.
fn parse_invite_link(link: &str) -> Result<(String, String, String), String> {
    let link = link.trim();
    let rest = link
        .strip_prefix(&format!("{}://", SCHEME))
        .or_else(|| link.strip_prefix("http://"))
        .or_else(|| link.strip_prefix("https://"))
        .unwrap_or(link);

    // query trennen
    let (path_part, query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };

    // token aus query
    let mut token = String::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "token" {
                token = urlencoding::decode(v).map(|s| s.into_owned()).unwrap_or_else(|_| v.to_string());
            }
        }
    }

    // Pfad: [addr, teamid]  — bei http://…/team/join das /team/join wegschneiden
    let mut segs: Vec<&str> = path_part.split('/').filter(|s| !s.is_empty()).collect();
    // mögliche "/team/join" Endung entfernen
    if segs.len() >= 2 && segs[segs.len() - 2] == "team" {
        segs.truncate(segs.len() - 2);
    }
    if segs.is_empty() {
        return Err("Link enthält keine Adresse".to_string());
    }
    let addr = segs[0].to_string();
    let team_id = segs.get(1).map(|s| s.to_string()).unwrap_or_default();

    if token.is_empty() {
        return Err("Link enthält kein Token".to_string());
    }
    Ok((addr, team_id, token))
}

#[tauri::command]
pub async fn team_join(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    link: String,
    name: Option<String>,
    format: Option<String>,
    quality: Option<String>,
    folder: Option<String>,
    share_files: Option<bool>,
) -> Result<TeamState, String> {
    let (host_addr, team_id, token) = parse_invite_link(&link)?;
    let node_id = short_id();
    let node_name = name.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_node_name);
    let self_address = self_addr();

    let download_folder = {
        let cfg = state.config.lock().unwrap();
        cfg.download_folder.clone()
    };

    // beim Host anmelden
    let body = serde_json::json!({
        "token": token,
        "team_id": team_id,
        "node_id": node_id,
        "name": node_name,
        "addr": self_address,
    });
    let url = format!("{}/team/join", normalize_base(&host_addr));
    let resp = http_post_json(url, token.clone(), body).await
        .map_err(|e| format!("Beitritt fehlgeschlagen: {}", e))?;
    let host_state = serde_json::from_str::<TeamState>(&resp)
        .map_err(|e| format!("Antwort des Hosts unlesbar: {}", e))?;

    {
        let mut team = state.team.lock().unwrap();
        *team = TeamState {
            active: true,
            role: TeamRole::Member,
            node_id: node_id.clone(),
            node_name: node_name.clone(),
            token: token.clone(),
            team_id: host_state.team_id.clone(),
            self_addr: self_address,
            host_addr: host_addr.clone(),
            members: host_state.members.clone(),
            tasks: host_state.tasks.clone(),
            format: format.unwrap_or_default(),
            quality: quality.unwrap_or_default(),
            folder: folder.filter(|s| !s.is_empty()).unwrap_or(download_folder),
            share_files: share_files.unwrap_or(true),
            blocked_until: 0,
            worker_running: false,
            last_error: String::new(),
        };
    }

    emit_team(&app);
    let st = app.state::<AppState>();
    let snapshot = st.team.lock().unwrap().clone();
    Ok(snapshot)
}

#[tauri::command]
pub async fn team_leave(state: tauri::State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let (role, node_id, host_addr, token, self_id) = {
        let team = state.team.lock().unwrap();
        (
            team.role,
            team.node_id.clone(),
            team.host_addr.clone(),
            team.token.clone(),
            team.node_id.clone(),
        )
    };

    // laufenden Worker stoppen
    {
        let mut team = state.team.lock().unwrap();
        team.active = false;
        team.worker_running = false;
    }
    *state.abort_flag.lock().unwrap() = true;
    kill_active(&state);

    // Gegenstelle informieren
    match role {
        TeamRole::Member => {
            let url = format!("{}/team/leave", normalize_base(&host_addr));
            let body = serde_json::json!({ "node_id": node_id });
            let _ = http_post_json(url, token, body).await;
        }
        TeamRole::Host => {
            let mut team = state.team.lock().unwrap();
            coord_leave(&mut team, &self_id);
        }
        _ => {}
    }

    // State zurücksetzen
    {
        let mut team = state.team.lock().unwrap();
        *team = TeamState::default();
    }
    *state.abort_flag.lock().unwrap() = false;
    emit_team(&app);
    Ok(())
}

fn kill_active(state: &tauri::State<'_, AppState>) {
    let pids = {
        let mut active = state.active_download_pids.lock().unwrap();
        let pids = active.clone();
        active.clear();
        pids
    };
    for pid in pids {
        crate::utils::kill_process_tree(pid);
    }
}

/// Standard-Relay (selbstgehosteter `soundsync-relay`).
pub const DEFAULT_RELAY: &str = "https://65.21.135.40";

#[tauri::command]
pub fn team_default_relay() -> String {
    DEFAULT_RELAY.to_string()
}

/// Direkt mit einem selbstgehosteten Relay verbinden (server-gehostete API).
/// `relay_url` default = DEFAULT_RELAY. `token` = SOUNDSYNC_RELAY_TOKEN des Relays.
/// Beide PCs verbinden sich nur ausgehend; kein Port-Forwarding auf den PCs.
#[tauri::command]
pub async fn team_connect_relay(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    relay_url: Option<String>,
    token: String,
    name: Option<String>,
    format: Option<String>,
    quality: Option<String>,
    folder: Option<String>,
    share_files: Option<bool>,
) -> Result<TeamState, String> {
    let relay = relay_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RELAY.to_string())
        .trim()
        .trim_end_matches('/')
        .to_string();
    let base = normalize_base(&relay);
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Relay-Token fehlt".to_string());
    }

    // 1) Relay erreichen + team_id + Token prüfen
    let state_url = format!("{}/team/state", base);
    let resp = http_get_json(state_url, token.clone())
        .await
        .map_err(|e| format!("Relay nicht erreichbar: {}", e))?;
    let relay_state = serde_json::from_str::<TeamState>(&resp)
        .map_err(|e| format!("Relay-Antwort unlesbar: {}", e))?;
    if !relay_state.active {
        return Err("Relay-Team ist nicht aktiv".to_string());
    }
    let team_id = relay_state.team_id.clone();

    // 2) Anmelden
    let node_id = short_id();
    let node_name = name.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_node_name);
    let self_address = self_addr();
    let body = serde_json::json!({
        "token": token, "team_id": team_id, "node_id": node_id, "name": node_name, "addr": self_address,
    });
    let join_url = format!("{}/team/join", base);
    let resp = http_post_json(join_url, token.clone(), body).await
        .map_err(|e| format!("Beitritt zum Relay fehlgeschlagen: {}", e))?;
    let host_state = serde_json::from_str::<TeamState>(&resp)
        .map_err(|e| format!("Relay-Antwort unlesbar: {}", e))?;

    let download_folder = { state.config.lock().unwrap().download_folder.clone() };

    {
        let mut team = state.team.lock().unwrap();
        *team = TeamState {
            active: true,
            role: TeamRole::Member,
            node_id: node_id.clone(),
            node_name,
            token,
            team_id: host_state.team_id.clone(),
            self_addr: self_address,
            host_addr: base.clone(),
            members: host_state.members.clone(),
            tasks: host_state.tasks.clone(),
            format: format.unwrap_or_default(),
            quality: quality.unwrap_or_default(),
            folder: folder.filter(|s| !s.is_empty()).unwrap_or(download_folder),
            share_files: share_files.unwrap_or(true),
            blocked_until: 0,
            worker_running: false,
            last_error: String::new(),
        };
    }

    emit_team(&app);
    Ok(app.state::<AppState>().team.lock().unwrap().clone())
}

/// Eigene Playlist beisteuern. `entries` = bereits aufgelöste Tracks
/// (title, url). Wenn `url` gesetzt ist, wird sie via yt-dlp aufgelöst.
#[tauri::command]
pub async fn team_add_playlist(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    url: Option<String>,
    cookies_path: Option<String>,
    source: Option<String>,
    entries: Option<Vec<(String, String)>>,
) -> Result<usize, String> {
    let (role, node_id, host_addr, token) = {
        let team = state.team.lock().unwrap();
        if !team.active {
            return Err("Kein aktives Team".to_string());
        }
        (team.role, team.node_id.clone(), team.host_addr.clone(), team.token.clone())
    };

    // Einträge bestimmen
    let mut resolved: Vec<(String, String)> = entries.unwrap_or_default();
    let mut source_name = source.clone().unwrap_or_default();

    if let Some(u) = url.clone() {
        let u = u.trim().to_string();
        if !u.is_empty() {
            let info = crate::commands::get_playlist_info(u.clone(), cookies_path.clone()).await?;
            if source_name.is_empty() {
                source_name = if info.title.is_empty() { u.clone() } else { info.title.clone() };
            }
            for e in info.entries {
                resolved.push((e.title, e.url));
            }
        }
    }
    if source_name.is_empty() {
        source_name = node_id.clone();
    }

    if resolved.is_empty() {
        return Err("Keine Tracks in der Playlist gefunden".to_string());
    }

    let added = match role {
        TeamRole::Host => {
            let mut team = state.team.lock().unwrap();
            coord_add_playlist(&mut team, &node_id, &source_name, resolved)
        }
        TeamRole::Member => {
            let body = serde_json::json!({
                "node_id": node_id,
                "source": source_name,
                "entries": resolved,
            });
            let url = format!("{}/team/playlist", normalize_base(&host_addr));
            let resp = http_post_json(url, token, body).await?;
            resp.trim().parse::<usize>().unwrap_or(0)
        }
        TeamRole::None => return Err("Kein aktives Team".to_string()),
    };

    emit_team(&app);
    Ok(added)
}

#[tauri::command]
pub fn team_start(state: tauri::State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    {
        let mut team = state.team.lock().unwrap();
        if !team.active {
            return Err("Kein aktives Team".to_string());
        }
        if team.worker_running {
            return Ok(());
        }
        team.worker_running = true;
    }
    // nicht-abbrechende Downloads
    *state.abort_flag.lock().unwrap() = false;
    *state.pause_flag.lock().unwrap() = false;

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        worker_loop(app_clone).await;
    });

    // unabhängiger Heartbeat (läuft auch während ein Download läuft)
    let app_hb = app.clone();
    tauri::async_runtime::spawn(async move {
        heartbeat_loop(app_hb).await;
    });

    emit_team(&app);
    Ok(())
}

async fn heartbeat_loop(app: AppHandle) {
    let node_id = self_node_id(&app);
    while team_active(&app) {
        let blocked = self_blocked(&app);
        worker_heartbeat(&app, &node_id, blocked).await;
        tokio::time::sleep(std::time::Duration::from_millis(HEARTBEAT_MS)).await;
    }
}

#[tauri::command]
pub fn team_stop(state: tauri::State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    // Team bleibt aktiv, nur der Worker hält an
    {
        let mut team = state.team.lock().unwrap();
        team.worker_running = false;
    }
    // Laufenden Download abbrechen
    *state.abort_flag.lock().unwrap() = true;
    kill_active(&state);
    *state.is_downloading.lock().unwrap() = false;
    emit_team(&app);
    Ok(())
}

#[tauri::command]
pub async fn team_status(app: AppHandle) -> Result<TeamState, String> {
    // lokalen Snapshot holen
    let (role, base, token) = {
        let st = app.state::<AppState>();
        let team = st.team.lock().unwrap();
        (
            team.role,
            if team.role == TeamRole::Member { Some(normalize_base(&team.host_addr)) } else { None },
            team.token.clone(),
        )
    };

    // Member: live vom Host aktualisieren
    if let (TeamRole::Member, Some(base)) = (role, base) {
        let url = format!("{}/team/state", base);
        if let Ok(resp) = http_get_json(url, token).await {
            if let Ok(remote) = serde_json::from_str::<TeamState>(&resp) {
                let st = app.state::<AppState>();
                let mut team = st.team.lock().unwrap();
                // kanonische Daten vom Host übernehmen, lokale Einstellungen behalten
                team.tasks = remote.tasks;
                team.members = remote.members;
                team.blocked_until = remote.blocked_until;
                team.team_id = remote.team_id;
                return Ok(team.clone());
            }
        }
    }

    let st = app.state::<AppState>();
    let team = st.team.lock().unwrap();
    Ok(team.clone())
}

/// Fehlende Dateien vom Koordinator holen (gemeinsame Ausgabe vervollständigen).
#[tauri::command]
pub async fn team_pull_missing(app: AppHandle) -> Result<usize, String> {
    let (role, base, token, folder, tasks) = {
        let st = app.state::<AppState>();
        let team = st.team.lock().unwrap();
        let cfg = st.config.lock().unwrap();
        (
            team.role,
            if team.role == TeamRole::Host { None } else { Some(normalize_base(&team.host_addr)) },
            team.token.clone(),
            if !team.folder.is_empty() { team.folder.clone() } else { cfg.download_folder.clone() },
            team.tasks.clone(),
        )
    };

    let (Some(base), TeamRole::Member) = (base, role) else {
        return Ok(0); // Host hat alles lokal / nichts zu holen
    };

    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut pulled = 0usize;

    for t in tasks.iter().filter(|t| t.status == "done" && !t.file_name.is_empty()) {
        let dest = std::path::Path::new(&folder).join(&t.file_name);
        if dest.exists() {
            continue;
        }
        let url = format!(
            "{}/team/file/{}?token={}",
            base,
            urlencoding::encode(&t.file_name),
            urlencoding::encode(&token),
        );
        let token_c = token.clone();
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let resp = ureq::get(&url).set("X-Team-Token", &token_c)
                .timeout(std::time::Duration::from_secs(120)).call();
            match resp {
                Ok(r) => {
                    let mut buf = Vec::new();
                    use std::io::Read;
                    r.into_reader().read_to_end(&mut buf).map_err(|e| e.to_string())?;
                    Ok(buf)
                }
                Err(e) => Err(e.to_string()),
            }
        }).await.map_err(|e| e.to_string())?;

        if let Ok(bytes) = bytes {
            if std::fs::write(&dest, bytes).is_ok() {
                pulled += 1;
            }
        }
    }
    Ok(pulled)
}


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn host_team() -> TeamState {
        TeamState {
            active: true,
            role: TeamRole::Host,
            node_id: "host".to_string(),
            token: "tok".to_string(),
            team_id: "team".to_string(),
            members: vec![TeamMember {
                node_id: "host".to_string(),
                name: "Host".to_string(),
                addr: "127.0.0.1:3030".to_string(),
                last_seen: now_ms(),
                blocked: false,
                online: true,
            }],
            ..Default::default()
        }
    }

    fn add_member(team: &mut TeamState, id: &str) {
        team.members.push(TeamMember {
            node_id: id.to_string(),
            name: id.to_string(),
            addr: "127.0.0.1:3030".to_string(),
            last_seen: now_ms(),
            blocked: false,
            online: true,
        });
    }

    #[test]
    fn add_playlist_dedupes_urls() {
        let mut t = host_team();
        let entries = vec![
            ("A".to_string(), "http://x/1".to_string()),
            ("B".to_string(), "http://x/2".to_string()),
            ("A dup".to_string(), "http://x/1".to_string()),
            ("".to_string(), "  ".to_string()),
        ];
        let added = coord_add_playlist(&mut t, "host", "src", entries);
        assert_eq!(added, 2);
        assert_eq!(t.tasks.len(), 2);
        let again = coord_add_playlist(&mut t, "host", "src", vec![("A".into(), "http://x/1".into())]);
        assert_eq!(again, 0);
    }

    #[test]
    fn claim_assigns_oldest_queued() {
        let mut t = host_team();
        add_member(&mut t, "member");
        coord_add_playlist(&mut t, "host", "s", vec![
            ("1".into(), "http://x/1".into()),
            ("2".into(), "http://x/2".into()),
        ]);
        let first = coord_claim(&mut t, "member").expect("task");
        assert_eq!(first.url, "http://x/1");
        assert_eq!(t.tasks[0].assigned_to, "member");
        assert_eq!(t.tasks[0].status, "assigned");
        assert_eq!(t.tasks[0].attempts, 1);
        let second = coord_claim(&mut t, "host").expect("task2");
        assert_eq!(second.url, "http://x/2");
    }

    #[test]
    fn blocked_node_failover_requeues_for_other() {
        let mut t = host_team();
        add_member(&mut t, "member");
        coord_add_playlist(&mut t, "host", "s", vec![("1".into(), "http://x/1".into())]);

        let task = coord_claim(&mut t, "member").expect("task");

        coord_fail(&mut t, "member", &task.id, true, "HTTP Error 429: Too Many Requests");

        assert!(t.members.iter().find(|m| m.node_id == "member").unwrap().blocked);
        assert!(coord_claim(&mut t, "member").is_none());
        assert_eq!(t.tasks[0].status, "queued");

        let other = coord_claim(&mut t, "host").expect("host takes over");
        assert_eq!(other.url, "http://x/1");
        assert_eq!(t.tasks[0].assigned_to, "host");
    }

    #[test]
    fn offline_node_tasks_requeued_by_sweep() {
        let mut t = host_team();
        add_member(&mut t, "member");
        coord_add_playlist(&mut t, "host", "s", vec![("1".into(), "http://x/1".into())]);
        let task = coord_claim(&mut t, "member").expect("task");

        let idx = t.members.iter().position(|m| m.node_id == "member").unwrap();
        t.members[idx].last_seen = now_ms().saturating_sub(MEMBER_OFFLINE_MS + 1000);
        t.tasks[0].last_update = now_ms().saturating_sub(TASK_STALE_MS + 1000);

        coord_sweep(&mut t);
        assert!(!t.members[idx].online);
        assert_eq!(t.tasks[0].status, "queued");
        assert!(t.tasks[0].assigned_to.is_empty());

        let other = coord_claim(&mut t, "host").expect("host takes over");
        assert_eq!(other.id, task.id);
    }

    #[test]
    fn attempts_cap_marks_failed() {
        let mut t = host_team();
        coord_add_playlist(&mut t, "host", "s", vec![("1".into(), "http://x/1".into())]);
        let id = t.tasks[0].id.clone();
        for _ in 0..MAX_ATTEMPTS {
            let _ = coord_claim(&mut t, "host");
            coord_fail(&mut t, "host", &id, false, "kaputt");
        }
        assert_eq!(t.tasks[0].status, "failed");
    }

    #[test]
    fn all_done_requires_nonempty_and_terminal() {
        let mut t = host_team();
        assert!(!team_all_done(&t));
        coord_add_playlist(&mut t, "host", "s", vec![("1".into(), "http://x/1".into())]);
        assert!(!team_all_done(&t));
        let id = t.tasks[0].id.clone();
        coord_claim(&mut t, "host");
        coord_done(&mut t, "host", &id, "file.mp3");
        assert!(team_all_done(&t));
    }

    #[test]
    fn detects_youtube_blocks() {
        assert!(is_blocked_error("yt-dlp exited with code: 1\nERROR: HTTP Error 429: Too Many Requests"));
        assert!(is_blocked_error("ERROR: Sign in to confirm you're not a bot"));
        assert!(is_blocked_error("HTTP Error 403: Forbidden"));
        assert!(is_blocked_error("rate-limit exceeded"));
        assert!(!is_blocked_error("ERROR: Video unavailable"));
        assert!(!is_blocked_error("some random error"));
    }

    #[test]
    fn blocked_node_does_not_get_work() {
        let mut t = host_team();
        add_member(&mut t, "member");
        coord_add_playlist(&mut t, "host", "s", vec![("1".into(), "http://x/1".into())]);
        coord_heartbeat(&mut t, "member", true);
        assert!(coord_claim(&mut t, "member").is_none());
    }

    #[test]
    fn invite_link_roundtrip() {
        let link = "soundsync-team://192.168.1.5:3030/abc123?token=xyz789";
        let (addr, team_id, token) = parse_invite_link(link).unwrap();
        assert_eq!(addr, "192.168.1.5:3030");
        assert_eq!(team_id, "abc123");
        assert_eq!(token, "xyz789");
    }
}

