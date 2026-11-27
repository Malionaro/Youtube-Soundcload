//! Reine Koordinator-Logik + Typen (ohne Tauri / ohne Download). Wiederverwendbar.
use serde::{Deserialize, Serialize};

pub const HEARTBEAT_MS: u64 = 5_000;
pub const MEMBER_OFFLINE_MS: u64 = 30_000;
pub const TASK_STALE_MS: u64 = 300_000;
pub const BLOCKED_COOLDOWN_MS: u64 = 120_000;
pub const MAX_ATTEMPTS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole { #[default] None, Host, Member }

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
    pub origin: String,
    pub source: String,
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
    pub blocked_until: u64,
    pub worker_running: bool,
    pub last_error: String,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn short_id() -> String { generate_remote_token().chars().take(16).collect() }

pub fn generate_remote_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id() as u128;
    let c = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    // OS-Entropie nachmischen (std-only, keine Crate)
    let rs = std::collections::hash_map::RandomState::new();
    use std::hash::{BuildHasher, Hasher};
    let mut h = rs.build_hasher(); h.write_u128(now); h.write_u64(pid as u64); h.write_u64(c as u64);
    let rand = h.finish() as u128;
    let mixed = now ^ rand ^ ((pid << 32) | (c & 0xffff_ffff));
    format!("{:032x}", mixed.wrapping_mul(0x9e3779b97f4a7c15))
}

pub fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 { return false; }
    if name.contains("..") || name.contains('/') || name.contains('\\') { return false; }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') { return false; }
    if name.contains(':') { return false; }
    if name.chars().any(|c| (c as u32) < 0x20) { return false; }
    // Windows-reservierte Namen
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"];
    if RESERVED.contains(&stem.as_str()) { return false; }
    true
}

pub fn coord_join(team: &mut TeamState, token: &str, team_id: &str, node_id: &str, name: &str, addr: &str) -> Result<TeamState, String> {
    if team.role != TeamRole::Host || !team.active { return Err("Team ist nicht aktiv".to_string()); }
    if token != team.token { return Err("Falsches Token".to_string()); }
    if !team_id.is_empty() && team_id != team.team_id { return Err("Falsche Team-ID".to_string()); }
    if node_id.is_empty() || node_id == team.node_id { return Err("Ungültige Node-ID".to_string()); }
    let now = now_ms();
    if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) {
        m.name = name.to_string(); m.addr = addr.to_string(); m.last_seen = now; m.online = true;
    } else {
        team.members.push(TeamMember { node_id: node_id.to_string(), name: name.to_string(), addr: addr.to_string(), last_seen: now, blocked: false, online: true });
    }
    Ok(team.clone())
}

pub fn coord_leave(team: &mut TeamState, node_id: &str) { requeue_node(team, node_id); team.members.retain(|m| m.node_id != node_id); }

pub fn coord_heartbeat(team: &mut TeamState, node_id: &str, blocked: bool) {
    let now = now_ms();
    if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) { m.last_seen = now; m.online = true; m.blocked = blocked; }
    for t in team.tasks.iter_mut() {
        if t.assigned_to == node_id && (t.status == "assigned" || t.status == "downloading") { t.last_update = now; }
    }
}

pub fn coord_add_playlist(team: &mut TeamState, origin: &str, source: &str, entries: Vec<(String, String)>) -> usize {
    let now = now_ms(); let mut added = 0usize;
    for (title, url) in entries {
        let url = url.trim().to_string(); if url.is_empty() { continue; }
        if team.tasks.iter().any(|t| t.url == url) { continue; }
        let title = if title.trim().is_empty() { url.clone() } else { title.trim().to_string() };
        team.tasks.push(TeamTask { id: short_id(), url, title, origin: origin.to_string(), source: source.to_string(),
            status: "queued".to_string(), assigned_to: String::new(), attempts: 0, last_update: now, percent: 0.0, error: String::new(), file_name: String::new() });
        added += 1;
    }
    added
}

fn node_can_take_work(team: &TeamState, node_id: &str) -> bool {
    if node_id == team.node_id { return now_ms() >= team.blocked_until; }
    team.members.iter().find(|m| m.node_id == node_id).map(|m| m.online && !m.blocked).unwrap_or(false)
}

pub fn coord_claim(team: &mut TeamState, node_id: &str) -> Option<TeamTask> {
    coord_sweep(team);
    if !node_can_take_work(team, node_id) { return None; }
    let now = now_ms();
    let idx = team.tasks.iter().position(|t| t.status == "queued")?;
    let task = &mut team.tasks[idx];
    task.status = "assigned".to_string(); task.assigned_to = node_id.to_string(); task.attempts += 1; task.last_update = now; task.percent = 0.0; task.error.clear();
    Some(task.clone())
}

pub fn coord_progress(team: &mut TeamState, node_id: &str, task_id: &str, percent: f64) {
    let now = now_ms();
    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        if t.assigned_to == node_id { t.status = "downloading".to_string(); t.percent = percent.clamp(0.0, 100.0); t.last_update = now; }
    }
}

pub fn coord_done(team: &mut TeamState, node_id: &str, task_id: &str, file_name: &str) {
    let now = now_ms();
    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        if t.assigned_to == node_id || t.assigned_to.is_empty() { t.status = "done".to_string(); t.percent = 100.0; t.file_name = file_name.to_string(); t.error.clear(); t.last_update = now; }
    }
}

pub fn coord_fail(team: &mut TeamState, node_id: &str, task_id: &str, blocked: bool, error: &str) {
    let now = now_ms();
    if blocked {
        if node_id == team.node_id { team.blocked_until = now + BLOCKED_COOLDOWN_MS; }
        if let Some(m) = team.members.iter_mut().find(|m| m.node_id == node_id) { m.blocked = true; }
        requeue_node(team, node_id);
    }
    if let Some(t) = team.tasks.iter_mut().find(|t| t.id == task_id) {
        if t.status == "done" { return; }
        t.error = error.chars().take(400).collect(); t.assigned_to.clear(); t.last_update = now;
        if t.attempts >= MAX_ATTEMPTS { t.status = "failed".to_string(); } else { t.status = "queued".to_string(); }
    }
}

fn requeue_node(team: &mut TeamState, node_id: &str) {
    let now = now_ms();
    for t in team.tasks.iter_mut() {
        if t.assigned_to == node_id && (t.status == "assigned" || t.status == "downloading") {
            t.assigned_to.clear(); t.status = "queued".to_string(); t.last_update = now; t.attempts = t.attempts.saturating_sub(1);
        }
    }
}

pub fn coord_sweep(team: &mut TeamState) {
    let now = now_ms();
    for m in team.members.iter_mut() { if m.online && now.saturating_sub(m.last_seen) > MEMBER_OFFLINE_MS { m.online = false; } }
    if team.blocked_until != 0 && now >= team.blocked_until { team.blocked_until = 0; }
    let offline: Vec<String> = team.members.iter().filter(|m| !m.online || m.blocked).map(|m| m.node_id.clone()).collect();
    let self_blocked = now < team.blocked_until;
    for node in offline { requeue_node(team, &node); }
    if self_blocked { let self_id = team.node_id.clone(); requeue_node(team, &self_id); }
    for t in team.tasks.iter_mut() {
        if (t.status == "assigned" || t.status == "downloading") && now.saturating_sub(t.last_update) > TASK_STALE_MS {
            t.assigned_to.clear(); t.status = "queued".to_string(); t.last_update = now; t.attempts = t.attempts.saturating_sub(1);
        }
    }
}

pub fn team_all_done(team: &TeamState) -> bool {
    !team.tasks.is_empty() && team.tasks.iter().all(|t| t.status == "done" || t.status == "failed")
}
