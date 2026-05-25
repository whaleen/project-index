use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{dirs_home, sanitize_project_component};

// ── Agent types ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MemoryAgent {
    Claude,
    Codex,
    Gemini,
    Pi,
}

impl MemoryAgent {
    pub(crate) fn label(self) -> &'static str {
        match self {
            MemoryAgent::Claude => "claude",
            MemoryAgent::Codex => "codex",
            MemoryAgent::Gemini => "gemini",
            MemoryAgent::Pi => "pi",
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub(crate) enum AgentSection {
    Mcp,
    Skills,
    Sessions,
}

impl AgentSection {
    pub(crate) fn next(self) -> Self {
        match self {
            AgentSection::Mcp => AgentSection::Skills,
            AgentSection::Skills => AgentSection::Sessions,
            AgentSection::Sessions => AgentSection::Mcp,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            AgentSection::Mcp => AgentSection::Sessions,
            AgentSection::Skills => AgentSection::Mcp,
            AgentSection::Sessions => AgentSection::Skills,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub(crate) enum AgentKind {
    Claude,
    Codex,
    Gemini,
    Pi,
}

impl AgentKind {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Gemini => "gemini",
            AgentKind::Pi => "pi",
        }
    }

    pub(crate) fn launch_cmd(&self, project_path: &Path, _prompt: Option<&str>) -> String {
        let mcp = project_path.join(".mcp.json");
        let has_mcp = mcp.exists();
        match self {
            AgentKind::Claude => {
                let mut cmd = String::from("claude");
                if has_mcp {
                    cmd.push_str(" --mcp-config .mcp.json");
                }
                cmd
            }
            AgentKind::Codex => String::from("codex"),
            AgentKind::Gemini => String::from("gemini"),
            AgentKind::Pi => String::from("pi"),
        }
    }

    pub(crate) fn resume_cmd(&self, session_id: &str) -> String {
        match self {
            AgentKind::Claude => format!("claude --resume {}", session_id),
            AgentKind::Codex => format!("codex --session {}", session_id),
            AgentKind::Gemini => format!("gemini  # session: {}", session_id),
            AgentKind::Pi => format!("pi --session {}", session_id),
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "gemini" => Some(AgentKind::Gemini),
            "pi" => Some(AgentKind::Pi),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct AgentSession {
    pub(crate) id: Option<String>,
    pub(crate) agent: AgentKind,
    pub(crate) started_at: String,
    pub(crate) prompt: Option<String>,
    pub(crate) first_message: Option<String>,
}

#[derive(PartialEq, Clone)]
pub(crate) enum SessionsState {
    List,
    NewPicker { agent_idx: usize, prompt_idx: Option<usize> },
    Summary { lines: Vec<String>, scroll: usize },
}

#[derive(PartialEq, Clone)]
pub(crate) enum MemoriesView {
    Claude,
    Codex,
    Gemini,
}

// ── Memory helpers ────────────────────────────────────────────────────────────

pub(crate) struct MemoryFile {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) content: String,
    pub(crate) origin: Option<MemoryAgent>,
}

pub(crate) fn claude_memory_path(project_path: &Path) -> Option<PathBuf> {
    claude_project_dirs(project_path)
        .into_iter()
        .map(|d| d.join("memory"))
        .find(|d| d.is_dir())
}

pub(crate) fn load_memory_files(dir: &Path) -> Vec<MemoryFile> {
    if !dir.is_dir() {
        return vec![];
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());
    entries
        .iter()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let content = fs::read_to_string(&path).unwrap_or_default();
            Some(MemoryFile { origin: None, name, path, content })
        })
        .collect()
}

pub(crate) fn codex_memory_dirs(project_path: &Path) -> Vec<PathBuf> {
    let Some(home) = dirs_home() else { return vec![] };
    let base = home.join(".codex").join("memories");
    let repo_name = project_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let sanitized = sanitize_project_component(project_path);
    let candidates = vec![base.join(&repo_name), base.join(&sanitized)];
    candidates.into_iter().filter(|path| path.is_dir()).collect()
}

pub(crate) fn gemini_memory_path() -> PathBuf {
    dirs_home().unwrap_or_default().join(".gemini").join("GEMINI.md")
}

// ── Sessions persistence ──────────────────────────────────────────────────────

pub(crate) fn load_sessions(_project_path: &Path) -> Vec<AgentSession> {
    vec![]
}

pub(crate) fn claude_project_dirs(project_path: &Path) -> Vec<PathBuf> {
    let home = match dirs_home() {
        Some(h) => h,
        None => return vec![],
    };
    let base = home.join(".claude").join("projects");
    let path_str = project_path.to_string_lossy();
    let encoded_v1 = path_str.replace('/', "-");
    let encoded_v2 = path_str.replace('/', "-").replace('_', "-");
    let mut dirs = vec![];
    for encoded in [encoded_v1, encoded_v2] {
        let dir = base.join(&encoded);
        if dir.exists() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

fn jsonl_files_in(dir: &Path) -> Vec<(String, std::time::SystemTime)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "jsonl")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            let stem = e.path().file_stem()?.to_str()?.to_string();
            Some((stem, mtime))
        })
        .collect()
}

pub(crate) fn resolve_sessions(sessions: &mut Vec<AgentSession>, project_path: &Path) {
    let claude_dirs = claude_project_dirs(project_path);

    if !claude_dirs.is_empty() {
        let all_jsonl: Vec<(PathBuf, String, std::time::SystemTime)> = claude_dirs
            .iter()
            .flat_map(|dir| {
                jsonl_files_in(dir)
                    .into_iter()
                    .map(|(stem, mtime)| (dir.join(format!("{}.jsonl", stem)), stem, mtime))
                    .collect::<Vec<_>>()
            })
            .collect();

        let unresolved: Vec<usize> = sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.id.is_none() && s.agent == AgentKind::Claude)
            .map(|(i, _)| i)
            .collect();

        for idx in unresolved {
            let Ok(session_time) = chrono_parse(&sessions[idx].started_at) else { continue };
            if let Some((jsonl_path, uuid, _)) = all_jsonl.iter().find(|(_, _, mtime)| {
                let secs = mtime.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                (secs as i64 - session_time as i64).abs() < 120
            }) {
                let first_msg = read_first_user_message(jsonl_path);
                sessions[idx].id = Some(uuid.clone());
                if sessions[idx].first_message.is_none() {
                    sessions[idx].first_message = first_msg;
                }
            }
        }

        let known_ids: std::collections::HashSet<String> = sessions
            .iter()
            .filter_map(|s| s.id.clone())
            .collect();

        for (jsonl_path, uuid, mtime) in &all_jsonl {
            if known_ids.contains(uuid) {
                continue;
            }
            let secs = mtime.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let started_at = unix_to_iso(secs);
            let first_message = read_first_user_message(jsonl_path);
            sessions.push(AgentSession {
                id: Some(uuid.clone()),
                agent: AgentKind::Claude,
                started_at,
                prompt: None,
                first_message,
            });
        }
    }

    import_codex_sessions(sessions, project_path);
    import_pi_sessions(sessions, project_path);
    import_gemini_sessions(sessions, project_path);

    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
}

fn import_codex_sessions(sessions: &mut Vec<AgentSession>, project_path: &Path) {
    let sessions_dir = match dirs_home() {
        Some(h) => h.join(".codex").join("sessions"),
        None => return,
    };
    if !sessions_dir.exists() {
        return;
    }

    let known_ids: std::collections::HashSet<String> = sessions
        .iter()
        .filter_map(|s| s.id.clone())
        .collect();

    let project_str = project_path.to_string_lossy().to_string();

    let year_dirs: Vec<PathBuf> = fs::read_dir(&sessions_dir)
        .into_iter().flatten().filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path()).collect();

    for year_dir in year_dirs {
        let month_dirs: Vec<PathBuf> = fs::read_dir(&year_dir)
            .into_iter().flatten().filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path()).collect();
        for month_dir in month_dirs {
            let day_dirs: Vec<PathBuf> = fs::read_dir(&month_dir)
                .into_iter().flatten().filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path()).collect();
            for day_dir in day_dirs {
                let jsonl_files: Vec<PathBuf> = fs::read_dir(&day_dir)
                    .into_iter().flatten().filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
                    .map(|e| e.path()).collect();
                for jsonl in jsonl_files {
                    if let Some(session) = parse_codex_session(&jsonl, &project_str, &known_ids) {
                        sessions.push(session);
                    }
                }
            }
        }
    }
}

fn parse_codex_session(
    path: &Path,
    project_path: &str,
    known_ids: &std::collections::HashSet<String>,
) -> Option<AgentSession> {
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines();

    let meta: serde_json::Value = serde_json::from_str(lines.next()?).ok()?;
    if meta.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return None;
    }
    let payload = meta.get("payload")?;
    let cwd = payload.get("cwd").and_then(|v| v.as_str())?;
    if cwd != project_path {
        return None;
    }
    let id = payload.get("id").and_then(|v| v.as_str())?.to_string();
    if known_ids.contains(&id) {
        return None;
    }
    let started_at = payload
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let first_message = content.lines().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("type").and_then(|t| t.as_str()) != Some("response_item") {
            return None;
        }
        let p = v.get("payload")?;
        if p.get("type").and_then(|t| t.as_str()) != Some("message") {
            return None;
        }
        if p.get("role").and_then(|r| r.as_str()) != Some("user") {
            return None;
        }
        for block in p.get("content")?.as_array()? {
            let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if !text.is_empty() && !text.starts_with("<environment_context>") {
                return Some(text.chars().take(80).collect());
            }
        }
        None
    });

    Some(AgentSession {
        id: Some(id),
        agent: AgentKind::Codex,
        started_at,
        prompt: None,
        first_message,
    })
}

fn pi_encode_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let stripped = s.trim_start_matches('/');
    let encoded = stripped.replace('/', "-");
    format!("--{}--", encoded)
}

fn import_pi_sessions(sessions: &mut Vec<AgentSession>, project_path: &Path) {
    let sessions_dir = match dirs_home() {
        Some(h) => h.join(".pi").join("agent").join("sessions"),
        None => return,
    };
    if !sessions_dir.exists() { return; }

    let project_dir = sessions_dir.join(pi_encode_path(project_path));
    if !project_dir.exists() { return; }

    let known_ids: std::collections::HashSet<String> = sessions
        .iter().filter_map(|s| s.id.clone()).collect();

    let project_str = project_path.to_string_lossy();

    let jsonl_files: Vec<PathBuf> = fs::read_dir(&project_dir)
        .into_iter().flatten().filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| e.path())
        .collect();

    for path in jsonl_files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let id = match stem.find('_') {
            Some(i) => stem[i + 1..].to_string(),
            None => continue,
        };
        if known_ids.contains(&id) { continue; }

        let content = match fs::read_to_string(&path) { Ok(c) => c, Err(_) => continue };
        let first_line = content.lines().next().unwrap_or("");
        let meta: serde_json::Value = match serde_json::from_str(first_line) { Ok(v) => v, Err(_) => continue };

        if meta.get("type").and_then(|t| t.as_str()) != Some("session") { continue; }

        let cwd = meta.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
        if cwd != project_str.as_ref() { continue; }

        let started_at = meta.get("timestamp").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let first_message = content.lines().find_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            if v.get("type").and_then(|t| t.as_str()) != Some("message") { return None; }
            let msg = v.get("message")?;
            if msg.get("role").and_then(|r| r.as_str()) != Some("user") { return None; }
            for block in msg.get("content")?.as_array()? {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if !text.is_empty() {
                        return Some(text.chars().take(80).collect());
                    }
                }
            }
            None
        });

        sessions.push(AgentSession {
            id: Some(id),
            agent: AgentKind::Pi,
            started_at,
            prompt: None,
            first_message,
        });
    }
}

fn import_gemini_sessions(sessions: &mut Vec<AgentSession>, project_path: &Path) {
    let home = match dirs_home() { Some(h) => h, None => return };
    let gemini_dir = home.join(".gemini");
    let project_str = project_path.to_string_lossy();

    let project_name: Option<String> = fs::read_to_string(gemini_dir.join("projects.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("projects")?
                .get(project_str.as_ref())?
                .as_str()
                .map(|s| s.to_string())
        });

    let known_ids: std::collections::HashSet<String> = sessions
        .iter().filter_map(|s| s.id.clone()).collect();

    let mut chats_dirs: Vec<PathBuf> = Vec::new();
    if let Some(ref name) = project_name {
        chats_dirs.push(gemini_dir.join("tmp").join(name).join("chats"));
    }

    for chats_dir in chats_dirs {
        if !chats_dir.exists() { continue; }

        let json_files: Vec<PathBuf> = fs::read_dir(&chats_dir)
            .into_iter().flatten().filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .map(|e| e.path())
            .collect();

        for path in json_files {
            let content = match fs::read_to_string(&path) { Ok(c) => c, Err(_) => continue };
            let v: serde_json::Value = match serde_json::from_str(&content) { Ok(v) => v, Err(_) => continue };

            let id = match v.get("sessionId").and_then(|s| s.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            if known_ids.contains(&id) { continue; }

            let started_at = v.get("startTime").and_then(|s| s.as_str()).unwrap_or("").to_string();

            let first_message = v.get("messages")
                .and_then(|m| m.as_array())
                .and_then(|msgs| msgs.iter().find_map(|msg| {
                    if msg.get("type").and_then(|t| t.as_str()) != Some("user") { return None; }
                    let content = msg.get("content")?;
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            if !text.is_empty() {
                                return Some(text.chars().take(80).collect::<String>());
                            }
                        }
                        None
                    } else {
                        content.as_str().filter(|s| !s.is_empty()).map(|s| s.chars().take(80).collect())
                    }
                }));

            sessions.push(AgentSession {
                id: Some(id),
                agent: AgentKind::Gemini,
                started_at,
                prompt: None,
                first_message,
            });
        }
    }
}

fn chrono_parse(s: &str) -> Result<u64, ()> {
    let s = s.trim_end_matches('Z');
    let parts: Vec<&str> = s.splitn(2, 'T').collect();
    if parts.len() != 2 {
        return Err(());
    }
    let date: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time: Vec<u64> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
    if date.len() < 3 || time.len() < 2 {
        return Err(());
    }
    let days_since_epoch = (date[0] - 1970) * 365 + (date[1] - 1) * 30 + date[2];
    let secs = days_since_epoch * 86400 + time[0] * 3600 + time[1] * 60 + time.get(2).copied().unwrap_or(0);
    Ok(secs)
}

fn read_first_user_message(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("type").and_then(|t| t.as_str()) == Some("user") {
            if let Some(msg) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                let trimmed = msg.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed.chars().take(80).collect());
                }
            }
        }
    }
    None
}

struct SessionTurn {
    role: String,
    text: String,
}

fn parse_session_turns(path: &Path) -> Vec<SessionTurn> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut turns: Vec<SessionTurn> = vec![];
    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if let Some(text) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        turns.push(SessionTurn { role: "user".into(), text });
                    }
                }
            }
            Some("assistant") => {
                if let Some(content_arr) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    let text: String = content_arr
                        .iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                block.get("text").and_then(|t| t.as_str()).map(|s| s.trim().to_string())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        turns.push(SessionTurn { role: "assistant".into(), text });
                    }
                }
            }
            _ => {}
        }
    }
    turns
}

pub(crate) fn session_summary_from_path(turns_path: &Path) -> Vec<String> {
    let turns = parse_session_turns(turns_path);
    turns
        .iter()
        .filter(|t| t.role == "user")
        .map(|t| {
            let line: String = t.text.lines().next().unwrap_or("").chars().take(100).collect();
            format!("• {}", line)
        })
        .collect()
}

pub(crate) fn session_is_exported(session: &AgentSession, project_path: &Path) -> bool {
    let id = match &session.id {
        Some(id) => id,
        None => return false,
    };
    let short_id = &id[..id.len().min(8)];
    let exports_dir = project_path.join(".pemguin").join("exports");
    fs::read_dir(&exports_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .contains(short_id)
        })
}

pub(crate) fn jsonl_path_for_session(session: &AgentSession, project_path: &Path) -> Option<PathBuf> {
    let id = session.id.as_ref()?;
    match session.agent {
        AgentKind::Claude => {
            for dir in claude_project_dirs(project_path) {
                let p = dir.join(format!("{}.jsonl", id));
                if p.exists() { return Some(p); }
            }
            None
        }
        AgentKind::Pi => {
            let home = dirs_home()?;
            let pi_dir = home.join(".pi").join("agent").join("sessions")
                .join(pi_encode_path(project_path));
            fs::read_dir(&pi_dir).ok()?
                .filter_map(|e| e.ok())
                .find(|e| {
                    let p = e.path();
                    p.extension().and_then(|x| x.to_str()) == Some("jsonl")
                        && p.file_stem().and_then(|s| s.to_str())
                            .map(|stem| stem.ends_with(id.as_str()))
                            .unwrap_or(false)
                })
                .map(|e| e.path())
        }
        _ => None,
    }
}

pub(crate) fn unix_to_iso(secs: u64) -> String {
    let min = secs / 60;
    let hour = min / 60;
    let day_total = hour / 24;
    let sec = secs % 60;
    let min = min % 60;
    let hour = hour % 24;
    let mut year = 1970u64;
    let mut days = day_total;
    loop {
        let y_days = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
        if days < y_days { break; }
        days -= y_days;
        year += 1;
    }
    let month_days = [31u64, if year % 4 == 0 { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    for &md in &month_days {
        if days < md { break; }
        days -= md;
        month += 1;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month + 1, days + 1, hour, min, sec)
}

pub(crate) fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    unix_to_iso(secs)
}

pub(crate) fn format_session_date(iso: &str) -> String {
    iso.trim_end_matches('Z')
        .replacen('T', " ", 1)
        .chars()
        .take(16)
        .collect()
}
