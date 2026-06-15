use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct ContextFileObservation {
    path: String,
    present: bool,
}

#[derive(Debug, Default, Serialize)]
struct InboxSummary {
    installed: bool,
    docs_present: bool,
    schema_present: bool,
    feedback_present: bool,
    counts: BTreeMap<String, usize>,
    active_count: usize,
    latest_title: Option<String>,
    latest_body: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct GitHubIssueSummary {
    repo: Option<String>,
    available: bool,
    open_count: usize,
    error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct GitHubIssueRecord {
    project_name: String,
    project_path: String,
    repo: String,
    number: u64,
    title: String,
    body: String,
    labels: Vec<String>,
    url: Option<String>,
    updated_at: Option<String>,
}

/// Freshness metadata attached to GitHub responses so the frontend can
/// display "last updated" and stale state without firing live calls on
/// every navigation event.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GitHubFreshness {
    /// Unix epoch seconds when the cache entry was last fetched from GitHub.
    fetched_at: u64,
    /// Whether the data exceeds its TTL and should be considered stale.
    stale: bool,
    /// "github-cache" | "github-live"
    source: String,
    /// Last fetch error, if any.
    error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GitHubRepoResponse {
    record: Option<GitHubRepoRecord>,
    freshness: GitHubFreshness,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GitHubIssuesResponse {
    records: Vec<GitHubIssueRecord>,
    freshness: GitHubFreshness,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct GitHubRepoRecord {
    project_name: String,
    project_path: String,
    repo: String,
    owner_avatar_url: Option<String>,
    repo_image_url: Option<String>,
    local_icon_path: Option<String>,
    local_icon_url: Option<String>,
    description: Option<String>,
    homepage_url: Option<String>,
    url: Option<String>,
    topics: Vec<String>,
    stars: Option<u64>,
    forks: Option<u64>,
    license: Option<String>,
    open_prs: Option<u64>,
    default_branch: Option<String>,
    pushed_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GitHubIssuesCacheEntry {
    fetched_at: u64,
    issues: Vec<GitHubIssueRecord>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GitHubRepoCacheEntry {
    fetched_at: u64,
    repo: GitHubRepoRecord,
}

type GitHubIssuesCache = BTreeMap<String, GitHubIssuesCacheEntry>;
type GitHubRepoCache = BTreeMap<String, GitHubRepoCacheEntry>;

#[derive(Debug, Serialize)]
struct AgentInboxRecord {
    project_name: String,
    project_path: String,
    id: String,
    kind: String,
    status: String,
    title: Option<String>,
    body: String,
    plan: Option<String>,
    route: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    context: Option<serde_json::Value>,
    comments: Option<serde_json::Value>,
    agent_notes: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ProjectVisuals {
    org_avatar_url: Option<String>,
    repo_image_url: Option<String>,
    local_icon_path: Option<String>,
    local_icon_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectObservation {
    name: String,
    path: String,
    context_files: Vec<ContextFileObservation>,
    inbox: InboxSummary,
    github_issues: GitHubIssueSummary,
    visuals: ProjectVisuals,
    readme: Option<String>,
    latest_commit_epoch: Option<u64>,
    latest_commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct GitSummary {
    branch: Option<String>,
    dirty_count: u32,
    ahead: u32,
    behind: u32,
    last_commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct LibraryAsset {
    kind: String,
    name: String,
    path: String,
    description: Option<String>,
    tags: Vec<String>,
    body: String,
}

#[derive(Debug, Serialize)]
struct AgentLibraryOverview {
    root: String,
    prompts: Vec<LibraryAsset>,
    recipes: Vec<LibraryAsset>,
    skills: Vec<LibraryAsset>,
}

#[derive(Debug, Serialize)]
struct AgentMemoryFile {
    agent: String,
    name: String,
    path: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ProjectSkillRecord {
    name: String,
    scope: String,
    source: String,
    path: String,
    seen_by: Vec<String>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpServerRecord {
    name: String,
    command: String,
    args: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentSessionRecord {
    agent: String,
    id: Option<String>,
    started_at: String,
    first_message: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectAgentsOverview {
    memories: Vec<AgentMemoryFile>,
    skills: Vec<ProjectSkillRecord>,
    mcp_servers: Vec<McpServerRecord>,
    sessions: Vec<AgentSessionRecord>,
}

#[derive(Clone, Debug, Serialize)]
struct LocalObservationEvent {
    reason: String,
    observed_at: u64,
    paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppOverview {
    contract: &'static str,
    projects_root: String,
    projects: Vec<ProjectObservation>,
    inbox_records: Vec<AgentInboxRecord>,
    github_issue_records: Vec<GitHubIssueRecord>,
    agent_library: AgentLibraryOverview,
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn github_issues_cache_path() -> PathBuf {
    home_dir()
        .unwrap_or_default()
        .join(".project-index")
        .join("cache")
        .join("github-issues.json")
}

fn github_repo_cache_path() -> PathBuf {
    home_dir()
        .unwrap_or_default()
        .join(".project-index")
        .join("cache")
        .join("github-repos.json")
}

fn load_github_issues_cache() -> GitHubIssuesCache {
    fs::read_to_string(github_issues_cache_path())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_github_issues_cache(cache: &GitHubIssuesCache) {
    let path = github_issues_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(path, json);
    }
}

fn load_github_repo_cache() -> GitHubRepoCache {
    fs::read_to_string(github_repo_cache_path())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_github_repo_cache(cache: &GitHubRepoCache) {
    let path = github_repo_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(path, json);
    }
}

fn default_projects_root() -> PathBuf {
    env::var_os("PROJECT_INDEX_PROJECTS_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join("Projects")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_agent_library_root() -> PathBuf {
    env::var_os("PROJECT_INDEX_AGENT_LIBRARY_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            home_dir().map(|home| home.join("Projects").join("_whaleen").join("agent-library"))
        })
        .unwrap_or_else(|| PathBuf::from("agent-library"))
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

fn scan_git_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let Ok(groups) = fs::read_dir(root) else {
        return repos;
    };

    for group in groups.flatten() {
        let group_path = group.path();
        if !group_path.is_dir() {
            continue;
        }
        if is_git_repo(&group_path) {
            repos.push(group_path.clone());
            continue;
        }
        let Ok(children) = fs::read_dir(&group_path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if child_path.is_dir() && is_git_repo(&child_path) {
                repos.push(child_path);
            }
        }
    }

    repos.sort();
    repos
}

fn context_files(path: &Path) -> Vec<ContextFileObservation> {
    [
        "SPEC.md",
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        ".mcp.json",
        "skills-lock.json",
        ".agents/skills",
        ".pi",
        ".agent/inbox/README.md",
        ".agent/inbox/schema.md",
        ".agent/inbox/feedback.jsonl",
    ]
    .into_iter()
    .map(|rel| ContextFileObservation {
        path: rel.to_string(),
        present: path.join(rel).exists(),
    })
    .collect()
}

fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn inbox_feedback_path(path: &Path) -> PathBuf {
    path.join(".agent").join("inbox").join("feedback.jsonl")
}

fn summarize_inbox(path: &Path) -> InboxSummary {
    let inbox_dir = path.join(".agent").join("inbox");
    let docs_present = inbox_dir.join("README.md").exists();
    let schema_present = inbox_dir.join("schema.md").exists();
    let feedback_path = inbox_feedback_path(path);
    let feedback_present = feedback_path.exists();
    let installed = docs_present || schema_present || feedback_present;

    let mut summary = InboxSummary {
        installed,
        docs_present,
        schema_present,
        feedback_present,
        ..InboxSummary::default()
    };

    let Ok(content) = fs::read_to_string(feedback_path) else {
        return summary;
    };

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let status = value
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        *summary.counts.entry(status.clone()).or_insert(0) += 1;
        if matches!(
            status.as_str(),
            "new" | "planned" | "accepted" | "in_progress"
        ) {
            summary.active_count += 1;
        }
        summary.latest_title = value
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or(summary.latest_title);
        summary.latest_body = value
            .get("body")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or(summary.latest_body);
    }

    summary
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn load_inbox_records(path: &Path) -> Vec<AgentInboxRecord> {
    let project_name = project_name(path);
    let project_path = path.display().to_string();
    let Ok(content) = fs::read_to_string(inbox_feedback_path(path)) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let id = string_field(&value, "id").unwrap_or_else(|| "unknown".to_string());
        let kind = string_field(&value, "kind").unwrap_or_else(|| "other".to_string());
        let status = string_field(&value, "status").unwrap_or_else(|| "unknown".to_string());
        let body = string_field(&value, "body").unwrap_or_default();

        records.push(AgentInboxRecord {
            project_name: project_name.clone(),
            project_path: project_path.clone(),
            id,
            kind,
            status,
            title: string_field(&value, "title"),
            body,
            plan: string_field(&value, "plan"),
            route: string_field(&value, "route"),
            created_at: string_field(&value, "createdAt"),
            updated_at: string_field(&value, "updatedAt"),
            context: value.get("context").cloned(),
            comments: value.get("comments").cloned(),
            agent_notes: value.get("agentNotes").cloned(),
        });
    }

    records.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    records
}

fn is_active_inbox_status(status: &str) -> bool {
    matches!(status, "new" | "planned" | "accepted" | "in_progress")
}

fn parse_github_repo(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches(".git");
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        return Some(rest.to_string());
    }
    None
}

fn github_owner_from_repo(repo: &str) -> Option<&str> {
    repo.split('/').next().filter(|owner| !owner.is_empty())
}

fn github_avatar_url(repo: &str) -> Option<String> {
    github_owner_from_repo(repo).map(|owner| format!("https://github.com/{owner}.png?size=96"))
}

fn icon_score(path: &Path) -> Option<u8> {
    let file = path.file_name()?.to_str()?.to_ascii_lowercase();
    let full = path.to_string_lossy().to_ascii_lowercase();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "png" | "svg" | "webp" | "jpg" | "jpeg" | "ico"
    ) {
        return None;
    }
    if full.contains("node_modules/") || full.contains("target/") || full.contains(".git/") {
        return None;
    }
    if file == "icon.png" && full.contains("src-tauri/icons/") {
        return Some(140);
    }
    if file == "128x128@2x.png" && full.contains("src-tauri/icons/") {
        return Some(138);
    }
    if file == "128x128.png" && full.contains("src-tauri/icons/") {
        return Some(136);
    }
    if file == "app-icon.png" || file == "app_icon.png" || file == "app-icon.icon" {
        return Some(94);
    }
    if file == "apple-touch-icon.png" {
        return Some(90);
    }
    if file == "favicon.svg" {
        return Some(86);
    }
    if file == "favicon.png" {
        return Some(84);
    }
    if file == "favicon.ico" {
        return Some(80);
    }
    // Generic `logo.*` is often a token/partner/vendor asset, not the app icon
    // (e.g. Solana token logos under public/assets or public/tokens). Only
    // accept it when it is clearly the app's own top-level public/source logo.
    if matches!(file.as_str(), "logo.webp" | "logo.png" | "logo.svg") {
        if full.ends_with("/public/logo.webp")
            || full.ends_with("/public/logo.png")
            || full.ends_with("/public/logo.svg")
            || full.ends_with("/src/logo.webp")
            || full.ends_with("/src/logo.png")
            || full.ends_with("/src/logo.svg")
        {
            return Some(62);
        }
    }
    None
}

fn walk_icon_candidates(dir: &Path, depth: u8, out: &mut Vec<(u8, PathBuf)>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(
                name,
                "node_modules" | "target" | ".git" | ".next" | ".turbo"
            ) {
                continue;
            }
            walk_icon_candidates(&path, depth - 1, out);
        } else if let Some(score) = icon_score(&path) {
            // Prefer source/public icons over generated dist assets when scores tie.
            let adjusted = if path.to_string_lossy().contains("/dist/") {
                score.saturating_sub(8)
            } else {
                score
            };
            out.push((adjusted, path));
        }
    }
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let idx = tag.find(&needle)? + needle.len();
    let rest = tag[idx..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)? + 1;
    Some(rest[1..end].to_string())
}

fn index_icon_candidates(project_root: &Path, project_name: &str) -> Vec<(u8, PathBuf)> {
    let mut out = Vec::new();
    let mut indexes = Vec::new();
    fn walk(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if matches!(
                    name,
                    "node_modules" | "target" | ".git" | ".next" | ".turbo" | "dist"
                ) {
                    continue;
                }
                walk(&path, depth - 1, out);
            } else if name == "index.html" {
                out.push(path);
            }
        }
    }
    walk(project_root, 4, &mut indexes);
    for index in indexes {
        let Ok(html) = fs::read_to_string(&index) else {
            continue;
        };
        for part in html.split('<') {
            let tag = part.split('>').next().unwrap_or("");
            let lower = tag.to_ascii_lowercase();
            if !lower.starts_with("link") || !lower.contains("rel=") || !lower.contains("icon") {
                continue;
            }
            let Some(href) = attr_value(tag, "href") else {
                continue;
            };
            if href.starts_with("http://")
                || href.starts_with("https://")
                || href.starts_with("data:")
            {
                continue;
            }
            let Some(app_dir) = index.parent() else {
                continue;
            };
            let candidate = if href.starts_with('/') {
                app_dir.join("public").join(href.trim_start_matches('/'))
            } else {
                app_dir.join(&href)
            };
            if candidate.exists() {
                let app_name = app_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let score = if app_name == project_name { 130 } else { 120 };
                out.push((score, candidate));
            }
        }
    }
    out
}

fn find_local_icon_path(path: &Path) -> Option<PathBuf> {
    let project = project_name(path);
    let mut candidates = index_icon_candidates(path, &project);
    walk_icon_candidates(path, 5, &mut candidates);
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().map(|(_, path)| path).next()
}

fn mime_for_icon(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        _ => "image/png",
    }
}

fn local_icon_data_url(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{};base64,{}", mime_for_icon(path), encoded))
}

fn cached_repo_image_url(repo: &str) -> Option<String> {
    load_github_repo_cache()
        .get(repo)
        .and_then(|entry| entry.repo.repo_image_url.clone())
}

fn project_visuals(path: &Path) -> ProjectVisuals {
    let repo = github_repo_for_project(path);
    let local_icon = find_local_icon_path(path);
    ProjectVisuals {
        org_avatar_url: repo.as_deref().and_then(github_avatar_url),
        repo_image_url: repo.as_deref().and_then(cached_repo_image_url),
        local_icon_path: local_icon.as_ref().map(|p| p.display().to_string()),
        local_icon_url: local_icon.as_ref().and_then(|p| local_icon_data_url(p)),
    }
}

fn github_repo_for_project(path: &Path) -> Option<String> {
    // Keep overview loading cheap: read `.git/config` directly instead of spawning
    // `git remote get-url` once per project. Worktrees with `.git` files can be
    // handled later by the shared core reader; for now, skip rather than block UI.
    let config = fs::read_to_string(path.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed == "[remote \"origin\"]";
            continue;
        }
        if in_origin {
            if let Some(url) = trimmed.strip_prefix("url =") {
                return parse_github_repo(url.trim());
            }
        }
    }
    None
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "command not found".to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("command timed out".to_string());
            }
            Err(err) => return Err(err.to_string()),
        }
    }
}

fn contextualize_issue_records(
    records: Vec<GitHubIssueRecord>,
    path: &Path,
    repo: &str,
) -> Vec<GitHubIssueRecord> {
    let project_name = project_name(path);
    let project_path = path.display().to_string();
    records
        .into_iter()
        .map(|mut record| {
            record.project_name = project_name.clone();
            record.project_path = project_path.clone();
            record.repo = repo.to_string();
            record
        })
        .collect()
}

fn load_cached_github_issue_records(path: &Path) -> Vec<GitHubIssueRecord> {
    let Some(repo) = github_repo_for_project(path) else {
        return Vec::new();
    };
    load_github_issues_cache()
        .remove(&repo)
        .map(|entry| contextualize_issue_records(entry.issues, path, &repo))
        .unwrap_or_default()
}

fn fetch_github_issue_records(path: &Path) -> Result<Vec<GitHubIssueRecord>, String> {
    let Some(repo) = github_repo_for_project(path) else {
        return Ok(Vec::new());
    };
    let mut command = Command::new("gh");
    command.args([
        "issue",
        "list",
        "--repo",
        &repo,
        "--json",
        "number,title,body,labels,url,updatedAt",
        "--limit",
        "25",
    ]);
    let output = command_output_with_timeout(command, Duration::from_secs(8))
        .map_err(|err| format!("gh issue list: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("JSON: {e}"))?;
    let project_name = project_name(path);
    let project_path = path.display().to_string();
    let records: Vec<GitHubIssueRecord> = value
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|item| {
            let number = item.get("number")?.as_u64()?;
            let labels = item
                .get("labels")
                .and_then(|v| v.as_array())
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|label| {
                    label
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .collect();
            Some(GitHubIssueRecord {
                project_name: project_name.clone(),
                project_path: project_path.clone(),
                repo: repo.clone(),
                number,
                title: string_field(item, "title").unwrap_or_default(),
                body: string_field(item, "body").unwrap_or_default(),
                labels,
                url: string_field(item, "url"),
                updated_at: string_field(item, "updatedAt"),
            })
        })
        .collect();

    let mut cache = load_github_issues_cache();
    cache.insert(
        repo,
        GitHubIssuesCacheEntry {
            fetched_at: now_epoch_secs(),
            issues: records.clone(),
        },
    );
    save_github_issues_cache(&cache);

    Ok(records)
}

const ISSUES_TTL_SECS: u64 = 15 * 60; // 15 minutes
const REPO_TTL_SECS: u64 = 60 * 60; // 60 minutes
const GITHUB_MIN_CALL_INTERVAL: Duration = Duration::from_millis(750);

static GITHUB_REMOTE_GATE: OnceLock<Mutex<()>> = OnceLock::new();
static GITHUB_LAST_CALL_STARTED: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn run_github_remote_call<T>(call: impl FnOnce() -> T) -> T {
    let _gate = GITHUB_REMOTE_GATE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    {
        let mut last_started = GITHUB_LAST_CALL_STARTED
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(last) = *last_started {
            let elapsed = last.elapsed();
            if elapsed < GITHUB_MIN_CALL_INTERVAL {
                thread::sleep(GITHUB_MIN_CALL_INTERVAL - elapsed);
            }
        }
        *last_started = Some(Instant::now());
    }

    call()
}

fn issues_freshness(fetched_at: u64, error: Option<String>, source: &str) -> GitHubFreshness {
    GitHubFreshness {
        fetched_at,
        stale: now_epoch_secs().saturating_sub(fetched_at) > ISSUES_TTL_SECS,
        source: source.to_string(),
        error,
    }
}

fn repo_freshness(fetched_at: u64, error: Option<String>, source: &str) -> GitHubFreshness {
    GitHubFreshness {
        fetched_at,
        stale: now_epoch_secs().saturating_sub(fetched_at) > REPO_TTL_SECS,
        source: source.to_string(),
        error,
    }
}

/// Return cached issues (if any) without hitting GitHub. Never auto-fetches.
fn query_github_issues_cached(path: &Path) -> GitHubIssuesResponse {
    let Some(repo) = github_repo_for_project(path) else {
        return GitHubIssuesResponse::default();
    };
    let cache = load_github_issues_cache();
    if let Some(entry) = cache.get(&repo) {
        return GitHubIssuesResponse {
            records: contextualize_issue_records(entry.issues.clone(), path, &repo),
            freshness: issues_freshness(entry.fetched_at, None, "github-cache"),
        };
    }
    GitHubIssuesResponse {
        records: vec![],
        freshness: GitHubFreshness {
            source: "github-cache".into(),
            ..Default::default()
        },
    }
}

/// Return cached repo record (if any) without hitting GitHub. Never auto-fetches.
fn query_github_repo_cached(path: &Path) -> GitHubRepoResponse {
    let Some(repo) = github_repo_for_project(path) else {
        return GitHubRepoResponse::default();
    };
    let cache = load_github_repo_cache();
    if let Some(entry) = cache.get(&repo) {
        return GitHubRepoResponse {
            record: Some(contextualize_repo_record(entry.repo.clone(), path, &repo)),
            freshness: repo_freshness(entry.fetched_at, None, "github-cache"),
        };
    }
    GitHubRepoResponse {
        record: None,
        freshness: GitHubFreshness {
            source: "github-cache".into(),
            ..Default::default()
        },
    }
}

/// Force-fetch issues from GitHub and update cache.
fn refresh_github_issues_live(path: &Path) -> GitHubIssuesResponse {
    let Some(repo) = github_repo_for_project(path) else {
        return GitHubIssuesResponse::default();
    };
    match run_github_remote_call(|| fetch_github_issue_records(path)) {
        Ok(records) => {
            let fetched_at = load_github_issues_cache()
                .get(&repo)
                .map(|e| e.fetched_at)
                .unwrap_or_else(now_epoch_secs);
            GitHubIssuesResponse {
                records,
                freshness: issues_freshness(fetched_at, None, "github-live"),
            }
        }
        Err(e) => {
            let cached = query_github_issues_cached(path);
            GitHubIssuesResponse {
                freshness: issues_freshness(cached.freshness.fetched_at, Some(e), "github-cache"),
                ..cached
            }
        }
    }
}

/// Force-fetch repo record from GitHub and update cache.
fn refresh_github_repo_live(path: &Path) -> GitHubRepoResponse {
    let Some(repo) = github_repo_for_project(path) else {
        return GitHubRepoResponse::default();
    };
    match run_github_remote_call(|| fetch_github_repo_record(path)) {
        Ok(record) => {
            let fetched_at = load_github_repo_cache()
                .get(&repo)
                .map(|e| e.fetched_at)
                .unwrap_or_else(now_epoch_secs);
            GitHubRepoResponse {
                record: Some(record),
                freshness: repo_freshness(fetched_at, None, "github-live"),
            }
        }
        Err(e) => {
            let cached = query_github_repo_cached(path);
            GitHubRepoResponse {
                freshness: repo_freshness(cached.freshness.fetched_at, Some(e), "github-cache"),
                ..cached
            }
        }
    }
}

#[allow(dead_code)]
fn load_github_issue_records(path: &Path) -> Result<Vec<GitHubIssueRecord>, String> {
    let Some(repo) = github_repo_for_project(path) else {
        return Ok(Vec::new());
    };
    let cache = load_github_issues_cache();
    if let Some(entry) = cache.get(&repo) {
        if now_epoch_secs().saturating_sub(entry.fetched_at) < ISSUES_TTL_SECS {
            return Ok(contextualize_issue_records(
                entry.issues.clone(),
                path,
                &repo,
            ));
        }
    }
    fetch_github_issue_records(path)
}

fn contextualize_repo_record(
    mut record: GitHubRepoRecord,
    path: &Path,
    repo: &str,
) -> GitHubRepoRecord {
    record.project_name = project_name(path);
    record.project_path = path.display().to_string();
    record.repo = repo.to_string();
    record.owner_avatar_url = github_avatar_url(repo);
    let local_icon = find_local_icon_path(path);
    record.local_icon_path = local_icon.as_ref().map(|p| p.display().to_string());
    record.local_icon_url = local_icon.as_ref().and_then(|p| local_icon_data_url(p));
    record
}

fn fetch_github_repo_record(path: &Path) -> Result<GitHubRepoRecord, String> {
    let Some(repo) = github_repo_for_project(path) else {
        return Err("No GitHub remote".to_string());
    };
    let mut command = Command::new("gh");
    command.args([
        "repo",
        "view",
        &repo,
        "--json",
        "description,homepageUrl,url,repositoryTopics,stargazerCount,forkCount,licenseInfo,pullRequests,defaultBranchRef,pushedAt,openGraphImageUrl",
    ]);
    let output = command_output_with_timeout(command, Duration::from_secs(8))
        .map_err(|err| format!("gh repo view: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("JSON: {e}"))?;
    let topics = value
        .get("repositoryTopics")
        .and_then(|v| v.as_array())
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|topic| {
            topic
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect();
    let record = GitHubRepoRecord {
        project_name: project_name(path),
        project_path: path.display().to_string(),
        repo: repo.clone(),
        owner_avatar_url: github_avatar_url(&repo),
        repo_image_url: string_field(&value, "openGraphImageUrl"),
        local_icon_path: find_local_icon_path(path)
            .as_ref()
            .map(|p| p.display().to_string()),
        local_icon_url: find_local_icon_path(path)
            .as_ref()
            .and_then(|p| local_icon_data_url(p)),
        description: string_field(&value, "description"),
        homepage_url: string_field(&value, "homepageUrl"),
        url: string_field(&value, "url"),
        topics,
        stars: value.get("stargazerCount").and_then(|v| v.as_u64()),
        forks: value.get("forkCount").and_then(|v| v.as_u64()),
        license: value
            .get("licenseInfo")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        open_prs: value
            .get("pullRequests")
            .and_then(|v| v.get("totalCount"))
            .and_then(|v| v.as_u64()),
        default_branch: value
            .get("defaultBranchRef")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        pushed_at: string_field(&value, "pushedAt"),
    };
    let mut cache = load_github_repo_cache();
    cache.insert(
        repo,
        GitHubRepoCacheEntry {
            fetched_at: now_epoch_secs(),
            repo: record.clone(),
        },
    );
    save_github_repo_cache(&cache);
    Ok(record)
}

#[allow(dead_code)]
fn load_github_repo_record(path: &Path) -> Result<GitHubRepoRecord, String> {
    let Some(repo) = github_repo_for_project(path) else {
        return Err("No GitHub remote".to_string());
    };
    let cache = load_github_repo_cache();
    if let Some(entry) = cache.get(&repo) {
        let fresh = now_epoch_secs().saturating_sub(entry.fetched_at) < REPO_TTL_SECS;
        let has_visuals =
            entry.repo.repo_image_url.is_some() || entry.repo.local_icon_path.is_some();
        if fresh && has_visuals {
            return Ok(contextualize_repo_record(entry.repo.clone(), path, &repo));
        }
    }
    fetch_github_repo_record(path)
}

fn summarize_github_issues(path: &Path) -> GitHubIssueSummary {
    let repo = github_repo_for_project(path);
    GitHubIssueSummary {
        available: repo.is_some(),
        repo,
        open_count: 0,
        error: None,
    }
}

fn git_output(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn inspect_git_summary_inner(path: &Path) -> GitSummary {
    let branch = git_output(path, &["branch", "--show-current"]).filter(|s| !s.is_empty());
    let dirty_count = git_output(path, &["status", "--porcelain"])
        .map(|s| s.lines().count() as u32)
        .unwrap_or(0);
    let (ahead, behind) = git_output(
        path,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    )
    .and_then(|s| {
        let mut parts = s.split_whitespace();
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    })
    .unwrap_or((0, 0));
    let last_commit = git_output(path, &["log", "-1", "--pretty=%h %cr %s"]);
    GitSummary {
        branch,
        dirty_count,
        ahead,
        behind,
        last_commit,
    }
}

fn frontmatter_field(content: &str, key: &str) -> Option<String> {
    if !content.trim_start().starts_with("---") {
        return None;
    }
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() == key {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn frontmatter_tags(content: &str) -> Vec<String> {
    frontmatter_field(content, "tags")
        .map(|value| {
            value
                .trim_matches(['[', ']'])
                .split(',')
                .map(|tag| tag.trim().trim_matches('"').to_string())
                .filter(|tag| !tag.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn scan_library_dir(root: &Path, subdir: &str, kind: &str) -> Vec<LibraryAsset> {
    let dir = root.join(subdir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut assets = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_path = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path.clone()
        };
        if file_path.extension().and_then(|e| e.to_str()) != Some("md") || !file_path.exists() {
            continue;
        }
        let Ok(body) = fs::read_to_string(&file_path) else {
            continue;
        };
        let fallback_name = file_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        assets.push(LibraryAsset {
            kind: kind.to_string(),
            name: frontmatter_field(&body, "name").unwrap_or(fallback_name),
            path: file_path.display().to_string(),
            description: frontmatter_field(&body, "description"),
            tags: frontmatter_tags(&body),
            body,
        });
    }
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    assets
}

fn load_agent_library() -> AgentLibraryOverview {
    let root = default_agent_library_root();
    AgentLibraryOverview {
        root: root.display().to_string(),
        prompts: scan_library_dir(&root, "prompts", "prompt"),
        recipes: scan_library_dir(&root, "recipes", "recipe"),
        skills: scan_library_dir(&root, "skills", "skill"),
    }
}

fn read_project_readme(path: &Path) -> Option<String> {
    ["README.md", "readme.md", "README.mdx"]
        .iter()
        .map(|name| path.join(name))
        .find(|candidate| candidate.exists())
        .and_then(|candidate| fs::read_to_string(candidate).ok())
}

fn sanitize_project_component(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '_'], "-")
}

fn claude_project_dirs(project_path: &Path) -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return vec![];
    };
    let base = home.join(".claude").join("projects");
    let path_str = project_path.to_string_lossy();
    let mut dirs = Vec::new();
    for encoded in [
        path_str.replace('/', "-"),
        path_str.replace('/', "-").replace('_', "-"),
    ] {
        let dir = base.join(encoded);
        if dir.exists() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

fn read_markdown_files(dir: &Path, agent: &str, out: &mut Vec<AgentMemoryFile>) {
    if !dir.is_dir() {
        return;
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("memory")
            .to_string();
        out.push(AgentMemoryFile {
            agent: agent.to_string(),
            name,
            path: path.display().to_string(),
            content,
        });
    }
}

fn load_agent_memories(path: &Path) -> Vec<AgentMemoryFile> {
    let mut out = Vec::new();
    for dir in claude_project_dirs(path) {
        read_markdown_files(&dir.join("memory"), "claude", &mut out);
    }
    if let Some(home) = home_dir() {
        let base = home.join(".codex").join("memories");
        let repo_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        for dir in [
            base.join(repo_name),
            base.join(sanitize_project_component(path)),
        ] {
            read_markdown_files(&dir, "codex", &mut out);
        }
        let gemini = home.join(".gemini").join("GEMINI.md");
        if let Ok(content) = fs::read_to_string(&gemini) {
            out.push(AgentMemoryFile {
                agent: "gemini".into(),
                name: "GEMINI".into(),
                path: gemini.display().to_string(),
                content,
            });
        }
        let pi_agent = home.join(".pi").join("agent");
        for file in ["AGENTS.md", "README.md"] {
            let candidate = pi_agent.join(file);
            if let Ok(content) = fs::read_to_string(&candidate) {
                out.push(AgentMemoryFile {
                    agent: "pi".into(),
                    name: file.trim_end_matches(".md").into(),
                    path: candidate.display().to_string(),
                    content,
                });
            }
        }
    }
    out.sort_by(|a, b| a.agent.cmp(&b.agent).then_with(|| a.name.cmp(&b.name)));
    out
}

fn load_skill_lock_sources(path: &Path) -> BTreeMap<String, String> {
    let Ok(content) = fs::read_to_string(path.join("skills-lock.json")) else {
        return BTreeMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return BTreeMap::new();
    };
    let Some(skills) = json.get("skills").and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };
    skills
        .iter()
        .map(|(name, val)| {
            let source = val
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("skills-lock.json")
                .to_string();
            (name.clone(), source)
        })
        .collect()
}

fn load_project_skills(path: &Path) -> Vec<ProjectSkillRecord> {
    let lock_sources = load_skill_lock_sources(path);
    let specs: [(&str, &str, &[&str], bool); 5] = [
        (
            ".agents/skills",
            "shared project",
            &["codex", "gemini", "pi"],
            false,
        ),
        (".claude/skills", "claude project", &["claude"], false),
        (".codex/skills", "codex project", &["codex"], false),
        (".gemini/skills", "gemini project", &["gemini"], false),
        (".pi/skills", "pi project", &["pi"], true),
    ];
    let mut out = Vec::new();
    for (rel, scope, seen_by, allow_root_md) in specs {
        let dir = path.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .map(|r| r.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();
        entries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());
        for entry in entries {
            let path = entry.path();
            let skill_path = if path.is_dir() && path.join("SKILL.md").is_file() {
                path.join("SKILL.md")
            } else if allow_root_md && path.extension().and_then(|e| e.to_str()) == Some("md") {
                path.clone()
            } else {
                continue;
            };
            let content = fs::read_to_string(&skill_path).unwrap_or_default();
            let fallback = skill_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string();
            let name = frontmatter_field(&content, "name").unwrap_or(fallback);
            let description = frontmatter_field(&content, "description");
            let source = lock_sources
                .get(&name)
                .cloned()
                .unwrap_or_else(|| "filesystem".into());
            out.push(ProjectSkillRecord {
                name,
                scope: scope.into(),
                source,
                path: skill_path.display().to_string(),
                seen_by: seen_by.iter().map(|s| s.to_string()).collect(),
                description,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.scope.cmp(&b.scope)));
    out
}

fn load_mcp_servers(path: &Path) -> Vec<McpServerRecord> {
    let Ok(content) = fs::read_to_string(path.join(".mcp.json")) else {
        return vec![];
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return vec![];
    };
    let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (name, val) in servers {
        let command = val
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let args = val
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(McpServerRecord {
            name: name.clone(),
            command,
            args,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn unix_to_iso(secs: u64) -> String {
    secs.to_string()
}

fn read_first_jsonl_message(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("type").and_then(|t| t.as_str()) == Some("user") {
            if let Some(text) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.chars().take(120).collect());
                }
            }
        }
        if v.get("type").and_then(|t| t.as_str()) == Some("message") {
            let msg = v.get("message")?;
            if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                    for block in arr {
                        let text = block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .trim();
                        if !text.is_empty() {
                            return Some(text.chars().take(120).collect());
                        }
                    }
                }
            }
        }
    }
    None
}

fn jsonl_session_files(dir: &Path) -> Vec<(PathBuf, String, u64)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let mtime = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((path, id, mtime))
        })
        .collect()
}

fn load_agent_sessions(project_path: &Path) -> Vec<AgentSessionRecord> {
    let mut out = Vec::new();
    for dir in claude_project_dirs(project_path) {
        for (path, id, mtime) in jsonl_session_files(&dir) {
            out.push(AgentSessionRecord {
                agent: "claude".into(),
                id: Some(id),
                started_at: unix_to_iso(mtime),
                first_message: read_first_jsonl_message(&path),
                path: Some(path.display().to_string()),
            });
        }
    }
    if let Some(home) = home_dir() {
        let pi_dir = home
            .join(".pi")
            .join("agent")
            .join("sessions")
            .join(format!(
                "--{}--",
                project_path
                    .to_string_lossy()
                    .trim_start_matches('/')
                    .replace('/', "-")
            ));
        for (path, id, mtime) in jsonl_session_files(&pi_dir) {
            out.push(AgentSessionRecord {
                agent: "pi".into(),
                id: Some(id),
                started_at: unix_to_iso(mtime),
                first_message: read_first_jsonl_message(&path),
                path: Some(path.display().to_string()),
            });
        }
        let codex_root = home.join(".codex").join("sessions");
        let project_str = project_path.to_string_lossy().to_string();
        for path in find_jsonl_limited(&codex_root, 4, 2500) {
            if let Some(session) = parse_codex_session_record(&path, &project_str) {
                out.push(session);
            }
        }
        out.extend(load_gemini_session_records(project_path, &home));
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out.truncate(80);
    out
}

fn find_jsonl_limited(root: &Path, depth: u8, max: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u8, max: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 || out.len() >= max {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if out.len() >= max {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth - 1, max, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, depth, max, &mut out);
    out
}

fn parse_codex_session_record(path: &Path, project_path: &str) -> Option<AgentSessionRecord> {
    let content = fs::read_to_string(path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(content.lines().next()?).ok()?;
    let payload = meta.get("payload")?;
    if payload.get("cwd").and_then(|v| v.as_str())? != project_path {
        return None;
    }
    let id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
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
        if p.get("role").and_then(|r| r.as_str()) != Some("user") {
            return None;
        }
        for block in p.get("content")?.as_array()? {
            let text = block
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim();
            if !text.is_empty() && !text.starts_with("<environment_context>") {
                return Some(text.chars().take(120).collect());
            }
        }
        None
    });
    Some(AgentSessionRecord {
        agent: "codex".into(),
        id,
        started_at,
        first_message,
        path: Some(path.display().to_string()),
    })
}

fn load_gemini_session_records(project_path: &Path, home: &Path) -> Vec<AgentSessionRecord> {
    let gemini_dir = home.join(".gemini");
    let project_str = project_path.to_string_lossy();
    let project_name = fs::read_to_string(gemini_dir.join("projects.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("projects")?
                .get(project_str.as_ref())?
                .as_str()
                .map(str::to_string)
        });
    let Some(project_name) = project_name else {
        return vec![];
    };
    let chats = gemini_dir.join("tmp").join(project_name).join("chats");
    let Ok(entries) = fs::read_dir(chats) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let id = v
            .get("sessionId")
            .and_then(|s| s.as_str())
            .map(str::to_string);
        let started_at = v
            .get("startTime")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let first_message = v
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|msgs| {
                msgs.iter().find_map(|msg| {
                    if msg.get("type").and_then(|t| t.as_str()) != Some("user") {
                        return None;
                    }
                    msg.get("content")?
                        .as_str()
                        .map(|s| s.chars().take(120).collect())
                })
            });
        out.push(AgentSessionRecord {
            agent: "gemini".into(),
            id,
            started_at,
            first_message,
            path: Some(path.display().to_string()),
        });
    }
    out
}

fn load_project_agents_overview(path: &Path) -> ProjectAgentsOverview {
    ProjectAgentsOverview {
        memories: load_agent_memories(path),
        skills: load_project_skills(path),
        mcp_servers: load_mcp_servers(path),
        sessions: load_agent_sessions(path),
    }
}

fn observe_project(path: &Path) -> ProjectObservation {
    ProjectObservation {
        name: project_name(path),
        path: path.display().to_string(),
        context_files: context_files(path),
        inbox: summarize_inbox(path),
        github_issues: summarize_github_issues(path),
        visuals: project_visuals(path),
        readme: read_project_readme(path),
        latest_commit_epoch: git_output(path, &["log", "-1", "--format=%ct"])
            .and_then(|s| s.parse().ok()),
        latest_commit: git_output(path, &["log", "-1", "--pretty=%h %cr %s"]),
    }
}

#[tauri::command]
fn app_overview() -> AppOverview {
    let root = default_projects_root();
    let repo_paths = scan_git_repos(&root);
    let projects = repo_paths
        .iter()
        .map(|path| observe_project(path))
        .collect();
    let mut inbox_records: Vec<AgentInboxRecord> = repo_paths
        .iter()
        .flat_map(|path| load_inbox_records(path))
        .filter(|record| is_active_inbox_status(&record.status))
        .collect();
    inbox_records.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });

    let mut github_issue_records: Vec<GitHubIssueRecord> = repo_paths
        .iter()
        .flat_map(|path| load_cached_github_issue_records(path))
        .collect();
    github_issue_records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    AppOverview {
        contract: "read-only desktop companion; observe local project and agent state only",
        projects_root: root.display().to_string(),
        projects,
        inbox_records,
        github_issue_records,
        agent_library: load_agent_library(),
    }
}

#[tauri::command]
fn inspect_project(path: String) -> ProjectObservation {
    observe_project(Path::new(&path))
}

#[tauri::command]
fn inspect_inbox(path: String) -> Vec<AgentInboxRecord> {
    load_inbox_records(Path::new(&path))
}

/// Return cached GitHub issues for a project path. Never fires a live call.
/// Use refresh_github_issues to force a live fetch.
#[tauri::command]
fn inspect_github_issues(path: String) -> GitHubIssuesResponse {
    query_github_issues_cached(Path::new(&path))
}

/// Return cached GitHub repo record for a project path. Never fires a live call.
/// Use refresh_github_repo to force a live fetch.
#[tauri::command]
fn inspect_github_repo(path: String) -> GitHubRepoResponse {
    query_github_repo_cached(Path::new(&path))
}

/// Force-refresh GitHub issues from the API and update the cache.
#[tauri::command]
fn refresh_github_issues(path: String) -> GitHubIssuesResponse {
    refresh_github_issues_live(Path::new(&path))
}

/// Force-refresh GitHub repo metadata from the API and update the cache.
#[tauri::command]
fn refresh_github_repo(path: String) -> GitHubRepoResponse {
    refresh_github_repo_live(Path::new(&path))
}

#[tauri::command]
fn inspect_git_summary(path: String) -> GitSummary {
    inspect_git_summary_inner(Path::new(&path))
}

#[tauri::command]
fn inspect_agent_library() -> AgentLibraryOverview {
    load_agent_library()
}

#[tauri::command]
fn inspect_project_agents(path: String) -> ProjectAgentsOverview {
    load_project_agents_overview(Path::new(&path))
}

fn watch_if_exists(watcher: &mut RecommendedWatcher, path: PathBuf, mode: RecursiveMode) {
    if path.exists() {
        let _ = watcher.watch(&path, mode);
    }
}

fn start_local_observation_watcher(app_handle: tauri::AppHandle) {
    thread::spawn(move || {
        let root = default_projects_root();
        let repo_paths = scan_git_repos(&root);
        let (tx, rx) = mpsc::channel();
        let Ok(mut watcher) = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            NotifyConfig::default(),
        ) else {
            return;
        };

        watch_if_exists(&mut watcher, root, RecursiveMode::NonRecursive);
        for repo_path in repo_paths {
            watch_if_exists(&mut watcher, repo_path.clone(), RecursiveMode::NonRecursive);
            watch_if_exists(
                &mut watcher,
                repo_path.join(".git"),
                RecursiveMode::Recursive,
            );
            watch_if_exists(
                &mut watcher,
                repo_path.join(".agent").join("inbox"),
                RecursiveMode::Recursive,
            );
            watch_if_exists(
                &mut watcher,
                repo_path.join(".claude"),
                RecursiveMode::Recursive,
            );
            watch_if_exists(
                &mut watcher,
                repo_path.join(".agents"),
                RecursiveMode::Recursive,
            );
        }

        let mut pending_paths = BTreeSet::new();
        let mut last_event_at: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(750)) {
                Ok(Ok(event)) => {
                    for path in event.paths {
                        pending_paths.insert(path.display().to_string());
                    }
                    last_event_at = Some(Instant::now());
                }
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !pending_paths.is_empty()
                        && last_event_at
                            .map(|instant| instant.elapsed() >= Duration::from_millis(750))
                            .unwrap_or(false)
                    {
                        let paths = pending_paths.iter().take(25).cloned().collect();
                        pending_paths.clear();
                        let _ = tauri::Emitter::emit(
                            &app_handle,
                            "observation://local-changed",
                            LocalObservationEvent {
                                reason: "filesystem".to_string(),
                                observed_at: now_epoch_secs(),
                                paths,
                            },
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            start_local_observation_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_overview,
            inspect_project,
            inspect_inbox,
            inspect_github_issues,
            inspect_github_repo,
            refresh_github_issues,
            refresh_github_repo,
            inspect_git_summary,
            inspect_agent_library,
            inspect_project_agents
        ])
        .run(tauri::generate_context!())
        .expect("error while running project-index desktop app");
}
