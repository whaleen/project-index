use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::Hash;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use arboard::Clipboard;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap},
};
use regex::Regex;

#[derive(serde::Serialize)]
struct CliProjectInspect {
    project_path: String,
    repo_root: bool,
    recommended_ok: usize,
    recommended_total: usize,
    setup_complete: bool,
    project_files: Vec<CliManagedPath>,
    items: Vec<CliSetupItem>,
    next_actions: Vec<String>,
}

#[derive(serde::Serialize)]
struct CliManagedPath {
    path: String,
    kind: &'static str,
    required: bool,
}

#[derive(serde::Serialize)]
struct CliSetupItem {
    id: String,
    label: String,
    detail: String,
    category: &'static str,
    status: &'static str,
    required: bool,
    optional: bool,
    gitignore_path: Option<String>,
    gitignored: bool,
    edit_path: Option<String>,
}

#[derive(serde::Serialize)]
struct CliAgentInstructions {
    project_path: String,
    summary: String,
    inspect_command: String,
    notes: Vec<String>,
}

struct McpToolDef {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize)]
struct ThemeConfig {
    #[serde(default = "default_accent")]  accent:  String,
    #[serde(default = "default_sel_fg")]  sel_fg:  String,
    #[serde(default = "default_fg_dim")]  fg_dim:  String,
    #[serde(default = "default_fg_xdim")] fg_xdim: String,
    #[serde(default = "default_green")]   green:   String,
    #[serde(default = "default_red")]     red:     String,
    #[serde(default = "default_yellow")]  yellow:  String,
    #[serde(default = "default_purple")]  purple:  String,
}

fn default_accent()  -> String { "#e8b887".into() }
fn default_sel_fg()  -> String { "#101010".into() }
fn default_fg_dim()  -> String { "#A0A0A0".into() }
fn default_fg_xdim() -> String { "#7E7E7E".into() }
fn default_green()   -> String { "#90b99f".into() }
fn default_red()     -> String { "#f5a191".into() }
fn default_yellow()  -> String { "#e6b99d".into() }
fn default_purple()  -> String { "#aca1cf".into() }

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent:  default_accent(),
            sel_fg:  default_sel_fg(),
            fg_dim:  default_fg_dim(),
            fg_xdim: default_fg_xdim(),
            green:   default_green(),
            red:     default_red(),
            yellow:  default_yellow(),
            purple:  default_purple(),
        }
    }
}

#[derive(Clone, Copy)]
struct Theme {
    accent: Color, sel_fg: Color, fg_dim: Color, fg_xdim: Color,
    green: Color, red: Color, yellow: Color, purple: Color,
}

fn hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(255);
    Color::Rgb(r, g, b)
}

thread_local! {
    static THEME: std::cell::Cell<Option<Theme>> = std::cell::Cell::new(None);
}

fn theme() -> Theme {
    THEME.with(|c| c.get().expect("theme not initialized"))
}

fn set_theme(cfg: &ThemeConfig) {
    let t = Theme {
        accent:  hex_color(&cfg.accent),
        sel_fg:  hex_color(&cfg.sel_fg),
        fg_dim:  hex_color(&cfg.fg_dim),
        fg_xdim: hex_color(&cfg.fg_xdim),
        green:   hex_color(&cfg.green),
        red:     hex_color(&cfg.red),
        yellow:  hex_color(&cfg.yellow),
        purple:  hex_color(&cfg.purple),
    };
    THEME.with(|c| c.set(Some(t)));
}

fn reload_pemguin_theme_if_changed(mtime: &mut Option<SystemTime>) {
    let Some(path) = dirs_home().map(|h| h.join(".pemguin.toml")) else { return };
    let Ok(meta) = fs::metadata(&path) else { return };
    let Ok(modified) = meta.modified() else { return };
    if mtime.map_or(true, |last| modified > last) {
        *mtime = Some(modified);
        if let Ok(s) = fs::read_to_string(&path) {
            if let Ok(cfg) = toml::from_str::<Config>(&s) {
                set_theme(&cfg.theme);
            }
        }
    }
}

// ── Icons (Nerd Fonts) ────────────────────────────────────────────────────────

const I_BRANCH: &str = "\u{e0a0}"; // git branch
const I_CHECK: &str = "\u{f00c}"; // ✓ check
const I_CROSS: &str = "\u{f12a}"; // ! exclamation (missing)
const I_WARN: &str = "\u{f071}"; // ⚠ triangle (stale)
const I_BULLET: &str = "\u{f111}"; // ● circle (active marker)
const I_COMMIT: &str = "\u{f1d3}"; // git commit
const I_ISSUES: &str = "\u{f41b}"; // github mark
const I_SETUP: &str = "\u{f013}"; // cog
const I_PROMPTS: &str = "\u{f0ae}"; // list
const I_PROJECTS: &str = "\u{f07b}"; // folder
const I_MEMORY: &str = "\u{f0eb}"; // lightbulb (memories)
const I_MCP: &str = "\u{f0c1}"; // link/chain
const I_PANE: &str = "\u{f120}"; // >_ terminal prompt

const PEMGUIN_MCP_SERVER_NAME: &str = "pemguin";

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize, Default)]
struct Config {
    #[serde(default)]
    projects: ProjectsConfig,
    #[serde(default)]
    theme: ThemeConfig,
}

#[derive(Clone, serde::Deserialize, Default)]
struct ProjectsConfig {
    root: Option<String>,
}

fn load_config() -> Config {
    let home = dirs_home().unwrap_or_default();
    let path = home.join(".pemguin.toml");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn sanitize_project_component(project_path: &Path) -> String {
    project_path
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

// ── Data ──────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RepoMeta {
    language: Option<String>,
    topics: Vec<String>,
    pushed_at: Option<String>, // ISO date string from GitHub
    open_issues: Option<u32>,
}

#[derive(Clone)]
struct Prompt {
    name: String,
    group: Option<String>,
    body: String,
    preview: String,
    placeholders: Vec<String>,
}

enum PromptDisplayRow {
    GroupHeader(String),
    Item(usize), // index into prompts vec
}

fn build_prompt_display_rows(prompts: &[Prompt]) -> Vec<PromptDisplayRow> {
    let mut rows = Vec::new();
    // Ungrouped prompts first
    for (i, p) in prompts.iter().enumerate() {
        if p.group.is_none() {
            rows.push(PromptDisplayRow::Item(i));
        }
    }
    // Then grouped, sorted by group name
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in prompts.iter().enumerate() {
        if let Some(g) = &p.group {
            groups.entry(g.clone()).or_default().push(i);
        }
    }
    for (group_name, indices) in groups {
        rows.push(PromptDisplayRow::GroupHeader(group_name));
        for i in indices {
            rows.push(PromptDisplayRow::Item(i));
        }
    }
    rows
}

struct MemoryFile {
    name: String,
    path: PathBuf,
    content: String,
    origin: Option<MemoryAgent>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MemoryAgent {
    Claude,
    Codex,
    Gemini,
    Pi,
}

impl MemoryAgent {
    fn label(self) -> &'static str {
        match self {
            MemoryAgent::Claude => "claude",
            MemoryAgent::Codex => "codex",
            MemoryAgent::Gemini => "gemini",
            MemoryAgent::Pi => "pi",
        }
    }
}


struct Issue {
    number: u64,
    title: String,
    body: String,
    labels: Vec<String>,
}

#[derive(Clone)]
struct Project {
    path: PathBuf,
    group: String, // parent dir name relative to base; "" for top-level repos
    repo: String,  // "owner/repo" or dir name
    branch: String,
    dirty_count: u32,  // number of uncommitted changes
    commits_ahead: u32,
    commits_behind: u32,
    recommended_ok: usize,
    recommended_total: usize,
    mcp_ready: bool,
    template_count: usize,
}

#[derive(Clone)]
enum ProjectEntry {
    Group(String), // section header
    Item(usize),   // index into app.projects
}

// ── Screens ───────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
enum ProjectTab {
    Home,
    Config,
    Issues,
    Prompts,
    Memories,
    Agents,
    Pane,
}

#[derive(PartialEq, Clone, Copy)]
enum AgentSection {
    Mcp,
    Skills,
    Sessions,
}

impl AgentSection {
    fn next(self) -> Self {
        match self {
            AgentSection::Mcp => AgentSection::Skills,
            AgentSection::Skills => AgentSection::Sessions,
            AgentSection::Sessions => AgentSection::Mcp,
        }
    }

    fn prev(self) -> Self {
        match self {
            AgentSection::Mcp => AgentSection::Sessions,
            AgentSection::Skills => AgentSection::Mcp,
            AgentSection::Sessions => AgentSection::Skills,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
enum AgentKind {
    Claude,
    Codex,
    Gemini,
    Pi,
}

impl AgentKind {
    fn label(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Gemini => "gemini",
            AgentKind::Pi => "pi",
        }
    }

    fn launch_cmd(&self, project_path: &Path, _prompt: Option<&str>) -> String {
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

    fn resume_cmd(&self, session_id: &str) -> String {
        match self {
            AgentKind::Claude => format!("claude --resume {}", session_id),
            AgentKind::Codex => format!("codex --session {}", session_id),
            AgentKind::Gemini => format!("gemini  # session: {}", session_id),
            AgentKind::Pi => format!("pi --session {}", session_id),
        }
    }

    fn from_str(s: &str) -> Option<Self> {
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
struct AgentSession {
    id: Option<String>,        // None until resolved from agent storage
    agent: AgentKind,
    started_at: String,        // ISO 8601
    prompt: Option<String>,
    first_message: Option<String>,
}

#[derive(PartialEq, Clone)]
enum SessionsState {
    List,
    NewPicker { agent_idx: usize, prompt_idx: Option<usize> },
    Summary { lines: Vec<String>, scroll: usize },
}

#[derive(PartialEq, Clone)]
enum MemoriesView {
    Claude,  // ~/.claude/projects/<encoded>/memory/
    Codex,   // ~/.codex/memories/<repo-name>/
    Gemini,  // ~/.gemini/GEMINI.md (global, single file)
}

#[derive(PartialEq, Clone)]
enum PromptsView {
    Global,
    Project,
}

#[derive(PartialEq, Clone)]
enum HomeEditField {
    Description,
    Homepage,
}

#[derive(PartialEq)]
enum Screen {
    Projects,              // root / launcher
    InProject(ProjectTab), // drilled into a project
}

// ── Home screen ───────────────────────────────────────────────────────────────

struct Skill {
    name: String,
    source: String,
    description: String,
}

struct RegistrySkill {
    name: String,
    source: String,
    installs: u64,
}

struct McpServer {
    name: String,
    command: String,
    args: Vec<String>,
}

struct ExternalCommand {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
}

struct TextEditorState {
    path: PathBuf,
    title: String,
    lines: Vec<String>,
    row: usize,
    col: usize,
    selection_anchor: Option<(usize, usize)>,
    status: Option<String>,
}

struct RecentCommit {
    hash: String,
    date_label: String,
    time_label: String,
    subject: String,
}

struct HomeData {
    gh_description: Option<String>, // GitHub repo description
    homepage: Option<String>,       // GitHub homepage URL (custom)
    url: String,                    // https://github.com/owner/repo
    recent_commits: Vec<RecentCommit>,
    recommended_ok: usize,
    recommended_total: usize,
    stack: Option<String>, // detected stack label
    stars: Option<u32>,
    forks: Option<u32>,
    license: Option<String>,
    open_prs: Option<u32>,
    readme: Option<String>,
    dirty_files: Vec<String>,
    mcp_ready: bool,
    sessions_count: usize,
    skills_count: usize,
}

fn md_frontmatter_status(path: &Path) -> (SetupStatus, Option<String>) {
    if !path.exists() {
        return (SetupStatus::Missing, None);
    }
    let (status, updated) = read_frontmatter(path);
    let s = match status.as_deref() {
        Some("template") => SetupStatus::Template,
        Some("stale")    => SetupStatus::Stale,
        Some("complete") => SetupStatus::Ok,
        _                => SetupStatus::Ok,
    };
    (s, updated)
}

fn setup_item_present(items: &[SetupItem], label: &str) -> bool {
    items.iter().any(|item| item.label == label && item.status == SetupStatus::Ok)
}

fn setup_template_count(items: &[SetupItem]) -> usize {
    items.iter().filter(|item| item.status == SetupStatus::Template).count()
}

fn count_sessions(project_path: &Path) -> usize {
    load_sessions(project_path).len()
}

fn detect_stack(path: &Path) -> Option<String> {
    if let Ok(s) = fs::read_to_string(path.join("Cargo.toml")) {
        let name = s.lines().find_map(|l| {
            l.strip_prefix("name = ")
                .map(|v| v.trim_matches('"').to_string())
        });
        return Some(format!("Rust ({})", name.as_deref().unwrap_or("?")));
    }
    if let Ok(s) = fs::read_to_string(path.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let name = v["name"].as_str().unwrap_or("?");
            let stack = if path.join("next.config.ts").exists()
                || path.join("next.config.js").exists()
            {
                "Next.js"
            } else if path.join("vite.config.ts").exists() || path.join("vite.config.js").exists() {
                "Vite"
            } else {
                "Node"
            };
            return Some(format!("{stack} ({name})"));
        }
    }
    if path.join("go.mod").exists() {
        return Some("Go".to_string());
    }
    None
}

fn load_recent_commits(path: &Path) -> Vec<RecentCommit> {
    git_in(
        path,
        &[
            "log",
            "--date=format:%A, %B %-d %Y",
            "--pretty=format:%h%x09%ad%x09%aI%x09%s",
            "-6",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|line| {
        let mut parts = line.splitn(4, '\t');
        let hash = parts.next()?.to_string();
        let date_label = parts.next()?.to_string();
        let timestamp = parts.next()?;
        let subject = parts.next()?.to_string();
        let (_, rest) = timestamp.split_once('T')?;
        let time_part = rest.get(0..5)?;
        let time_label = time_part.get(0..5)?.to_string();
        Some(RecentCommit {
            hash,
            date_label,
            time_label,
            subject,
        })
    })
    .collect()
}

fn load_readme(path: &Path) -> Option<String> {
    ["README.md", "readme.md", "Readme.md", "README"]
        .iter()
        .find_map(|name| fs::read_to_string(path.join(name)).ok())
}

fn load_dirty_files(path: &Path) -> Vec<String> {
    git_in(path, &["status", "--short"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn load_home_data(path: &Path, repo: &str) -> HomeData {
    let (gh_description, homepage, stars, forks, license, open_prs) = if !repo.is_empty() {
        let out = Command::new("gh")
            .args(["repo", "view", repo, "--json", "description,homepageUrl,stargazerCount,forkCount,licenseInfo,pullRequests"])
            .output()
            .ok()
            .filter(|o| o.status.success());
        if let Some(out) = out {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                let desc = v["description"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let home = v["homepageUrl"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let stars = v["stargazerCount"].as_u64().map(|n| n as u32);
                let forks = v["forkCount"].as_u64().map(|n| n as u32);
                let license = v["licenseInfo"]["spdxId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && *s != "NOASSERTION")
                    .map(|s| s.to_string());
                let open_prs = v["pullRequests"]["totalCount"].as_u64().map(|n| n as u32);
                (desc, home, stars, forks, license, open_prs)
            } else {
                (None, None, None, None, None, None)
            }
        } else {
            (None, None, None, None, None, None)
        }
    } else {
        (None, None, None, None, None, None)
    };

    let url = if repo.contains('/') {
        format!("https://github.com/{repo}")
    } else {
        String::new()
    };

    // Recent commits
    let recent_commits = load_recent_commits(path);

    let items = scan_setup(path);
    let recommended_ok = items.iter().filter(|i| i.status == SetupStatus::Ok).count();
    let recommended_total = items.len();
    let mcp_ready = path.join(".mcp.json").exists();
    let sessions_count = count_sessions(path);
    let skills_count = load_skills(path).len();

    let stack = detect_stack(path);
    let readme = load_readme(path);
    let dirty_files = load_dirty_files(path);

    HomeData {
        gh_description,
        homepage,
        url,
        recent_commits,
        recommended_ok,
        recommended_total,
        stack,
        stars,
        forks,
        license,
        open_prs,
        readme,
        dirty_files,
        mcp_ready,
        sessions_count,
        skills_count,
    }
}

fn load_home_data_local(path: &Path, repo: &str) -> HomeData {
    let url = if repo.contains('/') {
        format!("https://github.com/{repo}")
    } else {
        String::new()
    };

    let recent_commits = load_recent_commits(path);

    let items = scan_setup(path);
    let recommended_ok = items.iter().filter(|i| i.status == SetupStatus::Ok).count();
    let recommended_total = items.len();
    let mcp_ready = path.join(".mcp.json").exists();
    let sessions_count = count_sessions(path);
    let skills_count = load_skills(path).len();

    let stack = detect_stack(path);
    let readme = load_readme(path);
    let dirty_files = load_dirty_files(path);

    HomeData {
        gh_description: None,
        homepage: None,
        url,
        recent_commits,
        recommended_ok,
        recommended_total,
        stack,
        stars: None,
        forks: None,
        license: None,
        open_prs: None,
        readme,
        dirty_files,
        mcp_ready,
        sessions_count,
        skills_count,
    }
}

// ── Setup screen ──────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
enum SetupStatus {
    Ok,
    Missing,
    Stale,
    Template,
}

impl SetupStatus {
    fn as_str(&self) -> &'static str {
        match self {
            SetupStatus::Ok => "ok",
            SetupStatus::Missing => "missing",
            SetupStatus::Stale => "stale",
            SetupStatus::Template => "template",
        }
    }
}

enum SetupAction {
    Apply,
    Reset,
    Delete,
}

enum SetupRenderRow {
    GroupHeader(&'static str),
    Item(usize), // index into setup_items
}

fn setup_render_rows(items: &[SetupItem]) -> Vec<SetupRenderRow> {
    let mut rows = vec![];
    for (label, category) in [
        ("initialization", SetupCategory::Initialization),
        ("recommended files", SetupCategory::Recommended),
        ("repair", SetupCategory::Repair),
    ] {
        let indices: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| (it.category == category).then_some(i))
            .collect();
        if indices.is_empty() {
            continue;
        }
        rows.push(SetupRenderRow::GroupHeader(label));
        for i in indices {
            rows.push(SetupRenderRow::Item(i));
        }
    }
    rows
}

// Returns the setup_items index for the currently selected render row, if it is an Item.
fn selected_setup_item(items: &[SetupItem], list_state: &ListState) -> Option<usize> {
    let sel = list_state.selected()?;
    let rows = setup_render_rows(items);
    match rows.get(sel)? {
        SetupRenderRow::Item(i) => Some(*i),
        SetupRenderRow::GroupHeader(_) => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupCategory {
    Initialization,
    Recommended,
    Repair,
}

impl SetupCategory {
    fn as_str(&self) -> &'static str {
        match self {
            SetupCategory::Initialization => "initialization",
            SetupCategory::Recommended => "recommended",
            SetupCategory::Repair => "repair",
        }
    }
}

#[derive(Clone)]
struct SetupItem {
    label: &'static str,
    detail: String,
    category: SetupCategory,
    status: SetupStatus,
    gitignore_path: Option<&'static str>,  // entry in .gitignore, None if not toggleable
    gitignored: bool,                       // currently present in # Pemguin TUI block
}

fn read_frontmatter(path: &Path) -> (Option<String>, Option<String>) {
    // returns (status, updated)
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None);
    }
    let mut status = None;
    let mut updated = None;
    for line in lines {
        if line.trim() == "---" { break; }
        if let Some(rest) = line.strip_prefix("status:") {
            status = Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("updated:") {
            updated = Some(rest.trim().to_string());
        }
    }
    (status, updated)
}

fn read_frontmatter_status(path: &Path) -> Option<String> {
    read_frontmatter(path).0
}

fn md_file_status(path: &Path, base_detail: &str) -> (SetupStatus, String) {
    if !path.exists() {
        return (SetupStatus::Missing, base_detail.to_string());
    }
    match read_frontmatter_status(path).as_deref() {
        Some("template") => (SetupStatus::Template, format!("{base_detail} — template")),
        Some("stale")    => (SetupStatus::Stale,    format!("{base_detail} — stale")),
        Some("complete") => (SetupStatus::Ok,        base_detail.to_string()),
        _                => (SetupStatus::Ok,        base_detail.to_string()),
    }
}

fn scan_setup(path: &Path) -> Vec<SetupItem> {
    scan_setup_all(path)
}

fn scan_setup_all(path: &Path) -> Vec<SetupItem> {
    scan_setup_unfiltered(path)
}

fn scan_setup_unfiltered(path: &Path) -> Vec<SetupItem> {
    let spec_ok = path.join("SPEC.md").exists();
    let claude_ok = { let p = path.join("CLAUDE.md"); p.is_symlink() || p.exists() };
    let agents_ok = { let p = path.join("AGENTS.md"); p.is_symlink() || p.exists() };
    let gemini_ok = { let p = path.join("GEMINI.md"); p.is_symlink() || p.exists() };
    let mcp_ok    = path.join(".mcp.json").exists();

    vec![
        SetupItem {
            label: "SPEC.md",
            detail: "project contract artifact".to_string(),
            category: SetupCategory::Recommended,
            status: if spec_ok { SetupStatus::Ok } else { SetupStatus::Missing },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "CLAUDE.md",
            detail: "Claude Code context file".to_string(),
            category: SetupCategory::Recommended,
            status: if claude_ok { SetupStatus::Ok } else { SetupStatus::Missing },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "AGENTS.md",
            detail: "Codex / Pi context file".to_string(),
            category: SetupCategory::Recommended,
            status: if agents_ok { SetupStatus::Ok } else { SetupStatus::Missing },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "GEMINI.md",
            detail: "Gemini CLI context file".to_string(),
            category: SetupCategory::Recommended,
            status: if gemini_ok { SetupStatus::Ok } else { SetupStatus::Missing },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: ".mcp.json",
            detail: "MCP server definitions".to_string(),
            category: SetupCategory::Recommended,
            status: if mcp_ok { SetupStatus::Ok } else { SetupStatus::Missing },
            gitignore_path: None,
            gitignored: false,
        },
    ]
}

fn setup_item_edit_path(project_path: &Path, item: &SetupItem) -> Option<PathBuf> {
    match item.label {
        "SPEC.md"    => Some(project_path.join("SPEC.md")),
        "CLAUDE.md"  => Some(project_path.join("CLAUDE.md")),
        "AGENTS.md"  => Some(project_path.join("AGENTS.md")),
        "GEMINI.md"  => Some(project_path.join("GEMINI.md")),
        ".mcp.json"  => Some(project_path.join(".mcp.json")),
        _ => None,
    }
}

enum PromptState {
    Browse {
        list_state: ListState,
    },
    Fill {
        prompt_idx: usize,
        field_idx: usize,
        values: HashMap<String, String>,
        input: String,
    },
    Done(String),
}

enum DeleteTarget {
    Prompt {
        path: PathBuf,
        name: String,
    },
    Memory {
        path: PathBuf,
        name: String,
    },
    Setup {
        project_path: PathBuf,
        item: SetupItem,
    },
}

struct DeleteConfirm {
    title: String,
    detail: String,
    target: DeleteTarget,
}

struct App {
    config: Config,
    screen: Screen,
    // Prompts
    global_prompts: Vec<Prompt>, // always loaded from $PEMGUIN_DIR/prompts
    project_prompts: Vec<Prompt>, // loaded from <project>/.pemguin/prompts/ on drill-in
    prompts_view: PromptsView,
    prompts: Vec<Prompt>, // current display list (points at global or project)
    prompt_display_rows: Vec<PromptDisplayRow>, // flat + grouped rows for rendering
    prompt_state: PromptState,
    prompt_input: String,
    prompt_inputting: bool,
    prompt_message: Option<String>,
    // Issues
    issues: Vec<Issue>,
    issue_list_state: ListState,
    issues_error: Option<String>,
    issues_loaded: bool,
    issues_loading: bool,
    // Projects (root screen)
    projects: Vec<Project>,
    project_entries: Vec<ProjectEntry>, // flat render list (Group headers + Item refs)
    project_list_state: TableState,
    active_project_idx: Option<usize>, // index into projects; set on drill-in
    projects_msg: Option<String>,      // transient status shown in footer
    projects_loading: bool,
    scan_generation: u64,
    // Home (project sub-screen)
    home_data: Option<HomeData>,
    home_remote_loaded: bool,
    home_loading: bool,
    home_readme_scroll: u16,
    home_edit: Option<HomeEditField>,
    home_edit_input: String,
    home_save_msg: Option<String>,
    // Setup (project sub-screen)
    setup_items: Vec<SetupItem>,
    setup_list_state: ListState,
    setup_message: Option<String>,
    setup_on_open: bool,
    // GitHub metadata cache (keyed by "owner/repo")
    meta_cache: HashMap<String, RepoMeta>,
    // Avatar cache (keyed by "owner" -> raw chafa ANSI output)
    avatar_cache: HashMap<String, String>,
    avatar_loading_owner: Option<String>,
    // Memories tab
    memories_view: MemoriesView,
    memory_files: Vec<MemoryFile>,
    memory_list_state: ListState,
    memory_message: Option<String>,
    memories_loaded: bool,
    memory_input: String,
    memory_inputting: bool,
    pending_editor: Option<PathBuf>,
    text_editor: Option<TextEditorState>,
    pending_delete: Option<DeleteConfirm>,
    pending_command: Option<ExternalCommand>,
    // Skills
    skills: Vec<Skill>,
    skills_list_state: ListState,
    skills_loaded: bool,
    skills_registry: Vec<RegistrySkill>,
    skills_registry_loaded: bool,
    skills_registry_loading: bool,
    skills_browse: bool,
    skills_browse_query: String,
    skills_browse_query_active: bool,
    skills_browse_list_state: ListState,
    skills_browse_indices: Vec<usize>,
    skills_install_message: Option<String>,
    // MCP
    agent_section: AgentSection,
    mcp_servers: Vec<McpServer>,
    mcp_list_state: ListState,
    mcp_loaded: bool,
    mcp_message: Option<String>,
    pane_list_state: ListState,
    pane_message: Option<String>,
    // Sessions
    sessions: Vec<AgentSession>,
    sessions_list_state: ListState,
    sessions_loaded: bool,
    sessions_state: SessionsState,
    sessions_message: Option<String>,
    // Active context
    context: String,
    repo: String,
    async_tx: Sender<AsyncResult>,
    async_rx: Receiver<AsyncResult>,
    theme_mtime: Option<SystemTime>,
}

enum AsyncResult {
    Home {
        repo: String,
        data: HomeData,
    },
    Issues {
        repo: String,
        result: Result<Vec<Issue>, String>,
    },
    Avatar {
        owner: String,
        ansi: Option<String>,
    },
    Projects {
        generation: u64,
        projects: Vec<Project>,
    },
    Registry(Vec<RegistrySkill>),
    SkillInstalled {
        name: String,
        result: Result<String, String>,
    },
}

// ── Prompt loading ────────────────────────────────────────────────────────────

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn load_prompts_from(dir: &Path) -> Vec<Prompt> {
    if !dir.is_dir() {
        return vec![];
    }

    let re = Regex::new(r"\{([A-Z][A-Z0-9_]*)\}").unwrap();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());

    let parse_prompt = |path: &std::path::Path, group: Option<String>| -> Option<Prompt> {
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            return None;
        }
        let name = path.file_stem()?.to_str()?.to_string();
        let content = fs::read_to_string(path).ok()?;
        let body = extract_body(&content);
        let mut placeholders: Vec<String> = Vec::new();
        for cap in re.captures_iter(&body) {
            let p = cap[1].to_string();
            if !placeholders.contains(&p) {
                placeholders.push(p);
            }
        }
        Some(Prompt { name, group, body, preview: content, placeholders })
    };

    let mut prompts = Vec::new();

    // Root-level .md files — ungrouped
    for entry in entries.iter().filter(|e| e.path().is_file()) {
        if let Some(p) = parse_prompt(&entry.path(), None) {
            prompts.push(p);
        }
    }

    // Subdirectories — each dir name becomes a group
    for entry in entries.iter().filter(|e| e.path().is_dir()) {
        let group_name = entry.file_name().to_string_lossy().into_owned();
        let mut subentries: Vec<_> = fs::read_dir(entry.path())
            .map(|r| r.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();
        subentries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());
        for subentry in subentries.iter().filter(|e| e.path().is_file()) {
            if let Some(p) = parse_prompt(&subentry.path(), Some(group_name.clone())) {
                prompts.push(p);
            }
        }
    }

    prompts
}

fn global_prompts_dir() -> PathBuf {
    dirs_home()
        .unwrap_or_default()
        .join(".pemguin")
        .join("prompts")
}

fn extract_body(content: &str) -> String {
    let mut in_block = false;
    let mut block: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.starts_with("```") && !in_block {
            in_block = true;
            continue;
        }
        if line.starts_with("```") && in_block {
            if !block.is_empty() {
                return block.join("\n");
            }
            in_block = false;
            block.clear();
            continue;
        }
        if in_block {
            block.push(line);
        }
    }
    content.to_string()
}

// ── Issue loading ─────────────────────────────────────────────────────────────

fn load_issues(repo: &str) -> Result<Vec<Issue>, String> {
    if repo.is_empty() {
        return Err("No repo context".to_string());
    }
    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--json",
            "number,title,body,labels,state",
            "--limit",
            "50",
        ])
        .output()
        .map_err(|_| "gh CLI not found".to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("JSON: {e}"))?;
    Ok(json
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|item| {
            let number = item["number"].as_u64()?;
            let title = item["title"].as_str().unwrap_or("").to_string();
            let body = item["body"].as_str().unwrap_or("").to_string();
            let labels = item["labels"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                .collect();
            Some(Issue {
                number,
                title,
                body,
                labels,
            })
        })
        .collect())
}

// ── Project scanning ──────────────────────────────────────────────────────────

fn scan_projects(config: &Config) -> Vec<Project> {
    let base = std::env::var("PEMGUIN_PROJECTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            config
                .projects
                .root
                .as_ref()
                .map(|r| PathBuf::from(expand_tilde(r)))
                .unwrap_or_else(|| {
                    dirs_home()
                        .map(|h| h.join("Projects"))
                        .unwrap_or_else(|| PathBuf::from("."))
                })
        });

    // Walk up to 2 levels for .git dirs
    let Ok(level1) = fs::read_dir(&base) else {
        return vec![];
    };
    let mut level1_dirs: Vec<_> = level1
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    level1_dirs.sort_by_key(|e| e.file_name());

    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    for entry in level1_dirs {
        let path = entry.path();
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        // Skip hidden dirs
        if dir_name.starts_with('.') {
            continue;
        }
        if path.join(".git").is_dir() {
            candidates.push((path, String::new()));
        } else if let Ok(level2) = fs::read_dir(&path) {
            let mut level2_dirs: Vec<_> = level2
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect();
            level2_dirs.sort_by_key(|e| e.file_name());
            for sub in level2_dirs {
                let sub_path = sub.path();
                if sub_path.join(".git").is_dir() {
                    candidates.push((sub_path, dir_name.clone()));
                }
            }
        }
    }

    let mut projects: Vec<Project> = Vec::new();
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(candidates.len().max(1));
    let mut buckets = vec![Vec::new(); worker_count];
    for (i, candidate) in candidates.into_iter().enumerate() {
        buckets[i % worker_count].push(candidate);
    }

    let mut threads = Vec::new();
    for bucket in buckets {
        threads.push(std::thread::spawn(move || {
            bucket
                .into_iter()
                .filter_map(|(path, group)| project_info(&path, group))
                .collect::<Vec<_>>()
        }));
    }
    for thread in threads {
        if let Ok(mut batch) = thread.join() {
            projects.append(&mut batch);
        }
    }

    // Sort: group first (empty last), then repo name
    projects.sort_by(|a, b| {
        let ga = if a.group.is_empty() { "\x7f" } else { &a.group }; // empty → sort last
        let gb = if b.group.is_empty() { "\x7f" } else { &b.group };
        ga.cmp(gb).then(a.repo.cmp(&b.repo))
    });
    projects
}

fn project_info(path: &Path, group: String) -> Option<Project> {
    let repo = git_in(path, &["remote", "get-url", "origin"])
        .map(|u| parse_repo(&u))
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string()
        });
    let (branch, dirty_count, ahead, behind) = git_status_summary(path);
    let setup_items = scan_setup(path);
    let recommended_ok = setup_items.iter().filter(|i| i.status == SetupStatus::Ok).count();
    let recommended_total = setup_items.len();
    let mcp_ready = path.join(".mcp.json").exists();
    let template_count = setup_template_count(&setup_items);
    Some(Project {
        path: path.to_path_buf(),
        group,
        repo,
        branch,
        dirty_count,
        commits_ahead: ahead,
        commits_behind: behind,
        recommended_ok,
        recommended_total,
        mcp_ready,
        template_count,
    })
}

fn git_status_summary(path: &Path) -> (String, u32, u32, u32) {
    let out = match git_in(path, &["status", "--porcelain=2", "--branch"]) {
        Some(s) => s,
        None => return ("?".to_string(), 0, 0, 0),
    };

    let mut branch = "?".to_string();
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut dirty_count = 0u32;

    for line in out.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            if head != "(detached)" {
                branch = head.to_string();
            }
            continue;
        }
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    behind = n.parse().unwrap_or(0);
                }
            }
            continue;
        }
        if !line.starts_with('#') && !line.is_empty() {
            dirty_count += 1;
        }
    }

    (branch, dirty_count, ahead, behind)
}

fn build_project_entries(projects: &[Project]) -> Vec<ProjectEntry> {
    let mut entries: Vec<ProjectEntry> = Vec::new();
    let mut last_group: Option<&str> = None;
    for (i, p) in projects.iter().enumerate() {
        let group_str = if p.group.is_empty() {
            None
        } else {
            Some(p.group.as_str())
        };
        if group_str != last_group {
            if let Some(g) = group_str {
                entries.push(ProjectEntry::Group(g.to_string()));
            }
            last_group = group_str;
        }
        entries.push(ProjectEntry::Item(i));
    }
    entries
}

// ── GitHub metadata cache ─────────────────────────────────────────────────────

fn meta_cache_path() -> PathBuf {
    dirs_home()
        .unwrap_or_default()
        .join(".pemguin")
        .join("cache.json")
}

fn load_meta_cache() -> HashMap<String, RepoMeta> {
    let path = meta_cache_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_meta_cache(cache: &HashMap<String, RepoMeta>) {
    let path = meta_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(&path, json);
    }
}

fn refresh_project_meta(repo: &str) -> Option<RepoMeta> {
    if !repo.contains('/') {
        return None;
    }
    let out = Command::new("gh")
        .args([
            "repo",
            "view",
            repo,
            "--json",
            "primaryLanguage,repositoryTopics,pushedAt,issues",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let language = v["primaryLanguage"]["name"].as_str().map(|s| s.to_string());
    let topics = v["repositoryTopics"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
        .collect();
    let pushed_at = v["pushedAt"].as_str().map(|s| s.to_string());
    let open_issues = v["issues"]["totalCount"].as_u64().map(|n| n as u32);
    Some(RepoMeta {
        language,
        topics,
        pushed_at,
        open_issues,
    })
}

fn lang_short(lang: &str) -> &str {
    match lang {
        "TypeScript" => "TS",
        "JavaScript" => "JS",
        "Rust" => "RS",
        "Go" => "Go",
        "Python" => "Py",
        "Ruby" => "Rb",
        "CSS" => "CS",
        "HTML" => "HT",
        "Shell" => "SH",
        "Svelte" => "SV",
        "Solidity" => "So",
        "Nix" => "Nx",
        other => {
            if other.len() >= 2 {
                &other[..2]
            } else {
                other
            }
        }
    }
}

fn relative_date(iso: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let parts: Vec<u64> = iso
        .splitn(2, 'T')
        .next()
        .unwrap_or("")
        .split('-')
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 3 {
        return String::new();
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    let month_days = [0u64, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_sum: u64 = (1..m as usize).map(|i| month_days[i]).sum();
    let days_epoch = (y - 1970) * 365 + (y - 1970) / 4 + month_sum + d - 1;
    let diff = now.saturating_sub(days_epoch * 86400) / 86400;
    match diff {
        0 => "today".to_string(),
        1..=6 => format!("{}d", diff),
        7..=29 => format!("{}w", diff / 7),
        30..=364 => format!("{}mo", diff / 30),
        _ => format!("{}y", diff / 365),
    }
}

// ── Git / system helpers ──────────────────────────────────────────────────────

fn load_editor_state(path: &Path) -> Result<TextEditorState, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    if content.ends_with('\n') || lines.is_empty() {
        lines.push(String::new());
    }
    Ok(TextEditorState {
        path: path.to_path_buf(),
        title: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("editor")
            .to_string(),
        lines,
        row: 0,
        col: 0,
        selection_anchor: None,
        status: None,
    })
}

fn save_editor_state(editor: &mut TextEditorState) -> Result<(), String> {
    let mut content = editor.lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    fs::write(&editor.path, content).map_err(|e| e.to_string())?;
    editor.status = Some("Saved.".to_string());
    Ok(())
}

fn pos_le(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1)
}

fn selection_bounds(editor: &TextEditorState) -> Option<((usize, usize), (usize, usize))> {
    let anchor = editor.selection_anchor?;
    let cursor = (editor.row, editor.col);
    if anchor == cursor {
        None
    } else if pos_le(anchor, cursor) {
        Some((anchor, cursor))
    } else {
        Some((cursor, anchor))
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum ByteClass {
    Space,
    Word,
    Punct,
}

fn classify_byte(b: u8) -> ByteClass {
    if b.is_ascii_whitespace() {
        ByteClass::Space
    } else if b.is_ascii_alphanumeric() || b == b'_' {
        ByteClass::Word
    } else {
        ByteClass::Punct
    }
}

fn move_word_left(editor: &mut TextEditorState) {
    loop {
        if editor.col == 0 {
            if editor.row == 0 {
                return;
            }
            editor.row -= 1;
            editor.col = editor.lines[editor.row].len();
            if editor.col == 0 {
                return;
            }
        }

        let line = editor.lines[editor.row].as_bytes();
        while editor.col > 0 && classify_byte(line[editor.col - 1]) == ByteClass::Space {
            editor.col -= 1;
        }
        if editor.col == 0 {
            continue;
        }
        let class = classify_byte(line[editor.col - 1]);
        while editor.col > 0 && classify_byte(line[editor.col - 1]) == class {
            editor.col -= 1;
        }
        return;
    }
}

fn move_word_right(editor: &mut TextEditorState) {
    loop {
        let len = editor.lines[editor.row].len();
        if editor.col >= len {
            if editor.row + 1 >= editor.lines.len() {
                return;
            }
            editor.row += 1;
            editor.col = 0;
            if editor.lines[editor.row].is_empty() {
                return;
            }
        }

        let line = editor.lines[editor.row].as_bytes();
        while editor.col < line.len() && classify_byte(line[editor.col]) == ByteClass::Space {
            editor.col += 1;
        }
        if editor.col >= line.len() {
            continue;
        }
        let class = classify_byte(line[editor.col]);
        while editor.col < line.len() && classify_byte(line[editor.col]) == class {
            editor.col += 1;
        }
        return;
    }
}

fn set_selection_mode(editor: &mut TextEditorState, extending: bool) {
    if extending {
        if editor.selection_anchor.is_none() {
            editor.selection_anchor = Some((editor.row, editor.col));
        }
    } else {
        editor.selection_anchor = None;
    }
    editor.status = None;
}

fn clear_selection(editor: &mut TextEditorState) {
    editor.selection_anchor = None;
}

fn selected_text(editor: &TextEditorState) -> Option<String> {
    let ((sr, sc), (er, ec)) = selection_bounds(editor)?;
    if sr == er {
        return Some(editor.lines[sr][sc..ec].to_string());
    }
    let mut out = String::new();
    out.push_str(&editor.lines[sr][sc..]);
    out.push('\n');
    for row in (sr + 1)..er {
        out.push_str(&editor.lines[row]);
        out.push('\n');
    }
    out.push_str(&editor.lines[er][..ec]);
    Some(out)
}

fn delete_selection(editor: &mut TextEditorState) -> bool {
    let Some(((sr, sc), (er, ec))) = selection_bounds(editor) else {
        return false;
    };
    if sr == er {
        editor.lines[sr].replace_range(sc..ec, "");
    } else {
        let prefix = editor.lines[sr][..sc].to_string();
        let suffix = editor.lines[er][ec..].to_string();
        editor.lines.splice(sr..=er, [format!("{prefix}{suffix}")]);
    }
    editor.row = sr;
    editor.col = sc;
    clear_selection(editor);
    true
}

fn insert_text(editor: &mut TextEditorState, text: &str) {
    let _ = delete_selection(editor);
    let parts: Vec<&str> = text.split('\n').collect();
    if parts.len() == 1 {
        editor.lines[editor.row].insert_str(editor.col, parts[0]);
        editor.col += parts[0].len();
        return;
    }

    let suffix = editor.lines[editor.row].split_off(editor.col);
    editor.lines[editor.row].push_str(parts[0]);
    let row = editor.row;
    for (i, part) in parts.iter().enumerate().skip(1) {
        editor.lines.insert(row + i, (*part).to_string());
    }
    let last_row = row + parts.len() - 1;
    editor.lines[last_row].push_str(&suffix);
    editor.row = last_row;
    editor.col = parts.last().map(|s| s.len()).unwrap_or(0);
}

fn duplicate_current_line(editor: &mut TextEditorState) {
    let line = editor.lines[editor.row].clone();
    editor.lines.insert(editor.row + 1, line);
    editor.row += 1;
    editor.col = 0;
    clear_selection(editor);
}

fn delete_current_line(editor: &mut TextEditorState) {
    if editor.lines.len() == 1 {
        editor.lines[0].clear();
    } else {
        editor.lines.remove(editor.row);
        if editor.row >= editor.lines.len() {
            editor.row = editor.lines.len() - 1;
        }
    }
    editor.col = editor.col.min(editor.lines[editor.row].len());
    clear_selection(editor);
}

fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_repo(url: &str) -> String {
    let url = url.trim().trim_end_matches(".git");
    // HTTPS: https://host/owner/repo or http://host/owner/repo
    if url.starts_with("https://") || url.starts_with("http://") {
        let prefix = if url.starts_with("https://") { 8 } else { 7 };
        if let Some(slash) = url[prefix..].find('/') {
            return url[prefix + slash + 1..].to_string();
        }
    }
    // SSH: git@host:owner/repo
    if let Some(pos) = url.rfind(':') {
        let after = &url[pos + 1..];
        if after.contains('/') {
            return after.to_string();
        }
    }
    url.to_string()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

// ── Memory helpers ────────────────────────────────────────────────────────────

fn claude_memory_path(project_path: &Path) -> Option<PathBuf> {
    // Returns the first existing ~/.claude/projects/<encoded>/memory/ dir.
    // Uses claude_project_dirs which checks both v1 and v2 path encodings.
    claude_project_dirs(project_path)
        .into_iter()
        .map(|d| d.join("memory"))
        .find(|d| d.is_dir())
}

fn load_memory_files(dir: &Path) -> Vec<MemoryFile> {
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


fn codex_memory_dirs(project_path: &Path) -> Vec<PathBuf> {
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

fn gemini_memory_path() -> PathBuf {
    dirs_home().unwrap_or_default().join(".gemini").join("GEMINI.md")
}

// ── Skills / MCP loading ──────────────────────────────────────────────────────

fn load_skills(path: &Path) -> Vec<Skill> {
    let lock_path = path.join("skills-lock.json");
    let content = match fs::read_to_string(&lock_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let skills_obj = match json.get("skills").and_then(|v| v.as_object()) {
        Some(o) => o.clone(),
        None => return vec![],
    };
    let mut skills = Vec::new();
    for (name, val) in &skills_obj {
        let source = val
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Try to read description from .agents/skills/<name>/SKILL.md frontmatter
        let skill_md = path
            .join(".agents")
            .join("skills")
            .join(name)
            .join("SKILL.md");
        let description = fs::read_to_string(&skill_md)
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("description:"))
                    .map(|l| l.trim_start_matches("description:").trim().to_string())
            })
            .unwrap_or_default();
        skills.push(Skill {
            name: name.clone(),
            source,
            description,
        });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn parse_registry_skills(arr: Vec<serde_json::Value>) -> Vec<RegistrySkill> {
    let mut skills: Vec<RegistrySkill> = arr
        .iter()
        .filter_map(|v| {
            Some(RegistrySkill {
                name: v.get("name")?.as_str()?.to_string(),
                source: v.get("source")?.as_str()?.to_string(),
                installs: v.get("installs")?.as_u64().unwrap_or(0),
            })
        })
        .collect();
    skills.sort_by(|a, b| b.installs.cmp(&a.installs));
    skills
}

fn fetch_skills_registry() -> Vec<RegistrySkill> {
    let body = match ureq::get("https://skills.sh/")
        .set("User-Agent", "pemguin/1.0")
        .call()
    {
        Ok(resp) => match resp.into_string() {
            Ok(s) => s,
            Err(_) => return vec![],
        },
        Err(_) => return vec![],
    };

    // Try plain JSON form first: "initialSkills":[...]
    let plain = "\"initialSkills\":";
    if let Some(start) = body.find(plain) {
        if let Some(json_str) = extract_json_value(&body[start + plain.len()..]) {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
                return parse_registry_skills(arr);
            }
        }
    }

    // Try RSC escaped form: \"initialSkills\":
    let escaped = "\\\"initialSkills\\\":";
    if let Some(start) = body.find(escaped) {
        let slice = &body[start + escaped.len()..];
        // Find opening '[' within a short window
        if let Some(bracket) = slice[..slice.len().min(10)].find('[') {
            if let Some(raw) = extract_escaped_json_array(&slice[bracket..]) {
                let unescaped = raw.replace("\\\"", "\"").replace("\\\\", "\\");
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&unescaped) {
                    return parse_registry_skills(arr);
                }
            }
        }
    }

    vec![]
}

// Extracts a JSON array from RSC-escaped content where all " are written as \"
fn extract_escaped_json_array(s: &str) -> Option<String> {
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2; // skip escape sequence
            continue;
        }
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[..=i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_json_value(s: &str) -> Option<String> {
    let first = s.chars().next()?;
    let (open, close) = match first {
        '[' => ('[', ']'),
        '{' => ('{', '}'),
        _ => return None,
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(s[..=i].to_string());
            }
        }
    }
    None
}

fn filter_registry(registry: &[RegistrySkill], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..registry.len()).collect();
    }
    let q = query.to_lowercase();
    registry
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.name.to_lowercase().contains(&q) || s.source.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

fn load_mcp_servers(path: &Path) -> Vec<McpServer> {
    let mcp_path = path.join(".mcp.json");
    let content = match fs::read_to_string(&mcp_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let servers = match json.get("mcpServers").and_then(|v| v.as_object()) {
        Some(o) => o.clone(),
        None => return vec![],
    };
    let mut result = Vec::new();
    for (name, val) in &servers {
        let command = val
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let args: Vec<String> = val
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        result.push(McpServer {
            name: name.clone(),
            command,
            args,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

fn managed_project_paths() -> Vec<CliManagedPath> {
    vec![
        CliManagedPath { path: "SPEC.md".to_string(), kind: "file", required: false },
        CliManagedPath { path: "AGENTS.md".to_string(), kind: "file", required: false },
        CliManagedPath { path: "CLAUDE.md".to_string(), kind: "file/symlink", required: false },
        CliManagedPath { path: "GEMINI.md".to_string(), kind: "file/symlink", required: false },
        CliManagedPath { path: ".mcp.json".to_string(), kind: "file", required: false },
        CliManagedPath { path: "skills-lock.json".to_string(), kind: "file", required: false },
        CliManagedPath { path: ".agents/skills/".to_string(), kind: "dir", required: false },
        CliManagedPath { path: ".pi/".to_string(), kind: "dir", required: false },
    ]
}

fn setup_item_required(_item: &SetupItem) -> bool {
    false
}

fn cli_setup_item(project_path: &Path, item: &SetupItem) -> CliSetupItem {
    CliSetupItem {
        id: item.label.to_string(),
        label: item.label.to_string(),
        detail: item.detail.clone(),
        category: item.category.as_str(),
        status: item.status.as_str(),
        required: setup_item_required(item),
        optional: !setup_item_required(item),
        gitignore_path: item.gitignore_path.map(|s| s.to_string()),
        gitignored: item.gitignored,
        edit_path: setup_item_edit_path(project_path, item)
            .map(|p| p.to_string_lossy().to_string()),
    }
}

fn cli_next_actions(_project_path: &Path, items: &[SetupItem]) -> Vec<String> {
    let mut notes = Vec::new();
    let missing: Vec<&str> = items
        .iter()
        .filter(|i| i.status == SetupStatus::Missing)
        .map(|i| i.label)
        .collect();
    if !missing.is_empty() {
        notes.push(format!("Observed missing context files: {}.", missing.join(", ")));
    }
    let stale: Vec<&str> = items
        .iter()
        .filter(|i| i.status == SetupStatus::Stale)
        .map(|i| i.label)
        .collect();
    if !stale.is_empty() {
        notes.push(format!("Observed stale context files: {}.", stale.join(", ")));
    }
    if notes.is_empty() {
        notes.push("All observed context files are present.".to_string());
    }
    notes
}

fn inspect_project(path: &Path) -> CliProjectInspect {
    let items = scan_setup_all(path);
    let recommended_ok = items.iter().filter(|i| i.status == SetupStatus::Ok).count();
    let recommended_total = items.len();
    let setup_complete = items
        .iter()
        .filter(|item| setup_item_required(item))
        .all(|item| item.status == SetupStatus::Ok);

    CliProjectInspect {
        project_path: path.to_string_lossy().to_string(),
        repo_root: path.join(".git").exists(),
        recommended_ok,
        recommended_total,
        setup_complete,
        project_files: managed_project_paths(),
        items: items.iter().map(|item| cli_setup_item(path, item)).collect(),
        next_actions: cli_next_actions(path, &items),
    }
}

fn agent_instructions(path: &Path) -> CliAgentInstructions {
    CliAgentInstructions {
        project_path: path.to_string_lossy().to_string(),
        summary: "Use pemguin as a read-only project and agent-state index.".to_string(),
        inspect_command: format!("pm project inspect --json {}", path.display()),
        notes: vec![
            "Read `SPEC.md`, `AGENTS.md`, `CLAUDE.md`, or `GEMINI.md` when they exist.".to_string(),
            "Do not ask pemguin to create, repair, or mutate project files.".to_string(),
        ],
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let out = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    println!("{out}");
    Ok(())
}

fn cli_usage() {
    println!("pemguin CLI");
    println!();
    println!("  pm project inspect [--json] [PATH]");
    println!("  pm context inspect [--json] [PATH]");
    println!("  pm agent instructions [--json] [PATH]");
    println!("  pm mcp serve");
    println!("  pm mcp-server");
    println!();
    println!("If no subcommand is given, the Ratatui TUI starts.");
}

fn parse_path_arg(args: &[String]) -> PathBuf {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn run_cli(args: &[String]) -> io::Result<bool> {
    if args.is_empty() {
        return Ok(false);
    }
    match args.first().map(|s| s.as_str()) {
        Some("help") | Some("--help") | Some("-h") => {
            cli_usage();
            Ok(true)
        }
        Some("project") if args.get(1).map(|s| s.as_str()) == Some("inspect") => {
            let path = parse_path_arg(&args[2..]);
            print_json(&inspect_project(&path))?;
            Ok(true)
        }
        Some("context") if args.get(1).map(|s| s.as_str()) == Some("inspect") => {
            let path = parse_path_arg(&args[2..]);
            print_json(&inspect_project(&path))?;
            Ok(true)
        }
        Some("setup") if args.get(1).map(|s| s.as_str()) == Some("plan") => {
            let path = parse_path_arg(&args[2..]);
            print_json(&inspect_project(&path))?;
            Ok(true)
        }
        Some("agent") if args.get(1).map(|s| s.as_str()) == Some("instructions") => {
            let path = parse_path_arg(&args[2..]);
            print_json(&agent_instructions(&path))?;
            Ok(true)
        }
        Some("mcp") if args.get(1).map(|s| s.as_str()) == Some("serve") => {
            run_mcp_server()?;
            Ok(true)
        }
        Some("mcp-server") => {
            run_mcp_server()?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn mcp_tool_defs() -> Vec<McpToolDef> {
    vec![
        McpToolDef {
            name: "pemguin_project_inspect",
            title: "Inspect Project Context",
            description: "Read-only inspection of observed project context files and agent-related surfaces.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Project root path. Defaults to the server process working directory."
                    }
                },
                "additionalProperties": false
            }),
        },
        McpToolDef {
            name: "pemguin_setup_plan",
            title: "Inspect Project Context (Legacy Setup Name)",
            description: "Legacy alias for read-only project context inspection. Does not create or repair files.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Project root path. Defaults to the server process working directory."
                    }
                },
                "additionalProperties": false
            }),
        },
        McpToolDef {
            name: "pemguin_agent_instructions",
            title: "Pemguin Agent Instructions",
            description: "Return read-only pemguin instructions an agent should follow when inspecting project context.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Project root path. Defaults to the server process working directory."
                    }
                },
                "additionalProperties": false
            }),
        },
    ]
}

fn mcp_default_path(arguments: Option<&serde_json::Value>) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(args) = arguments else {
        return Ok(cwd);
    };
    let Some(obj) = args.as_object() else {
        return Err("arguments must be a JSON object".to_string());
    };
    match obj.get("path") {
        Some(v) => v
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| "path must be a string".to_string()),
        None => Ok(cwd),
    }
}

fn mcp_tool_result(name: &str, arguments: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
    match name {
        "pemguin_project_inspect" => {
            let path = mcp_default_path(arguments)?;
            serde_json::to_value(inspect_project(&path)).map_err(|e| e.to_string())
        }
        "pemguin_setup_plan" => {
            let path = mcp_default_path(arguments)?;
            serde_json::to_value(inspect_project(&path)).map_err(|e| e.to_string())
        }
        "pemguin_agent_instructions" => {
            let path = mcp_default_path(arguments)?;
            serde_json::to_value(agent_instructions(&path)).map_err(|e| e.to_string())
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn mcp_success_response(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn mcp_error_response(id: Option<serde_json::Value>, code: i64, message: &str) -> serde_json::Value {
    let mut response = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message
        }
    });
    if let Some(id) = id {
        response["id"] = id;
    }
    response
}

fn write_mcp_message(stdout: &mut impl Write, value: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)
        .map_err(|e| io::Error::other(e.to_string()))?;
    write!(stdout, "Content-Length: {}\r\n\r\n", body.len())?;
    stdout.write_all(&body)?;
    stdout.flush()
}

fn read_mcp_message(stdin: &mut impl BufRead) -> io::Result<Option<serde_json::Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let bytes = stdin.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header"));
    };
    let mut body = vec![0u8; length];
    stdin.read_exact(&mut body)?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(value))
}

fn run_mcp_server() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());
    let tool_defs = mcp_tool_defs();
    let supported_versions = ["2025-11-25", "2024-11-05"];
    let mut initialized = false;

    while let Some(message) = read_mcp_message(&mut reader)? {
        let method = message.get("method").and_then(|v| v.as_str());
        let id = message.get("id").cloned();
        let params = message.get("params");

        let response = match method {
            Some("initialize") => {
                let requested = params
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("2025-11-25");
                let negotiated = if supported_versions.contains(&requested) {
                    requested
                } else {
                    supported_versions[0]
                };
                Some(mcp_success_response(
                    id.unwrap_or(serde_json::Value::Null),
                    serde_json::json!({
                        "protocolVersion": negotiated,
                        "capabilities": {
                            "tools": {
                                "listChanged": false
                            }
                        },
                        "serverInfo": {
                            "name": "pemguin",
                            "title": "Pemguin",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "Use pemguin tools as a read-only index of project and agent state. Inspect observed context; do not ask pemguin to create, repair, or mutate files."
                    }),
                ))
            }
            Some("notifications/initialized") => {
                initialized = true;
                None
            }
            Some("ping") => Some(mcp_success_response(
                id.unwrap_or(serde_json::Value::Null),
                serde_json::json!({}),
            )),
            Some("tools/list") => {
                if !initialized {
                    Some(mcp_error_response(id, -32002, "server not initialized"))
                } else {
                    let tools: Vec<serde_json::Value> = tool_defs
                        .iter()
                        .map(|tool| {
                            serde_json::json!({
                                "name": tool.name,
                                "title": tool.title,
                                "description": tool.description,
                                "inputSchema": tool.input_schema
                            })
                        })
                        .collect();
                    Some(mcp_success_response(
                        message.get("id").cloned().unwrap_or(serde_json::Value::Null),
                        serde_json::json!({ "tools": tools }),
                    ))
                }
            }
            Some("tools/call") => {
                if !initialized {
                    Some(mcp_error_response(id, -32002, "server not initialized"))
                } else {
                    let name = params
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str());
                    match name {
                        Some(name) => match mcp_tool_result(name, params.and_then(|p| p.get("arguments"))) {
                            Ok(value) => {
                                let text = serde_json::to_string_pretty(&value)
                                    .unwrap_or_else(|_| "{}".to_string());
                                Some(mcp_success_response(
                                    message.get("id").cloned().unwrap_or(serde_json::Value::Null),
                                    serde_json::json!({
                                        "content": [
                                            {
                                                "type": "text",
                                                "text": text
                                            }
                                        ],
                                        "structuredContent": value
                                    }),
                                ))
                            }
                            Err(err) => Some(mcp_success_response(
                                message.get("id").cloned().unwrap_or(serde_json::Value::Null),
                                serde_json::json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": format!("Error: {err}")
                                        }
                                    ],
                                    "isError": true
                                }),
                            )),
                        },
                        None => Some(mcp_error_response(id, -32602, "tools/call requires a string `name`")),
                    }
                }
            }
            Some(_) => id.map(|id| mcp_error_response(Some(id), -32601, "method not found")),
            None => Some(mcp_error_response(id, -32600, "invalid request")),
        };

        if let Some(response) = response {
            write_mcp_message(&mut writer, &response)?;
        }
    }

    Ok(())
}

// ── Avatar (chafa) ────────────────────────────────────────────────────────────

fn avatar_dir() -> PathBuf {
    dirs_home()
        .unwrap_or_default()
        .join(".pemguin")
        .join("avatars")
}

/// Download owner avatar and render via chafa. Returns raw ANSI string.
fn fetch_avatar(owner: &str) -> Option<String> {
    let dir = avatar_dir();
    let _ = fs::create_dir_all(&dir);
    let png = dir.join(format!("{owner}.png"));

    if !png.exists() {
        let url = format!("https://github.com/{owner}.png?size=128");
        let ok = Command::new("curl")
            .args(["-s", "-L", "-o", png.to_str().unwrap_or(""), &url])
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
    }

    let out = Command::new("chafa")
        .args([
            "--size",
            "20x10",
            "--format",
            "symbols",
            png.to_str().unwrap_or(""),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse chafa ANSI output into ratatui Lines.
fn ansi_to_lines(s: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            let mut seq = String::new();
            for nc in chars.by_ref() {
                if nc == 'm' {
                    break;
                }
                seq.push(nc);
            }
            if !text.is_empty() {
                spans.push(Span::styled(text.clone(), style));
                text.clear();
            }
            style = apply_sgr(style, &seq);
        } else if c == '\n' {
            if !text.is_empty() {
                spans.push(Span::styled(text.clone(), style));
                text.clear();
            }
            lines.push(Line::from(spans.clone()));
            spans.clear();
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn apply_sgr(mut style: Style, seq: &str) -> Style {
    let codes: Vec<u32> = seq.split(';').filter_map(|s| s.parse().ok()).collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            39 => style = style.fg(Color::Reset),
            49 => style = style.bg(Color::Reset),
            38 if codes.get(i + 1) == Some(&2) && i + 4 < codes.len() => {
                style = style.fg(Color::Rgb(
                    codes[i + 2] as u8,
                    codes[i + 3] as u8,
                    codes[i + 4] as u8,
                ));
                i += 4;
            }
            48 if codes.get(i + 1) == Some(&2) && i + 4 < codes.len() => {
                style = style.bg(Color::Rgb(
                    codes[i + 2] as u8,
                    codes[i + 3] as u8,
                    codes[i + 4] as u8,
                ));
                i += 4;
            }
            38 if codes.get(i + 1) == Some(&5) && i + 2 < codes.len() => {
                style = style.fg(ansi256(codes[i + 2] as u8));
                i += 2;
            }
            48 if codes.get(i + 1) == Some(&5) && i + 2 < codes.len() => {
                style = style.bg(ansi256(codes[i + 2] as u8));
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

fn ansi256(n: u8) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        16..=231 => {
            let v = n - 16;
            let b = (v % 6) * 51;
            let g = ((v / 6) % 6) * 51;
            let r = (v / 36) * 51;
            Color::Rgb(r, g, b)
        }
        232..=255 => {
            let v = (n - 232) * 10 + 8;
            Color::Rgb(v, v, v)
        }
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

impl App {
    fn new(config: Config) -> Self {
        let (async_tx, async_rx) = mpsc::channel();
        let projects = vec![];
        let project_entries = vec![];
        let global_prompts = vec![];
        let init_display_rows = vec![];

        let mut prompt_ls = ListState::default();
        let first_prompt = init_display_rows.iter().position(|r| matches!(r, PromptDisplayRow::Item(_)));
        prompt_ls.select(first_prompt);
        let mut project_ls = TableState::default();
        if let Some(first_item) = project_entries
            .iter()
            .position(|e| matches!(e, ProjectEntry::Item(_)))
        {
            project_ls.select(Some(first_item));
        }
        let mut setup_ls = ListState::default();
        setup_ls.select(Some(0));
        let mut pane_ls = ListState::default();
        pane_ls.select(Some(0));

        let prompts = vec![];
        let mut app = App {
            config,
            screen: Screen::Projects,
            global_prompts,
            project_prompts: vec![],
            prompts_view: PromptsView::Global,
            prompts,
            prompt_display_rows: init_display_rows,
            prompt_state: PromptState::Browse {
                list_state: prompt_ls,
            },
            prompt_input: String::new(),
            prompt_inputting: false,
            prompt_message: None,
            issues: vec![],
            issue_list_state: ListState::default(),
            issues_error: None,
            issues_loaded: false,
            issues_loading: false,
            projects,
            project_entries,
            project_list_state: project_ls,
            active_project_idx: None,
            projects_msg: None,
            projects_loading: false,
            scan_generation: 0,
            home_data: None,
            home_remote_loaded: false,
            home_loading: false,
            home_readme_scroll: 0,
            home_edit: None,
            home_edit_input: String::new(),
            home_save_msg: None,
            setup_items: vec![],
            setup_list_state: setup_ls,
            setup_message: None,
            setup_on_open: false,
            meta_cache: load_meta_cache(),
            avatar_cache: HashMap::new(),
            avatar_loading_owner: None,
            memories_view: MemoriesView::Claude,
            memory_files: vec![],
            memory_list_state: ListState::default(),
            memory_message: None,
            memories_loaded: false,
            memory_input: String::new(),
            memory_inputting: false,
            pending_editor: None,
            text_editor: None,
            pending_delete: None,
            pending_command: None,
            skills: vec![],
            skills_list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            skills_loaded: false,
            skills_registry: vec![],
            skills_registry_loaded: false,
            skills_registry_loading: false,
            skills_browse: false,
            skills_browse_query: String::new(),
            skills_browse_query_active: false,
            skills_browse_list_state: ListState::default(),
            skills_browse_indices: vec![],
            skills_install_message: None,
            agent_section: AgentSection::Mcp,
            mcp_servers: vec![],
            mcp_list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            mcp_loaded: false,
            mcp_message: None,
            pane_list_state: pane_ls,
            pane_message: None,
            sessions: vec![],
            sessions_list_state: ListState::default(),
            sessions_loaded: false,
            sessions_state: SessionsState::List,
            sessions_message: None,
            context: String::new(),
            repo: String::new(),
            async_tx,
            async_rx,
            theme_mtime: None,
        };
        app.start_projects_scan(false);
        app
    }

    fn switch_prompts_view(&mut self, view: PromptsView) {
        self.prompts_view = view.clone();
        self.prompts = match view {
            PromptsView::Global => self.global_prompts.clone(),
            PromptsView::Project => self.project_prompts.clone(),
        };
        self.prompt_display_rows = build_prompt_display_rows(&self.prompts);
        let mut ls = ListState::default();
        let first = self.prompt_display_rows.iter().position(|r| matches!(r, PromptDisplayRow::Item(_)));
        ls.select(first);
        self.prompt_state = PromptState::Browse { list_state: ls };
        self.prompt_message = None;
    }

    fn reload_project_prompts(&mut self) {
        if let Some(idx) = self.active_project_idx {
            if let Some(p) = self.projects.get(idx) {
                let dir = p.path.join(".pemguin").join("prompts");
                self.project_prompts = load_prompts_from(&dir);
                if self.prompts_view == PromptsView::Project {
                    self.prompts = self.project_prompts.clone();
                    self.prompt_display_rows = build_prompt_display_rows(&self.prompts);
                }
            }
        }
    }

    fn open_text_editor(&mut self, path: PathBuf) {
        match load_editor_state(&path) {
            Ok(editor) => {
                self.text_editor = Some(editor);
            }
            Err(e) => {
                self.prompt_message = Some(format!("Error: {e}"));
                self.setup_message = Some(format!("Error: {e}"));
                self.memory_message = Some(format!("Error: {e}"));
            }
        }
    }

    fn refresh_setup(&mut self) {
        if let Some(idx) = self.active_project_idx {
            if let Some(p) = self.projects.get(idx) {
                let path = p.path.clone();
                self.setup_items = scan_setup(&path);
                // Select the first real Item row (skip GroupHeader)
                let first = setup_render_rows(&self.setup_items)
                    .iter()
                    .position(|r| matches!(r, SetupRenderRow::Item(_)));
                self.setup_list_state.select(first);
                // Reload project prompts in case project prompt dir was just created
                self.reload_project_prompts();
            }
        } else {
            self.setup_items = vec![];
        }
        self.refresh_active_project_summary();
    }

    fn refresh_mcp(&mut self) {
        if let Some(idx) = self.active_project_idx {
            if let Some(project) = self.projects.get(idx) {
                self.mcp_servers = load_mcp_servers(&project.path);
                let mut state = ListState::default();
                if !self.mcp_servers.is_empty() {
                    state.select(Some(0));
                }
                self.mcp_list_state = state;
                self.mcp_loaded = true;
            }
        }
    }

    fn refresh_active_project_summary(&mut self) {
        let Some(idx) = self.active_project_idx else { return; };
        let Some(project) = self.projects.get(idx).cloned() else { return; };
        let items = scan_setup(&project.path);
        let recommended_ok = items.iter().filter(|i| i.status == SetupStatus::Ok).count();
        let recommended_total = items.len();

        if let Some(active) = self.projects.get_mut(idx) {
            active.recommended_ok = recommended_ok;
            active.recommended_total = recommended_total;
            active.mcp_ready = project.path.join(".mcp.json").exists();
            active.template_count = setup_template_count(&items);
        }

        if let Some(home) = self.home_data.as_mut() {
            home.recommended_ok = recommended_ok;
            home.recommended_total = recommended_total;
            home.mcp_ready = project.path.join(".mcp.json").exists();
            home.sessions_count = count_sessions(&project.path);
            home.skills_count = load_skills(&project.path).len();
        }

        self.setup_on_open = false;
    }

    fn auto_values(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if !self.repo.is_empty() {
            map.insert("REPO".to_string(), self.repo.clone());
        }
        map
    }

    fn selected_prompt_idx(&self) -> Option<usize> {
        if let PromptState::Browse { list_state } = &self.prompt_state {
            let display_idx = list_state.selected()?;
            match self.prompt_display_rows.get(display_idx)? {
                PromptDisplayRow::Item(i) => Some(*i),
                PromptDisplayRow::GroupHeader(_) => None,
            }
        } else {
            None
        }
    }

    fn issue_prompt_body(&self) -> String {
        self.prompts
            .iter()
            .find(|p| p.name.contains("issue") || p.name.contains("work-on"))
            .map(|p| p.body.clone())
            .unwrap_or_else(|| DEFAULT_ISSUE_PROMPT.to_string())
    }

    fn switch_project(&mut self, idx: usize) {
        let Some(project) = self.projects.get(idx) else {
            return;
        };
        self.repo = project.repo.clone();
        self.context = make_context(&project.repo, &project.branch);
        self.active_project_idx = Some(idx);
        let path = project.path.clone();
        // Load project-local prompts; default to project view if any exist
        self.project_prompts = load_prompts_from(&path.join(".pemguin").join("prompts"));
        let view = if !self.project_prompts.is_empty() {
            PromptsView::Project
        } else {
            PromptsView::Global
        };
        self.switch_prompts_view(view);
        // Load only cheap local data on project open; heavier tab data loads lazily.
        self.home_data = Some(load_home_data_local(&path, &self.repo.clone()));
        self.home_remote_loaded = false;
        self.home_loading = false;
        self.home_readme_scroll = 0;
        self.issues = vec![];
        self.issue_list_state = ListState::default();
        self.issues_error = None;
        self.issues_loaded = false;
        self.issues_loading = false;
        // Load setup
        self.setup_items = scan_setup(&path);
        if !self.setup_items.is_empty() {
            self.setup_list_state.select(Some(0));
        }
        self.setup_message = None;
        self.memories_view = MemoriesView::Claude;
        self.memory_files = vec![];
        self.memory_list_state = ListState::default();
        self.memory_message = None;
        self.memories_loaded = false;
        self.skills = vec![];
        self.skills_list_state = ListState::default();
        self.skills_loaded = false;
        self.agent_section = AgentSection::Mcp;
        self.mcp_servers = vec![];
        self.mcp_list_state = ListState::default();
        self.mcp_loaded = false;
        self.mcp_message = None;
        self.sessions = vec![];
        self.sessions_list_state = ListState::default();
        self.sessions_loaded = false;
        self.sessions_state = SessionsState::List;
        self.sessions_message = None;
        let repo = self.repo.clone();
        self.start_home_load(&path, &repo);
        // Drill in
        self.setup_on_open = false;
        self.screen = Screen::InProject(if self.setup_on_open {
            ProjectTab::Config
        } else {
            ProjectTab::Home
        });
    }

    fn ensure_tab_loaded(&mut self, tab: &ProjectTab) {
        let Some(idx) = self.active_project_idx else {
            return;
        };
        let Some(project) = self.projects.get(idx) else {
            return;
        };
        let path = project.path.clone();

        match tab {
            ProjectTab::Home if !self.home_remote_loaded && !self.home_loading => {
                let repo = self.repo.clone();
                self.start_home_load(&path, &repo);
            }
            ProjectTab::Config => {
                self.refresh_setup();
            }
            ProjectTab::Issues if !self.issues_loaded && !self.issues_loading => {
                let repo = self.repo.clone();
                self.start_issues_load(&repo);
            }
            ProjectTab::Memories if !self.memories_loaded => {
                self.reload_memories();
                self.memories_loaded = true;
            }
            ProjectTab::Agents if !self.skills_loaded => {
                self.skills = load_skills(&path);
                self.skills_list_state = {
                    let mut s = ListState::default();
                    if !self.skills.is_empty() {
                        s.select(Some(0));
                    }
                    s
                };
                self.skills_loaded = true;
                self.mcp_servers = load_mcp_servers(&path);
                self.mcp_list_state = {
                    let mut s = ListState::default();
                    if !self.mcp_servers.is_empty() {
                        s.select(Some(0));
                    }
                    s
                };
                self.mcp_loaded = true;
                let Some(idx) = self.active_project_idx else { return; };
                let Some(project) = self.projects.get(idx) else { return; };
                let path = project.path.clone();
                self.sessions = load_sessions(&path);
                resolve_sessions(&mut self.sessions, &path);
                self.sessions_list_state = {
                    let mut s = ListState::default();
                    if !self.sessions.is_empty() {
                        s.select(Some(0));
                    }
                    s
                };
                self.sessions_loaded = true;
            }
            _ => {}
        }
    }

    fn set_project_tab(&mut self, tab: ProjectTab) {
        self.screen = Screen::InProject(tab.clone());
        self.ensure_tab_loaded(&tab);
    }

    fn start_home_load(&mut self, path: &Path, repo: &str) {
        self.home_loading = true;
        let tx = self.async_tx.clone();
        let repo_owned = repo.to_string();
        let path_owned = path.to_path_buf();
        std::thread::spawn(move || {
            let data = load_home_data(&path_owned, &repo_owned);
            let _ = tx.send(AsyncResult::Home {
                repo: repo_owned,
                data,
            });
        });

        let owner = repo.split('/').next().unwrap_or("").to_string();
        if !owner.is_empty()
            && !self.avatar_cache.contains_key(&owner)
            && self.avatar_loading_owner.as_deref() != Some(owner.as_str())
        {
            self.avatar_loading_owner = Some(owner.clone());
            let tx = self.async_tx.clone();
            std::thread::spawn(move || {
                let ansi = fetch_avatar(&owner);
                let _ = tx.send(AsyncResult::Avatar { owner, ansi });
            });
        }
    }

    fn start_issues_load(&mut self, repo: &str) {
        self.issues_loading = true;
        self.issues_error = None;
        let tx = self.async_tx.clone();
        let repo_owned = repo.to_string();
        std::thread::spawn(move || {
            let result = load_issues(&repo_owned);
            let _ = tx.send(AsyncResult::Issues {
                repo: repo_owned,
                result,
            });
        });
    }

    fn start_projects_scan(&mut self, preserve_message: bool) {
        self.scan_generation += 1;
        self.projects_loading = true;
        if !preserve_message {
            self.projects_msg = Some("scanning projects...".to_string());
        }
        let tx = self.async_tx.clone();
        let generation = self.scan_generation;
        let config = self.config.clone();
        std::thread::spawn(move || {
            let projects = scan_projects(&config);
            let _ = tx.send(AsyncResult::Projects {
                generation,
                projects,
            });
        });
    }

    fn process_async_results(&mut self) {
        while let Ok(msg) = self.async_rx.try_recv() {
            match msg {
                AsyncResult::Home { repo, data } => {
                    if self.repo == repo {
                        self.home_data = Some(data);
                        self.home_remote_loaded = true;
                        self.home_loading = false;
                    }
                }
                AsyncResult::Issues { repo, result } => {
                    if self.repo == repo {
                        match result {
                            Ok(issues) => {
                                let mut ls = ListState::default();
                                if !issues.is_empty() {
                                    ls.select(Some(0));
                                }
                                self.issues = issues;
                                self.issue_list_state = ls;
                                self.issues_error = None;
                            }
                            Err(e) => {
                                self.issues = vec![];
                                self.issues_error = Some(e);
                            }
                        }
                        self.issues_loaded = true;
                        self.issues_loading = false;
                    }
                }
                AsyncResult::Avatar { owner, ansi } => {
                    if let Some(ansi) = ansi {
                        self.avatar_cache.insert(owner.clone(), ansi);
                    }
                    if self.avatar_loading_owner.as_deref() == Some(owner.as_str()) {
                        self.avatar_loading_owner = None;
                    }
                }
                AsyncResult::Projects {
                    generation,
                    projects,
                } => {
                    if generation == self.scan_generation {
                        self.projects = projects;
                        self.project_entries = build_project_entries(&self.projects);
                        self.project_list_state = {
                            let mut ls = TableState::default();
                            if let Some(first_item) = self
                                .project_entries
                                .iter()
                                .position(|e| matches!(e, ProjectEntry::Item(_)))
                            {
                                ls.select(Some(first_item));
                            }
                            ls
                        };
                        self.projects_loading = false;
                        self.projects_msg =
                            Some(format!("{} projects loaded", self.projects.len()));
                    }
                }
                AsyncResult::Registry(skills) => {
                    self.skills_registry = skills;
                    self.skills_registry_loaded = true;
                    self.skills_registry_loading = false;
                    self.skills_browse_indices = (0..self.skills_registry.len()).collect();
                    let mut ls = ListState::default();
                    if !self.skills_registry.is_empty() {
                        ls.select(Some(0));
                    }
                    self.skills_browse_list_state = ls;
                }
                AsyncResult::SkillInstalled { name, result } => {
                    match result {
                        Ok(msg) => {
                            self.skills_install_message = Some(msg);
                            self.skills_loaded = false; // trigger reload
                        }
                        Err(e) => {
                            self.skills_install_message = Some(format!("Error: {e}"));
                        }
                    }
                    let _ = name;
                }
            }
        }
    }

    fn memory_dir(&self) -> Option<PathBuf> {
        let project_path = self.active_project_idx
            .and_then(|i| self.projects.get(i))
            .map(|p| p.path.clone());
        match self.memories_view {
            MemoriesView::Claude => project_path.as_deref().and_then(claude_memory_path),
            MemoriesView::Codex  => project_path.as_deref().and_then(|p| codex_memory_dirs(p).into_iter().next()),
            MemoriesView::Gemini => None, // single file, not a dir
        }
    }

    fn reload_memories(&mut self) {
        self.memory_files = match self.memories_view {
            MemoriesView::Gemini => {
                let p = gemini_memory_path();
                if p.exists() {
                    let content = fs::read_to_string(&p).unwrap_or_default();
                    vec![MemoryFile { name: "GEMINI.md".to_string(), path: p, content, origin: Some(MemoryAgent::Gemini) }]
                } else {
                    vec![]
                }
            }
            _ => {
                self.memory_dir()
                    .map(|dir| load_memory_files(&dir))
                    .unwrap_or_default()
            }
        };
        let mut ls = ListState::default();
        if !self.memory_files.is_empty() {
            ls.select(Some(0));
        }
        self.memory_list_state = ls;
    }

    fn switch_memories_view(&mut self, view: MemoriesView) {
        self.memories_view = view;
        self.reload_memories();
        self.memories_loaded = true;
    }
}

fn make_context(repo: &str, branch: &str) -> String {
    if repo.is_empty() {
        format!("no repo ({branch})")
    } else {
        format!("{repo} ({branch})")
    }
}

const DEFAULT_ISSUE_PROMPT: &str = "Work on issue #{ISSUE} in {REPO}.\n\nBefore writing any code:\n1. Read AGENT.md and SPEC.md in the project root\n2. Read the issue in full: gh issue view {ISSUE}\n3. Identify only the files relevant to the issue\n\nDo the work. Then:\n1. Run vp check — fix any errors before committing\n2. Run vp build — must succeed\n3. Commit: \"fix: <description> (closes #{ISSUE})\"\n\nDo not close the issue. Do not open a PR. Stop after the commit.";

const MEMORY_INDEX_TEMPLATE: &str = "# Memory Index\n\nAgent memory for this project. Read this first, then load only the files relevant to the current task.\n\n> Format: `- [filename.md](filename.md) — one-line description`\n\n<!-- add entries below as memories are created -->\n";

// ── Event handling ────────────────────────────────────────────────────────────

fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> bool {
    if app.text_editor.is_some() {
        return handle_text_editor(app, key, modifiers);
    }
    if app.pending_delete.is_some() {
        return handle_delete_confirm(app, key);
    }
    if matches!(&app.sessions_state, SessionsState::Summary { .. })
        && matches!(&app.screen, Screen::InProject(ProjectTab::Agents))
    {
        return handle_sessions(app, key);
    }
    if key == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    match &app.screen {
        Screen::Projects => handle_projects(app, key),
        Screen::InProject(_) => {
            // Fill/Done, home-edit, and memory-input capture all keys before global nav
            let in_flow = matches!(
                &app.prompt_state,
                PromptState::Fill { .. } | PromptState::Done(_)
            ) || app.home_edit.is_some()
                || app.memory_inputting
                || app.prompt_inputting;
            if !in_flow {
                match key {
                    KeyCode::Esc => {
                        app.screen = Screen::Projects;
                        return false;
                    }
                    KeyCode::Char('q') => return true,
                    KeyCode::Tab => {
                        let next = match &app.screen {
                            Screen::InProject(ProjectTab::Home) => ProjectTab::Config,
                            Screen::InProject(ProjectTab::Config) => ProjectTab::Issues,
                            Screen::InProject(ProjectTab::Issues) => ProjectTab::Prompts,
                            Screen::InProject(ProjectTab::Prompts) => ProjectTab::Memories,
                            Screen::InProject(ProjectTab::Memories) => ProjectTab::Agents,
                            Screen::InProject(ProjectTab::Agents) => ProjectTab::Pane,
                            Screen::InProject(ProjectTab::Pane) => ProjectTab::Home,
                            _ => ProjectTab::Home,
                        };
                        app.set_project_tab(next);
                        return false;
                    }
                    KeyCode::Char('1') => {
                        app.set_project_tab(ProjectTab::Home);
                        return false;
                    }
                    KeyCode::Char('2') => {
                        app.set_project_tab(ProjectTab::Config);
                        return false;
                    }
                    KeyCode::Char('3') => {
                        app.set_project_tab(ProjectTab::Issues);
                        return false;
                    }
                    KeyCode::Char('4') => {
                        app.set_project_tab(ProjectTab::Prompts);
                        return false;
                    }
                    KeyCode::Char('5') => {
                        app.set_project_tab(ProjectTab::Memories);
                        return false;
                    }
                    KeyCode::Char('6') => {
                        app.set_project_tab(ProjectTab::Agents);
                        return false;
                    }
                    KeyCode::Char('7') => {
                        app.set_project_tab(ProjectTab::Pane);
                        return false;
                    }
                    _ => {}
                }
            }
            // Dispatch to sub-screen handler
            let tab = if let Screen::InProject(t) = &app.screen {
                t.clone()
            } else {
                return false;
            };
            match tab {
                ProjectTab::Home => handle_home(app, key),
                ProjectTab::Config => handle_setup(app, key),
                ProjectTab::Issues => handle_issues(app, key),
                ProjectTab::Prompts => handle_prompts(app, key),
                ProjectTab::Memories => handle_memories(app, key),
                ProjectTab::Agents => handle_agents(app, key),
                ProjectTab::Pane => handle_pane(app, key),
            }
        }
    }
}

fn handle_prompts(app: &mut App, key: KeyCode) -> bool {
    if app.prompt_inputting {
        match key {
            KeyCode::Esc => {
                app.prompt_inputting = false;
                app.prompt_input.clear();
            }
            KeyCode::Backspace => {
                app.prompt_input.pop();
            }
            KeyCode::Char(c) => {
                app.prompt_input.push(c);
            }
            KeyCode::Enter => {
                app.prompt_message = Some("Read-only: pemguin does not create or store prompts.".to_string());
                app.prompt_inputting = false;
                app.prompt_input.clear();
            }
            _ => {}
        }
        return false;
    }

    // Subnav: g = global, p = project
    if matches!(&app.prompt_state, PromptState::Browse { .. }) {
        match key {
            KeyCode::Char('g') => {
                app.switch_prompts_view(PromptsView::Global);
                return false;
            }
            KeyCode::Char('p') => {
                app.switch_prompts_view(PromptsView::Project);
                return false;
            }
            _ => {}
        }
    }

    let auto = app.auto_values();
    let fillable_cache: Vec<String> =
        if let PromptState::Fill { prompt_idx, .. } = &app.prompt_state {
            let idx = *prompt_idx;
            app.prompts[idx]
                .placeholders
                .iter()
                .filter(|p| !auto.contains_key(*p))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

    match &mut app.prompt_state {
        PromptState::Browse { list_state } => {
            let len = app.prompt_display_rows.len();
            match key {
                KeyCode::Down | KeyCode::Char('j') => {
                    if len > 0 {
                        let mut n = (list_state.selected().unwrap_or(0) + 1) % len;
                        let mut guard = 0;
                        while matches!(app.prompt_display_rows.get(n), Some(PromptDisplayRow::GroupHeader(_))) && guard < len {
                            n = (n + 1) % len;
                            guard += 1;
                        }
                        list_state.select(Some(n));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if len > 0 {
                        let current = list_state.selected().unwrap_or(0);
                        let mut n = if current == 0 { len - 1 } else { current - 1 };
                        let mut guard = 0;
                        while matches!(app.prompt_display_rows.get(n), Some(PromptDisplayRow::GroupHeader(_))) && guard < len {
                            n = if n == 0 { len - 1 } else { n - 1 };
                            guard += 1;
                        }
                        list_state.select(Some(n));
                    }
                }
                KeyCode::Enter => {
                    if let Some(idx) = list_state.selected() {
                        let prompt = &app.prompts[idx];
                        let fillable: Vec<String> = prompt
                            .placeholders
                            .iter()
                            .filter(|p| !auto.contains_key(*p))
                            .cloned()
                            .collect();
                        if fillable.is_empty() {
                            let filled = fill(&prompt.body, &auto);
                            copy_to_clipboard(&filled);
                            app.prompt_state = PromptState::Done(filled);
                        } else {
                            app.prompt_state = PromptState::Fill {
                                prompt_idx: idx,
                                field_idx: 0,
                                values: auto.clone(),
                                input: String::new(),
                            };
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('e') | KeyCode::Char('d') if app.prompts_view == PromptsView::Project => {
                    app.prompt_message = Some("Read-only: pemguin observes legacy prompts but does not create, edit, or delete them.".to_string());
                }
                KeyCode::Char('r') if app.prompts_view == PromptsView::Project => {
                    app.reload_project_prompts();
                    app.switch_prompts_view(PromptsView::Project);
                    app.prompt_message = None;
                }
                _ => {}
            }
        }
        PromptState::Fill {
            prompt_idx,
            field_idx,
            values,
            input,
        } => {
            let fillable = &fillable_cache;
            match key {
                KeyCode::Esc => {
                    let idx = *prompt_idx;
                    let mut ls = ListState::default();
                    ls.select(Some(idx));
                    app.prompt_state = PromptState::Browse { list_state: ls };
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => {
                    input.push(c);
                }
                KeyCode::Enter => {
                    if *field_idx < fillable.len() {
                        values.insert(fillable[*field_idx].clone(), input.clone());
                        *input = String::new();
                        *field_idx += 1;
                        if *field_idx >= fillable.len() {
                            let v = values.clone();
                            let b = app.prompts[*prompt_idx].body.clone();
                            let filled = fill(&b, &v);
                            copy_to_clipboard(&filled);
                            app.prompt_state = PromptState::Done(filled);
                        }
                    }
                }
                _ => {}
            }
        }
        PromptState::Done(_) => {
            let mut ls = ListState::default();
            ls.select(Some(0));
            app.prompt_state = PromptState::Browse { list_state: ls };
        }
    }
    false
}

fn handle_home(app: &mut App, key: KeyCode) -> bool {
    // Edit mode: capture all keys
    if let Some(field) = app.home_edit.clone() {
        match key {
            KeyCode::Esc => {
                app.home_edit = None;
                app.home_edit_input.clear();
            }
            KeyCode::Backspace => {
                app.home_edit_input.pop();
            }
            KeyCode::Char(c) => {
                app.home_edit_input.push(c);
            }
            KeyCode::Enter => {
                app.home_save_msg = Some("Read-only: pemguin observes GitHub metadata but does not edit it.".to_string());
                app.home_edit = None;
                app.home_edit_input.clear();
            }
            _ => {}
        }
        return false;
    }

    match key {
        KeyCode::Char('i') => {
            app.set_project_tab(ProjectTab::Config);
        }
        KeyCode::Char('c') => {
            app.set_project_tab(ProjectTab::Config);
        }
        KeyCode::Char('r') => {
            if let Some(idx) = app.active_project_idx {
                if let Some(p) = app.projects.get(idx) {
                    let path = p.path.clone();
                    let repo = app.repo.clone();
                    app.start_home_load(&path, &repo);
                    app.home_save_msg = None;
                }
            }
        }
        KeyCode::Char('e') | KeyCode::Char('u') => {
            app.home_save_msg = Some("Read-only: pemguin observes GitHub metadata but does not edit it.".to_string());
        }
        KeyCode::Char('y') => {
            if let Some(data) = &app.home_data {
                if !data.url.is_empty() {
                    copy_to_clipboard(&data.url);
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.home_readme_scroll = app.home_readme_scroll.saturating_add(3);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.home_readme_scroll = app.home_readme_scroll.saturating_sub(3);
        }
        _ => {}
    }
    false
}

fn handle_issues(app: &mut App, key: KeyCode) -> bool {
    let len = app.issues.len();
    match key {
        KeyCode::Down | KeyCode::Char('j') if len > 0 => {
            let n = (app.issue_list_state.selected().unwrap_or(0) + 1) % len;
            app.issue_list_state.select(Some(n));
        }
        KeyCode::Up | KeyCode::Char('k') if len > 0 => {
            let n = app
                .issue_list_state
                .selected()
                .map(|i| if i == 0 { len - 1 } else { i - 1 })
                .unwrap_or(0);
            app.issue_list_state.select(Some(n));
        }
        KeyCode::Enter => {
            if let Some(idx) = app.issue_list_state.selected() {
                let number = app.issues[idx].number.to_string();
                let body = app.issue_prompt_body();
                let mut values = app.auto_values();
                values.insert("ISSUE".to_string(), number);
                let filled = fill(&body, &values);
                copy_to_clipboard(&filled);
                app.screen = Screen::InProject(ProjectTab::Prompts);
                app.prompt_state = PromptState::Done(filled);
            }
        }
        KeyCode::Char('r') => {
            let repo = app.repo.clone();
            app.start_issues_load(&repo);
        }
        _ => {}
    }
    false
}

fn next_item_entry(entries: &[ProjectEntry], from: usize, step: isize) -> usize {
    let len = entries.len();
    let mut i = ((from as isize + step).rem_euclid(len as isize)) as usize;
    for _ in 0..len {
        if matches!(entries[i], ProjectEntry::Item(_)) {
            return i;
        }
        i = ((i as isize + step).rem_euclid(len as isize)) as usize;
    }
    from
}

fn handle_projects(app: &mut App, key: KeyCode) -> bool {
    let elen = app.project_entries.len();
    match key {
        KeyCode::Char('q') => return true, // quit from root
        KeyCode::Down | KeyCode::Char('j') if elen > 0 => {
            let cur = app.project_list_state.selected().unwrap_or(0);
            app.project_list_state
                .select(Some(next_item_entry(&app.project_entries, cur, 1)));
        }
        KeyCode::Up | KeyCode::Char('k') if elen > 0 => {
            let cur = app.project_list_state.selected().unwrap_or(0);
            app.project_list_state
                .select(Some(next_item_entry(&app.project_entries, cur, -1)));
        }
        KeyCode::Enter => {
            if let Some(entry_idx) = app.project_list_state.selected() {
                if let Some(ProjectEntry::Item(proj_idx)) = app.project_entries.get(entry_idx) {
                    let idx = *proj_idx;
                    app.switch_project(idx);
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(entry_idx) = app.project_list_state.selected() {
                if let Some(ProjectEntry::Item(proj_idx)) =
                    app.project_entries.get(entry_idx).cloned()
                {
                    let p = &app.projects[proj_idx];
                    let (path, group) = (p.path.clone(), p.group.clone());
                    if let Some(fresh) = project_info(&path, group) {
                        let repo = fresh.repo.clone();
                        app.projects[proj_idx] = fresh;
                        app.project_entries = build_project_entries(&app.projects);
                        // gh meta refresh (blocking but only one repo)
                        match refresh_project_meta(&repo) {
                            Some(meta) => {
                                app.meta_cache.insert(repo.clone(), meta);
                                save_meta_cache(&app.meta_cache);
                                app.projects_msg = Some(format!("{repo} refreshed"));
                            }
                            None => {
                                app.projects_msg =
                                    Some(format!("{repo} — git refreshed (gh meta unavailable)"));
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Char('R') => {
            app.start_projects_scan(false);
        }
        _ => {}
    }
    false
}

fn fill(body: &str, values: &HashMap<String, String>) -> String {
    let mut r = body.to_string();
    for (k, v) in values {
        r = r.replace(&format!("{{{k}}}"), v);
    }
    r
}

fn copy_to_clipboard(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text);
    }
}

fn centered_rect(width: u16, height: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(width.min(area.width)),
            Constraint::Fill(1),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn execute_delete(app: &mut App, confirm: DeleteConfirm) {
    match confirm.target {
        DeleteTarget::Prompt { .. } => {
            app.prompt_message = Some("Read-only: pemguin does not delete prompts.".to_string());
        }
        DeleteTarget::Memory { .. } => {
            app.memory_message = Some("Read-only: pemguin does not delete memories.".to_string());
        }
        DeleteTarget::Setup { .. } => {
            app.setup_message = Some("Read-only: pemguin does not delete context files.".to_string());
        }
    }
}

fn handle_delete_confirm(app: &mut App, key: KeyCode) -> bool {
    match key {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.pending_delete = None;
        }
        KeyCode::Enter | KeyCode::Char('y') => {
            if let Some(confirm) = app.pending_delete.take() {
                execute_delete(app, confirm);
            }
        }
        _ => {}
    }
    false
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &App) {
    if let Some(editor) = &app.text_editor {
        draw_text_editor(frame, editor);
        return;
    }
    match (&app.screen, &app.prompt_state) {
        (
            _,
            PromptState::Fill {
                prompt_idx,
                field_idx,
                values,
                input,
            },
        ) => draw_fill(frame, app, *prompt_idx, *field_idx, values, input),
        (_, PromptState::Done(text)) => draw_done(frame, text),
        (Screen::Projects, _) => draw_projects(frame, app),
        (Screen::InProject(ProjectTab::Home), _) => draw_home(frame, app),
        (Screen::InProject(ProjectTab::Config), _) => draw_setup(frame, app),
        (Screen::InProject(ProjectTab::Issues), _) => draw_issues(frame, app),
        (Screen::InProject(ProjectTab::Prompts), _) => draw_prompts(frame, app),
        (Screen::InProject(ProjectTab::Memories), _) => draw_memories(frame, app),
        (Screen::InProject(ProjectTab::Agents), _) if app.skills_browse => draw_skills(frame, app),
        (Screen::InProject(ProjectTab::Agents), _) if !matches!(&app.sessions_state, SessionsState::List) => draw_sessions(frame, app),
        (Screen::InProject(ProjectTab::Agents), _) => draw_agents(frame, app),
        (Screen::InProject(ProjectTab::Pane), _) => draw_pane(frame, app),
    }
    if let Some(confirm) = &app.pending_delete {
        draw_delete_confirm(frame, confirm);
    }
}

fn tab_span(icon: &str, label: &str, n: u8, active: bool) -> Vec<Span<'static>> {
    let text = if icon.is_empty() {
        format!(" {n} {label} ")
    } else {
        format!(" {icon} {n} {label} ")
    };
    if active {
        vec![Span::styled(
            text,
            Style::default()
                .fg(theme().sel_fg)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD),
        )]
    } else {
        vec![Span::styled(text, Style::default().fg(theme().fg_dim))]
    }
}

fn header_row(app: &App) -> Line<'static> {
    let badge = Span::styled(
        " 🐧 pm ",
        Style::default()
            .fg(theme().sel_fg)
            .bg(theme().accent)
            .add_modifier(Modifier::BOLD),
    );
    match &app.screen {
        Screen::Projects => Line::from(vec![
            badge,
            Span::raw("  "),
            Span::styled(
                format!(" {I_PROJECTS} projects "),
                Style::default()
                    .fg(theme().sel_fg)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Screen::InProject(_) => {
            let repo_short = app.repo.split('/').last().unwrap_or(&app.repo).to_string();
            let branch = app
                .context
                .split('(')
                .nth(1)
                .unwrap_or("")
                .trim_end_matches(')')
                .to_string();
            let mut spans = vec![
                badge,
                Span::raw("  "),
                Span::styled(
                    repo_short,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if !branch.is_empty() {
                spans.push(Span::styled(
                    format!("  {I_BRANCH} {branch}"),
                    Style::default().fg(theme().fg_dim),
                ));
            }
            Line::from(spans)
        }
    }
}

fn nav_row(app: &App) -> Line<'static> {
    let Screen::InProject(active_tab) = &app.screen else {
        return Line::from("");
    };
    let repo_short = app.repo.split('/').last().unwrap_or(&app.repo).to_string();
    let mut spans: Vec<Span> = Vec::new();
    let tabs: &[(&str, &str, u8, bool)] = &[
        (
            "",
            repo_short.as_str(),
            1u8,
            *active_tab == ProjectTab::Home,
        ),
        (I_SETUP, "config", 2, *active_tab == ProjectTab::Config),
        (I_ISSUES, "issues", 3, *active_tab == ProjectTab::Issues),
        (I_PROMPTS, "prompts", 4, *active_tab == ProjectTab::Prompts),
        (I_MEMORY, "memories", 5, *active_tab == ProjectTab::Memories),
        (I_MCP, "agents", 6, *active_tab == ProjectTab::Agents),
        (I_PANE, "pane", 7, *active_tab == ProjectTab::Pane),
    ];
    for (icon, label, n, active) in tabs {
        spans.extend(tab_span(icon, label, *n, *active));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn draw_home(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let bottom_h = if app.home_edit.is_some() || app.home_save_msg.is_some() {
        3u16
    } else {
        0
    };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(bottom_h),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    let title = format!(" {} ", app.repo);
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(title);

    if let Some(data) = &app.home_data {
        let inner = block.inner(outer[2]);
        frame.render_widget(block, outer[2]);

        // Split content: top (info+commits) / bottom (README)
        let vsplit = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(38), Constraint::Min(0)])
            .split(inner);

        let top_area = vsplit[0];
        let readme_area = vsplit[1];

        // Top area: avatar | left_info | right_commits
        let owner = app.repo.split('/').next().unwrap_or("");
        let avatar_ansi = app.avatar_cache.get(owner);
        let avatar_w = if avatar_ansi.is_some() { 22u16 } else { 0 };

        let top_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(avatar_w),
                Constraint::Percentage(52),
                Constraint::Percentage(48),
            ])
            .split(top_area);

        if let Some(ansi) = avatar_ansi {
            frame.render_widget(Paragraph::new(ansi_to_lines(ansi)), top_split[0]);
        }

        // ── Left info column ──────────────────────────────────────────
        let mut left: Vec<Line> = vec![Line::from("")];

        // Stats row: ★ stars  ⑂ forks  ! issues  ↳ PRs  license
        {
            let mut stat_spans: Vec<Span> = vec![Span::raw("  ")];
            let mut any = false;
            if let Some(s) = data.stars {
                stat_spans.push(Span::styled(format!("★ {s}"), Style::default().fg(theme().yellow)));
                stat_spans.push(Span::raw("   "));
                any = true;
            }
            if let Some(f) = data.forks {
                stat_spans.push(Span::styled(format!("⑂ {f}"), Style::default().fg(theme().fg_dim)));
                stat_spans.push(Span::raw("   "));
                any = true;
            }
            if let Some(n) = app
                .active_project_idx
                .and_then(|i| app.projects.get(i))
                .and_then(|p| app.meta_cache.get(&p.repo))
                .and_then(|m| m.open_issues)
            {
                let color = if n > 0 { theme().red } else { theme().fg_xdim };
                stat_spans.push(Span::styled(format!("! {n}"), Style::default().fg(color)));
                stat_spans.push(Span::raw("   "));
                any = true;
            }
            if let Some(pr) = data.open_prs {
                let color = if pr > 0 { theme().purple } else { theme().fg_xdim };
                stat_spans.push(Span::styled(
                    format!("↳ {pr} PR"),
                    Style::default().fg(color),
                ));
                stat_spans.push(Span::raw("   "));
                any = true;
            }
            if let Some(lic) = &data.license {
                stat_spans.push(Span::styled(lic.clone(), Style::default().fg(theme().fg_dim)));
                any = true;
            } else if app.home_loading {
                stat_spans.push(Span::styled("loading…", Style::default().fg(theme().fg_xdim)));
                any = true;
            }
            if any {
                left.push(Line::from(stat_spans));
                left.push(Line::from(""));
            }
        }

        // URL
        if !data.url.is_empty() {
            left.push(Line::from(vec![
                Span::styled("  url      ", Style::default().fg(theme().fg_dim)),
                Span::styled(data.url.clone(), Style::default().fg(theme().purple)),
                Span::styled("  y copy", Style::default().fg(theme().fg_xdim)),
            ]));
        }

        // Setup status
        left.push(Line::from(""));
        {
            left.push(Line::from(vec![
                Span::styled("  req      ", Style::default().fg(theme().fg_dim)),
                Span::styled(
                    format!("{}/{} context files present", data.recommended_ok, data.recommended_total),
                    Style::default().fg(if data.recommended_ok == data.recommended_total {
                        theme().green
                    } else {
                        theme().yellow
                    }),
                ),
            ]));
            left.push(Line::from(vec![
                Span::styled("  mcp      ", Style::default().fg(theme().fg_dim)),
                Span::styled(
                    if data.mcp_ready { "configured" } else { "not configured" },
                    Style::default().fg(if data.mcp_ready { theme().green } else { theme().yellow }),
                ),
            ]));
            left.push(Line::from(vec![
                Span::styled("  agents   ", Style::default().fg(theme().fg_dim)),
                Span::styled(
                    format!("{} skills  {} sessions", data.skills_count, data.sessions_count),
                    Style::default().fg(theme().purple),
                ),
            ]));
        }

        // Description
        left.push(Line::from(""));
        if let Some(desc) = &data.gh_description {
            left.push(Line::from(vec![
                Span::styled("  desc     ", Style::default().fg(theme().fg_dim)),
                Span::styled(desc.clone(), Style::default().fg(theme().fg_dim)),
            ]));
        } else if app.home_loading {
            left.push(Line::from(vec![
                Span::styled("  desc     ", Style::default().fg(theme().fg_dim)),
                Span::styled("loading...", Style::default().fg(theme().fg_xdim)),
            ]));
        } else {
            left.push(Line::from(vec![
                Span::styled("  desc     ", Style::default().fg(theme().fg_dim)),
                Span::styled("not set", Style::default().fg(theme().fg_xdim)),
            ]));
        }

        // Homepage
        if let Some(home) = &data.homepage {
            left.push(Line::from(vec![
                Span::styled("  homepage ", Style::default().fg(theme().fg_dim)),
                Span::styled(home.clone(), Style::default().fg(theme().purple)),
            ]));
        } else if app.home_loading {
            left.push(Line::from(vec![
                Span::styled("  homepage ", Style::default().fg(theme().fg_dim)),
                Span::styled("loading...", Style::default().fg(theme().fg_xdim)),
            ]));
        }

        // Stack
        if let Some(stack) = &data.stack {
            left.push(Line::from(""));
            left.push(Line::from(vec![
                Span::styled("  stack    ", Style::default().fg(theme().fg_dim)),
                Span::styled(stack.clone(), Style::default().fg(theme().purple)),
            ]));
        }

        // Topics
        if let Some(meta) = app
            .active_project_idx
            .and_then(|i| app.projects.get(i))
            .and_then(|p| app.meta_cache.get(&p.repo))
        {
            if !meta.topics.is_empty() {
                left.push(Line::from(""));
                let mut spans = vec![Span::styled("  topics   ", Style::default().fg(theme().fg_dim))];
                for (i, t) in meta.topics.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw("  "));
                    }
                    spans.push(Span::styled(t.clone(), Style::default().fg(theme().fg_dim)));
                }
                left.push(Line::from(spans));
            }
        }

        // Dirty files
        if !data.dirty_files.is_empty() {
            left.push(Line::from(""));
            left.push(Line::from(Span::styled(
                "  unstaged ",
                Style::default().fg(theme().fg_dim),
            )));
            for f in data.dirty_files.iter().take(6) {
                left.push(Line::from(Span::styled(
                    format!("    {f}"),
                    Style::default().fg(theme().yellow),
                )));
            }
            if data.dirty_files.len() > 6 {
                left.push(Line::from(Span::styled(
                    format!("    +{} more", data.dirty_files.len() - 6),
                    Style::default().fg(theme().fg_xdim),
                )));
            }
        }

        frame.render_widget(
            Paragraph::new(left).wrap(Wrap { trim: false }),
            top_split[1],
        );

        // ── Right: recent commits ─────────────────────────────────────
        let mut right: Vec<Line> = vec![
            Line::from(Span::styled(
                format!("  {I_COMMIT} recent commits"),
                Style::default().fg(theme().fg_xdim),
            )),
            Line::from(""),
        ];
        if data.recent_commits.is_empty() {
            right.push(Line::from(Span::styled(
                "  no commits",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let mut current_day: Option<&str> = None;
            for commit in &data.recent_commits {
                if current_day != Some(commit.date_label.as_str()) {
                    if current_day.is_some() {
                        right.push(Line::from(""));
                    }
                    right.push(Line::from(Span::styled(
                        format!("  {}", commit.date_label),
                        Style::default().fg(theme().purple).add_modifier(Modifier::BOLD),
                    )));
                    current_day = Some(commit.date_label.as_str());
                }
                right.push(Line::from(vec![
                    Span::styled(
                        format!("    {}  ", commit.time_label),
                        Style::default().fg(theme().fg_xdim),
                    ),
                    Span::styled(
                        format!("{I_COMMIT} {}  ", commit.hash),
                        Style::default().fg(theme().fg_dim),
                    ),
                    Span::raw(commit.subject.clone()),
                ]));
            }
        }
        frame.render_widget(
            Paragraph::new(right).wrap(Wrap { trim: false }),
            top_split[2],
        );

        // ── README section ────────────────────────────────────────────
        let readme_block = Block::default()
            .borders(Borders::TOP)
            .title(Span::styled(" README ", Style::default().fg(theme().fg_dim)));
        let readme_inner = readme_block.inner(readme_area);
        frame.render_widget(readme_block, readme_area);

        if let Some(readme) = &data.readme {
            // Light markdown rendering: color headings, dim code fences
            let lines: Vec<Line> = readme
                .lines()
                .map(|l| {
                    if l.starts_with("# ") || l.starts_with("## ") || l.starts_with("### ") {
                        Line::from(Span::styled(
                            l.to_string(),
                            Style::default()
                                .fg(theme().accent)
                                .add_modifier(Modifier::BOLD),
                        ))
                    } else if l.starts_with("```") || l.starts_with("~~~") {
                        Line::from(Span::styled(l.to_string(), Style::default().fg(theme().fg_xdim)))
                    } else {
                        Line::from(Span::raw(l.to_string()))
                    }
                })
                .collect();
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((app.home_readme_scroll, 0)),
                readme_inner,
            );
        } else if app.home_loading {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "  loading README…",
                    Style::default().fg(theme().fg_xdim),
                )),
                readme_inner,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "  No README found",
                    Style::default().fg(theme().fg_xdim),
                )),
                readme_inner,
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Loading…",
                Style::default().fg(Color::DarkGray),
            ))
            .block(block),
            outer[2],
        );
    }

    // Edit input or save message — keep exactly as before
    if let Some(field) = &app.home_edit {
        let label = match field {
            HomeEditField::Description => "description",
            HomeEditField::Homepage => "homepage url",
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("> ", Style::default().fg(theme().accent)),
                Span::raw(app.home_edit_input.clone()),
                Span::styled("█", Style::default().fg(theme().accent)),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(format!(" edit {label} ")),
            ),
            outer[3],
        );
    } else if let Some(msg) = &app.home_save_msg {
        let (icon, color) = if msg.starts_with("Error") {
            (I_CROSS, theme().red)
        } else {
            (I_CHECK, theme().green)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("  {icon}  {msg}"),
                Style::default().fg(color),
            ))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
            outer[3],
        );
    }

    let not_initialized = false;
    let footer_hints = if app.home_edit.is_some() {
        footer(&[("enter", "save"), ("esc", "cancel")])
    } else if not_initialized {
        footer(&[
            ("i", "open config"),
            ("c", "config"),
            ("r", "reload"),
            ("e", "edit desc"),
            ("y", "copy url"),
            ("esc", "back"),
        ])
    } else {
        footer(&[
            ("c", "config"),
            ("r", "reload"),
            ("e", "edit desc"),
            ("u", "edit homepage"),
            ("y", "copy url"),
            ("jk", "scroll readme"),
            ("esc", "back"),
        ])
    };
    frame.render_widget(Paragraph::new(footer_hints), outer[4]);
}

fn footer(hints: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    for (key, desc) in hints {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default().fg(theme().accent),
        ));
        spans.push(Span::styled(
            format!("{desc}  "),
            Style::default().fg(theme().fg_dim),
        ));
    }
    Line::from(spans)
}

fn hl() -> Style {
    Style::default()
        .fg(theme().sel_fg)
        .bg(theme().accent)
        .add_modifier(Modifier::BOLD)
}

fn draw_prompts(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let bottom_h = if app.prompt_inputting || app.prompt_message.is_some() {
        3
    } else {
        1
    };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(bottom_h),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    // Split left pane into subnav + list
    let left_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(outer[2]);

    let list_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(left_area[0]);

    // Subnav
    let global_active = app.prompts_view == PromptsView::Global;
    let project_active = app.prompts_view == PromptsView::Project;
    let has_project = !app.project_prompts.is_empty();
    let subnav = Line::from(vec![
        Span::styled(
            " g global ",
            if global_active {
                Style::default()
                    .fg(theme().sel_fg)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme().fg_dim)
            },
        ),
        Span::raw(" "),
        Span::styled(
            " p project ",
            if project_active {
                Style::default()
                    .fg(theme().sel_fg)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if has_project {
                Style::default().fg(theme().fg_dim)
            } else {
                Style::default().fg(theme().fg_xdim)
            },
        ),
    ]);
    frame.render_widget(Paragraph::new(subnav), list_split[0]);

    let items: Vec<ListItem> = app.prompt_display_rows.iter().map(|row| match row {
        PromptDisplayRow::GroupHeader(name) => ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {name}"),
                Style::default().fg(theme().fg_dim).add_modifier(Modifier::BOLD),
            ),
        ])),
        PromptDisplayRow::Item(i) => ListItem::new(app.prompts[*i].name.clone()),
    }).collect();
    let empty_hint = if !project_active {
        String::new()
    } else {
        " no legacy .pemguin/prompts/ observed ".to_string()
    };
    let block_title = if items.is_empty() && project_active {
        empty_hint.as_str()
    } else {
        " prompts "
    };
    let mut ls = if let PromptState::Browse { list_state } = &app.prompt_state {
        list_state.clone()
    } else {
        ListState::default()
    };
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(block_title))
            .highlight_style(hl())
            .highlight_symbol("> "),
        list_split[1],
        &mut ls,
    );

    let preview = app
        .selected_prompt_idx()
        .and_then(|i| app.prompts.get(i))
        .map(|p| p.preview.as_str())
        .unwrap_or("");
    frame.render_widget(
        Paragraph::new(preview)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" preview "))
            .wrap(Wrap { trim: false }),
        left_area[1],
    );
    if app.prompt_inputting {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("> ", Style::default().fg(theme().accent)),
                Span::raw(app.prompt_input.clone()),
                Span::styled("█", Style::default().fg(theme().accent)),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(" new prompt name "),
            ),
            outer[3],
        );
    } else if let Some(msg) = &app.prompt_message {
        let color = if msg.starts_with("Error:") {
            theme().red
        } else {
            theme().green
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("  {msg}"), Style::default().fg(color)))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
            outer[3],
        );
    } else {
        frame.render_widget(
            Paragraph::new(footer(&[
                ("g", "global"),
                ("p", "project"),
                ("↑↓/jk", "navigate"),
                ("enter", "copy/fill"),
                ("n", "new"),
                ("e", "edit"),
                ("d", "delete"),
                ("r", "reload"),
                ("esc", "back"),
            ])),
            outer[3],
        );
    }
}

fn draw_issues(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    if let Some(err) = &app.issues_error {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {I_CROSS}  {err}"),
                    Style::default().fg(theme().red),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" issues ")),
            outer[2],
        );
    } else if app.issues_loading {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Loading issues...",
                    Style::default().fg(theme().fg_xdim),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" issues ")),
            outer[2],
        );
    } else if app.issues.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No open issues.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" issues ")),
            outer[2],
        );
    } else {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(outer[2]);

        let items: Vec<ListItem> = app
            .issues
            .iter()
            .map(|issue| {
                let lbl = if issue.labels.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", issue.labels.join(", "))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("#{:<5}", issue.number), Style::default().fg(theme().fg_dim)),
                    Span::raw(issue.title.clone()),
                    Span::styled(lbl, Style::default().fg(theme().fg_dim)),
                ]))
            })
            .collect();

        let mut ls = app.issue_list_state.clone();
        frame.render_stateful_widget(
            List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL).border_type(BorderType::Rounded)
                        .title(" open issues "),
                )
                .highlight_style(hl())
                .highlight_symbol("> "),
            main[0],
            &mut ls,
        );

        let preview: Vec<Line> = app
            .issue_list_state
            .selected()
            .and_then(|i| app.issues.get(i))
            .map(|issue| {
                let mut lines = vec![
                    Line::from(Span::styled(
                        format!("{I_ISSUES} #{} — {}", issue.number, issue.title),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                ];
                if !issue.labels.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("Labels: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(issue.labels.join(", ")),
                    ]));
                    lines.push(Line::from(""));
                }
                for l in issue.body.lines() {
                    lines.push(Line::from(l.to_string()));
                }
                lines
            })
            .unwrap_or_default();

        frame.render_widget(
            Paragraph::new(preview)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" body "))
                .wrap(Wrap { trim: false }),
            main[1],
        );
    }
    frame.render_widget(
        Paragraph::new(footer(&[
            ("↑↓/jk", "navigate"),
            ("enter", "copy prompt"),
            ("r", "refresh"),
            ("esc", "back"),
            ("tab", "switch"),
            ("q", "quit"),
        ])),
        outer[3],
    );
}

fn draw_projects(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);

    if app.projects_loading && app.projects.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Scanning projects...",
                    Style::default().fg(theme().fg_dim),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  The UI stays live while the scan runs.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" projects ")),
            outer[1],
        );
    } else if app.projects.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No projects found.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Set $PEMGUIN_PROJECTS_DIR or place projects under ~/Projects/",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" projects ")),
            outer[1],
        );
    } else {
        // Build table rows from project entries (group headers + project items)
        let rows: Vec<Row> = app
            .project_entries
            .iter()
            .map(|entry| match entry {
                ProjectEntry::Group(name) => Row::new(vec![
                    Cell::from(Span::styled(
                        format!("  {name}"),
                        Style::default().fg(theme().purple).add_modifier(Modifier::BOLD),
                    )),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                ]),
                ProjectEntry::Item(proj_idx) => {
                    let p = &app.projects[*proj_idx];
                    let meta = app.meta_cache.get(&p.repo);
                    let active = app.active_project_idx == Some(*proj_idx);

                    // repo — active project shows bullet, others show folder icon
                    let repo_display = p.repo.split('/').last().unwrap_or(&p.repo);
                    let repo_text = if active {
                        format!("{I_BULLET} {repo_display}")
                    } else {
                        format!("{I_PROJECTS} {repo_display}")
                    };
                    let repo_cell = Cell::from(Span::styled(
                        repo_text,
                        if active {
                            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ));

                    // lang
                    let lang = meta
                        .and_then(|m| m.language.as_deref())
                        .map(lang_short)
                        .unwrap_or("");
                    let lang_cell =
                        Cell::from(Span::styled(lang.to_string(), Style::default().fg(theme().purple)));

                    // branch
                    let branch_display = if p.branch.len() > 10 {
                        format!("{}~", &p.branch[..9])
                    } else {
                        p.branch.clone()
                    };
                    let branch_cell = Cell::from(Span::styled(
                        format!("{I_BRANCH} {branch_display}"),
                        Style::default().fg(theme().fg_dim),
                    ));

                    // changes — multi-colored spans in one cell
                    let mut change_spans: Vec<Span> = vec![];
                    if p.dirty_count > 0 {
                        change_spans.push(Span::styled(
                            format!("●{}", p.dirty_count),
                            Style::default().fg(theme().yellow),
                        ));
                    }
                    if p.commits_ahead > 0 {
                        if !change_spans.is_empty() {
                            change_spans.push(Span::raw(" "));
                        }
                        change_spans.push(Span::styled(
                            format!("↑{}", p.commits_ahead),
                            Style::default().fg(theme().purple),
                        ));
                    }
                    if p.commits_behind > 0 {
                        if !change_spans.is_empty() {
                            change_spans.push(Span::raw(" "));
                        }
                        change_spans.push(Span::styled(
                            format!("↓{}", p.commits_behind),
                            Style::default().fg(theme().red),
                        ));
                    }
                    let changes_cell = Cell::from(Line::from(change_spans));

                    // ready
                    let ready_text = if p.template_count > 0 {
                        format!("t{}", p.template_count)
                    } else {
                        format!(
                            "{}/{}",
                            p.recommended_ok, p.recommended_total
                        )
                    };
                    let ready_style = if p.template_count > 0 {
                        Style::default().fg(theme().yellow)
                    } else if p.recommended_ok == p.recommended_total {
                        Style::default().fg(theme().green)
                    } else {
                        Style::default().fg(theme().yellow)
                    };
                    let pm_cell = Cell::from(Span::styled(ready_text, ready_style));

                    // iss
                    let iss_cell = match meta.and_then(|m| m.open_issues) {
                        Some(n) if n > 0 => Cell::from(Span::styled(
                            format!("{n}"),
                            Style::default().fg(theme().red),
                        )),
                        _ => Cell::from(Span::styled("0", Style::default().fg(theme().fg_xdim))),
                    };

                    // pushed
                    let pushed = meta
                        .and_then(|m| m.pushed_at.as_deref())
                        .map(relative_date)
                        .unwrap_or_default();
                    let pushed_cell =
                        Cell::from(Span::styled(pushed, Style::default().fg(theme().fg_xdim)));

                    Row::new(vec![
                        repo_cell,
                        lang_cell,
                        branch_cell,
                        changes_cell,
                        pm_cell,
                        iss_cell,
                        pushed_cell,
                    ])
                }
            })
            .collect();

        let header = Row::new(vec![
            Cell::from("repo"),
            Cell::from("lang"),
            Cell::from("branch"),
            Cell::from("changes"),
            Cell::from("ready"),
            Cell::from("iss"),
            Cell::from("pushed"),
        ])
        .style(Style::default().fg(theme().fg_xdim))
        .bottom_margin(0);

        let widths = [
            Constraint::Min(16),    // repo — takes remaining space
            Constraint::Length(4),  // lang
            Constraint::Length(12), // branch (icon + space + name)
            Constraint::Length(11), // changes
            Constraint::Length(7),  // ready
            Constraint::Length(4),  // iss
            Constraint::Length(5),  // pushed
        ];

        let mut ts = app.project_list_state.clone();
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .header(header)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" projects "))
                .row_highlight_style(hl())
                .highlight_symbol("> ")
                .column_spacing(2),
            outer[1],
            &mut ts,
        );
    }

    let footer_line = if let Some(msg) = &app.projects_msg {
        let mut spans = footer(&[
            ("↑↓/jk", "navigate"),
            ("enter", "open"),
            ("r", "refresh"),
            ("R", "rescan all"),
            ("q", "quit"),
        ])
        .spans;
        spans.push(Span::styled(
            format!("  {msg}"),
            Style::default().fg(theme().fg_dim),
        ));
        Line::from(spans)
    } else if app.projects_loading {
        let mut spans = footer(&[
            ("↑↓/jk", "navigate"),
            ("enter", "open"),
            ("r", "refresh"),
            ("R", "rescan all"),
            ("q", "quit"),
        ])
        .spans;
        spans.push(Span::styled(
            "  scanning projects...",
            Style::default().fg(theme().fg_dim),
        ));
        Line::from(spans)
    } else {
        footer(&[
            ("↑↓/jk", "navigate"),
            ("enter", "open"),
            ("r", "refresh"),
            ("R", "rescan all"),
            ("q", "quit"),
        ])
    };
    frame.render_widget(Paragraph::new(footer_line), outer[2]);
}

fn handle_memories(app: &mut App, key: KeyCode) -> bool {
    // Name-input mode (creating a new memory file)
    if app.memory_inputting {
        match key {
            KeyCode::Esc => {
                app.memory_inputting = false;
                app.memory_input.clear();
            }
            KeyCode::Backspace => {
                app.memory_input.pop();
            }
            KeyCode::Char(c) => {
                app.memory_input.push(c);
            }
            KeyCode::Enter => {
                app.memory_message = Some("Read-only: pemguin observes memories but does not create them.".to_string());
                app.memory_inputting = false;
                app.memory_input.clear();
            }
            _ => {}
        }
        return false;
    }

    match key {
        KeyCode::Char('c') => {
            app.switch_memories_view(MemoriesView::Claude);
        }
        KeyCode::Char('x') => {
            app.switch_memories_view(MemoriesView::Codex);
        }
        KeyCode::Char('g') => {
            app.switch_memories_view(MemoriesView::Gemini);
        }
        KeyCode::Down | KeyCode::Char('j') if !app.memory_files.is_empty() => {
            let n = (app.memory_list_state.selected().unwrap_or(0) + 1) % app.memory_files.len();
            app.memory_list_state.select(Some(n));
        }
        KeyCode::Up | KeyCode::Char('k') if !app.memory_files.is_empty() => {
            let len = app.memory_files.len();
            let n = app
                .memory_list_state
                .selected()
                .map(|i| if i == 0 { len - 1 } else { i - 1 })
                .unwrap_or(0);
            app.memory_list_state.select(Some(n));
        }
        KeyCode::Enter | KeyCode::Char('y') => {
            if let Some(idx) = app.memory_list_state.selected() {
                if let Some(f) = app.memory_files.get(idx) {
                    let text = f.path.to_string_lossy().to_string();
                    match Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
                        Ok(_) => app.memory_message = Some(format!("Copied path: {text}")),
                        Err(e) => app.memory_message = Some(format!("Error: clipboard: {e}")),
                    }
                }
            }
        }
        KeyCode::Char('e') | KeyCode::Char('n') | KeyCode::Char('d') => {
            app.memory_message = Some("Read-only: pemguin observes memories but does not edit, create, or delete them.".to_string());
        }
        KeyCode::Char('r') => {
            app.reload_memories();
        }
        _ => {}
    }
    false
}

fn handle_setup(app: &mut App, key: KeyCode) -> bool {
    match key {
        KeyCode::Down | KeyCode::Char('j') if !app.setup_items.is_empty() => {
            let rows = setup_render_rows(&app.setup_items);
            let len = rows.len();
            let cur = app.setup_list_state.selected().unwrap_or(0);
            let mut next = (cur + 1) % len;
            while matches!(rows[next], SetupRenderRow::GroupHeader(_)) {
                next = (next + 1) % len;
            }
            app.setup_list_state.select(Some(next));
        }
        KeyCode::Up | KeyCode::Char('k') if !app.setup_items.is_empty() => {
            let rows = setup_render_rows(&app.setup_items);
            let len = rows.len();
            let cur = app.setup_list_state.selected().unwrap_or(0);
            let mut prev = if cur == 0 { len - 1 } else { cur - 1 };
            while matches!(rows[prev], SetupRenderRow::GroupHeader(_)) {
                prev = if prev == 0 { len - 1 } else { prev - 1 };
            }
            app.setup_list_state.select(Some(prev));
        }
        KeyCode::Enter | KeyCode::Char('y') => {
            if let Some(item_idx) = selected_setup_item(&app.setup_items, &app.setup_list_state) {
                if let Some(idx) = app.active_project_idx {
                    if let Some(p) = app.projects.get(idx) {
                        let item = app.setup_items[item_idx].clone();
                        if let Some(path) = setup_item_edit_path(&p.path, &item) {
                            let text = path.to_string_lossy().to_string();
                            match Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
                                Ok(_) => app.setup_message = Some(format!("Copied path: {text}")),
                                Err(e) => app.setup_message = Some(format!("Error: clipboard: {e}")),
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Char('e') | KeyCode::Char('d') | KeyCode::Char('R') | KeyCode::Char('a') | KeyCode::Char('g') => {
            app.setup_message = Some("Read-only: pemguin observes context files but does not edit, create, delete, reset, or repair them.".to_string());
        }
        KeyCode::Char('r') => {
            app.refresh_setup();
        }
        _ => {}
    }
    false
}

fn draw_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    if app.active_project_idx.is_none() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No project selected.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Go to [3 projects] and press enter to set the active project.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" context ")),
            outer[2],
        );
    } else if app.setup_items.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Scanning…",
                Style::default().fg(Color::DarkGray),
            ))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" context ")),
            outer[2],
        );
    } else {
        let prompt_h = if app.setup_on_open { 4u16 } else { 0 };
        let msg_h = if app.setup_message.is_some() { 3u16 } else { 0 };
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(prompt_h),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(msg_h),
            ])
            .split(outer[2]);

        if app.setup_on_open {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "  This project has missing observed context files.",
                        Style::default().fg(theme().yellow).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        "  Read-only: create or repair files outside pemguin if desired.",
                        Style::default().fg(theme().fg_dim),
                    )),
                ])
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" initialize ")),
                inner[0],
            );
        }

        // Column headers
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("        "), // border(1) + highlight(2) + icon(5)
                Span::styled(format!("{:<26}", "item"), Style::default().fg(theme().fg_dim).add_modifier(Modifier::BOLD)),
                Span::styled("git  ", Style::default().fg(theme().fg_dim).add_modifier(Modifier::BOLD)),
                Span::styled("detail", Style::default().fg(theme().fg_dim).add_modifier(Modifier::BOLD)),
            ])),
            inner[1],
        );

        let render_rows = setup_render_rows(&app.setup_items);
        let items: Vec<ListItem> = render_rows
            .iter()
            .map(|row| match row {
                SetupRenderRow::GroupHeader(label) => ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {label}"),
                        Style::default().fg(theme().fg_dim).add_modifier(Modifier::BOLD),
                    ),
                ])),
                SetupRenderRow::Item(i) => {
                    let item = &app.setup_items[*i];
                    let (icon, icon_style) = match item.status {
                        SetupStatus::Ok       => (I_CHECK, Style::default().fg(theme().green)),
                        SetupStatus::Missing  => (I_CROSS, Style::default().fg(theme().red)),
                        SetupStatus::Stale    => (I_WARN,  Style::default().fg(theme().yellow)),
                        SetupStatus::Template => (I_WARN,  Style::default().fg(theme().yellow)),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {icon}  "), icon_style),
                        Span::styled(format!("{:<26}", item.label), Style::default()),
                        if item.gitignore_path.is_some() {
                            if item.gitignored {
                                Span::styled("↓  ", Style::default().fg(theme().green).add_modifier(Modifier::DIM))
                            } else {
                                Span::styled("○  ", Style::default().fg(theme().fg_xdim))
                            }
                        } else {
                            Span::raw("   ")
                        },
                        Span::styled(item.detail.clone(), Style::default().fg(theme().fg_dim)),
                    ]))
                }
            })
            .collect();

        let mut ls = app.setup_list_state.clone();
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" observed context "))
                .highlight_style(hl())
                .highlight_symbol("> "),
            inner[2],
            &mut ls,
        );

        if let Some(msg) = &app.setup_message {
            let color = if msg.starts_with("Error:") {
                theme().red
            } else {
                theme().green
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(
                        "  {} {msg}",
                        if msg.starts_with("Error:") {
                            I_CROSS
                        } else {
                            I_CHECK
                        }
                    ),
                    Style::default().fg(color),
                ))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
                inner[3],
            );
        }
    }

    let footer_line = if app.setup_on_open {
        footer(&[
            ("↑↓/jk", "navigate"),
            ("enter/y", "copy path"),
            ("r", "rescan"),
            ("esc", "back"),
        ])
    } else {
        footer(&[
            ("↑↓/jk", "navigate"),
            ("enter/y", "copy path"),
            ("r", "rescan"),
            ("esc", "back"),
        ])
    };
    frame.render_widget(Paragraph::new(footer_line), outer[3]);
}

fn draw_memories(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let input_h = if app.memory_inputting || app.memory_message.is_some() {
        3u16
    } else {
        0
    };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    // Main area: list (left) + preview (right)
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(outer[2]);

    // Left: subnav + file list
    let left_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(main[0]);

    let c_active = app.memories_view == MemoriesView::Claude;
    let x_active = app.memories_view == MemoriesView::Codex;
    let g_active = app.memories_view == MemoriesView::Gemini;
    let tab_style = |active: bool| if active {
        Style::default().fg(theme().sel_fg).bg(theme().accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().fg_dim)
    };
    let subnav = Line::from(vec![
        Span::styled(" c claude ", tab_style(c_active)),
        Span::raw(" "),
        Span::styled(" x codex ", tab_style(x_active)),
        Span::raw(" "),
        Span::styled(" g gemini ", tab_style(g_active)),
    ]);
    frame.render_widget(Paragraph::new(subnav), left_split[0]);

    let dir_label = match app.memories_view {
        MemoriesView::Claude  => " .claude/…/memory/ ",
        MemoriesView::Codex   => " .codex/memories/… ",
        MemoriesView::Gemini  => " ~/.gemini/GEMINI.md ",
    };
    let items: Vec<ListItem> = if app.memory_files.is_empty() {
        vec![ListItem::new(Span::styled(
            "  (empty)",
            Style::default().fg(theme().fg_xdim),
        ))]
    } else {
        app.memory_files
            .iter()
            .map(|f| {
                let mut spans = Vec::new();
                if let Some(origin) = f.origin {
                    spans.push(Span::styled(
                        format!("[{}] ", origin.label()),
                        Style::default().fg(theme().purple),
                    ));
                } else {
                    spans.push(Span::styled(
                        "[local] ".to_string(),
                        Style::default().fg(theme().fg_dim),
                    ));
                }
                spans.push(Span::raw(f.name.clone()));
                ListItem::new(Line::from(spans))
            })
            .collect()
    };
    let mut ls = app.memory_list_state.clone();
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(dir_label))
            .highlight_style(hl())
            .highlight_symbol("> "),
        left_split[1],
        &mut ls,
    );

    // Right: preview
    let preview = app
        .memory_list_state
        .selected()
        .and_then(|i| app.memory_files.get(i))
        .map(|f| f.content.as_str())
        .unwrap_or("");
    let preview_title = app
        .memory_list_state
        .selected()
        .and_then(|i| app.memory_files.get(i))
        .map(|f| match f.origin {
            Some(origin) => format!(" preview [{}] ", origin.label()),
            None => " preview [local] ".to_string(),
        })
        .unwrap_or_else(|| " preview ".to_string());
    frame.render_widget(
        Paragraph::new(preview)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(preview_title))
            .wrap(Wrap { trim: false }),
        main[1],
    );

    // Input or message
    if app.memory_inputting {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("> ", Style::default().fg(theme().accent)),
                Span::raw(app.memory_input.clone()),
                Span::styled("█", Style::default().fg(theme().accent)),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(" new memory name "),
            ),
            outer[3],
        );
    } else if let Some(msg) = &app.memory_message {
        let (icon, color) = if msg.starts_with("Error") {
            (I_CROSS, theme().red)
        } else {
            (I_CHECK, theme().green)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("  {icon}  {msg}"),
                Style::default().fg(color),
            ))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
            outer[3],
        );
    }

    let footer_hints = if app.memory_inputting {
        footer(&[("enter", "read-only notice"), ("esc", "cancel")])
    } else if c_active {
        footer(&[
            ("↑↓/jk", "navigate"),
            ("enter/y", "copy path"),
            ("r", "reload"),
            ("esc", "back"),
            ("p/g/c", "view"),
        ])
    } else {
        footer(&[
            ("↑↓/jk", "navigate"),
            ("enter/y", "copy path"),
            ("r", "reload"),
            ("esc", "back"),
            ("p/g/c", "view"),
            ("tab", "switch"),
        ])
    };
    frame.render_widget(Paragraph::new(footer_hints), outer[4]);
}

fn agent_section_label(section: AgentSection) -> &'static str {
    match section {
        AgentSection::Mcp => "mcp",
        AgentSection::Skills => "skills",
        AgentSection::Sessions => "sessions",
    }
}

fn agent_section_spans(active: AgentSection) -> Vec<Span<'static>> {
    [AgentSection::Mcp, AgentSection::Skills, AgentSection::Sessions]
        .into_iter()
        .flat_map(|section| {
            let label = agent_section_label(section);
            let span = if section == active {
                Span::styled(
                    format!(" {label} "),
                    Style::default()
                        .fg(theme().sel_fg)
                        .bg(theme().accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(format!(" {label} "), Style::default().fg(theme().fg_dim))
            };
            vec![span, Span::raw(" ")]
        })
        .collect()
}

fn draw_agents_summary(app: &App) -> Vec<Line<'static>> {
    let project = app.active_project_idx.and_then(|i| app.projects.get(i));
    let mcp_ready = project.map(|p| p.mcp_ready).unwrap_or(false);
    let skills_count = app.skills.len();
    let sessions_count = app.sessions.len();

    vec![
        Line::from(vec![
            Span::styled(" status   ", Style::default().fg(theme().fg_dim)),
            Span::styled(
                if mcp_ready { "mcp ready" } else { "mcp missing" },
                Style::default().fg(if mcp_ready { theme().green } else { theme().yellow }),
            ),
            Span::raw("   "),
            Span::styled(
                format!("{skills_count} skills"),
                Style::default().fg(theme().purple),
            ),
            Span::raw("   "),
            Span::styled(
                format!("{sessions_count} sessions"),
                Style::default().fg(theme().purple),
            ),
        ]),
    ]
}

fn draw_agents(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let msg = match app.agent_section {
        AgentSection::Mcp => app.mcp_message.as_ref(),
        AgentSection::Skills => app.skills_install_message.as_ref(),
        AgentSection::Sessions => app.sessions_message.as_ref(),
    };
    let msg_h = if msg.is_some() { 3 } else { 0 };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(msg_h),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);
    frame.render_widget(Paragraph::new(Line::from(agent_section_spans(app.agent_section))), outer[2]);
    frame.render_widget(
        Paragraph::new(draw_agents_summary(app))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" agents overview ")),
        outer[3],
    );

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(outer[4]);

    match app.agent_section {
        AgentSection::Mcp => {
            let items: Vec<ListItem> = if app.mcp_servers.is_empty() {
                vec![ListItem::new("pemguin")]
            } else {
                app.mcp_servers
                    .iter()
                    .map(|s| ListItem::new(s.name.clone()))
                    .collect()
            };
            let mut ls = app.mcp_list_state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" mcp "))
                    .highlight_style(hl())
                    .highlight_symbol("> "),
                main[0],
                &mut ls,
            );
            let pemguin_installed = app
                .active_project_idx
                .and_then(|i| app.projects.get(i))
                .map(|p| p.mcp_ready)
                .unwrap_or(false);
            let detail = if let Some(server) = app
                .mcp_list_state
                .selected()
                .and_then(|i| app.mcp_servers.get(i))
            {
                vec![
                    Line::from(Span::styled(server.name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("command  ", Style::default().fg(theme().fg_dim)),
                        Span::raw(if server.args.is_empty() {
                            server.command.clone()
                        } else {
                            format!("{} {}", server.command, server.args.join(" "))
                        }),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("pemguin  ", Style::default().fg(theme().fg_dim)),
                        Span::styled(
                            if pemguin_installed { "installed" } else { "not installed" },
                            Style::default().fg(if pemguin_installed { theme().green } else { theme().yellow }),
                        ),
                    ]),
                ]
            } else {
                vec![
                    Line::from(Span::styled("No MCP servers configured.", Style::default().fg(theme().fg_xdim))),
                    Line::from(""),
                    Line::from(Span::styled("Use `i` to install the pemguin MCP server.", Style::default().fg(theme().fg_dim))),
                ]
            };
            frame.render_widget(
                Paragraph::new(detail)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );
        }
        AgentSection::Skills => {
            let items: Vec<ListItem> = if app.skills.is_empty() {
                vec![ListItem::new("no installed skills")]
            } else {
                app.skills.iter().map(|s| ListItem::new(s.name.clone())).collect()
            };
            let mut ls = app.skills_list_state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" skills "))
                    .highlight_style(hl())
                    .highlight_symbol("> "),
                main[0],
                &mut ls,
            );
            let detail = app
                .skills_list_state
                .selected()
                .and_then(|i| app.skills.get(i))
                .map(|skill| {
                    vec![
                        Line::from(Span::styled(skill.name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("source   ", Style::default().fg(theme().fg_dim)),
                            Span::raw(skill.source.clone()),
                        ]),
                        Line::from(""),
                        Line::from(Span::raw(if skill.description.is_empty() {
                            "No description found.".to_string()
                        } else {
                            skill.description.clone()
                        })),
                    ]
                })
                .unwrap_or_else(|| {
                    vec![
                        Line::from(Span::styled("No skills installed.", Style::default().fg(theme().fg_xdim))),
                        Line::from(""),
                        Line::from(Span::styled("Use `b` to browse and install skills.", Style::default().fg(theme().fg_dim))),
                    ]
                });
            frame.render_widget(
                Paragraph::new(detail)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );
        }
        AgentSection::Sessions => {
            let items: Vec<ListItem> = if app.sessions.is_empty() {
                vec![ListItem::new("no recorded sessions")]
            } else {
                app.sessions.iter().map(|s| {
                    ListItem::new(format!("{}  {}", s.agent.label(), format_session_date(&s.started_at)))
                }).collect()
            };
            let mut ls = app.sessions_list_state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" sessions "))
                    .highlight_style(hl())
                    .highlight_symbol("> "),
                main[0],
                &mut ls,
            );
            let detail = app
                .sessions_list_state
                .selected()
                .and_then(|i| app.sessions.get(i))
                .map(|s| {
                    let mut lines = vec![
                        Line::from(Span::styled(
                            s.agent.label().to_uppercase(),
                            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("started   ", Style::default().fg(theme().fg_dim)),
                            Span::raw(format_session_date(&s.started_at)),
                        ]),
                    ];
                    if let Some(prompt) = &s.prompt {
                        lines.push(Line::from(vec![
                            Span::styled("prompt    ", Style::default().fg(theme().fg_dim)),
                            Span::raw(prompt.clone()),
                        ]));
                    }
                    if let Some(id) = &s.id {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![
                            Span::styled("resume    ", Style::default().fg(theme().fg_dim)),
                            Span::styled(s.agent.resume_cmd(id), Style::default().fg(theme().green)),
                        ]));
                    }
                    lines
                })
                .unwrap_or_else(|| {
                    vec![
                        Line::from(Span::styled("No sessions recorded.", Style::default().fg(theme().fg_xdim))),
                        Line::from(""),
                        Line::from(Span::styled("Use `n` to create a new agent launch command.", Style::default().fg(theme().fg_dim))),
                    ]
                });
            frame.render_widget(
                Paragraph::new(detail)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );
        }
    }

    if let Some(msg) = msg {
        let color = if msg.starts_with("Error:") { theme().red } else { theme().green };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("  {msg}"), Style::default().fg(color)))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
            outer[4],
        );
    }

    let footer_line = match app.agent_section {
        AgentSection::Mcp => footer(&[
            ("h/l", "section"),
            ("j/k", "navigate"),
            ("i", "install"),
            ("e", "edit"),
            ("d", "remove"),
            ("y", "copy"),
            ("r", "reload"),
            ("esc", "back"),
        ]),
        AgentSection::Skills => footer(&[
            ("h/l", "section"),
            ("j/k", "navigate"),
            ("b", "browse/install"),
            ("esc", "back"),
        ]),
        AgentSection::Sessions => footer(&[
            ("h/l", "section"),
            ("j/k", "navigate"),
            ("n", "new"),
            ("y", "resume"),
            ("s", "summary"),
            ("e", "export"),
            ("d", "delete"),
            ("esc", "back"),
        ]),
    };
    frame.render_widget(Paragraph::new(footer_line), outer[6]);
}

fn draw_skills(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let footer_h = if app.skills_browse && app.skills_install_message.is_some() { 3 } else { 1 };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(footer_h),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    if app.skills_browse {
        // ── Browse mode ──────────────────────────────────────────────────
        let browse_h = if app.skills_browse_query_active { 3 } else { 0 };
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(browse_h)])
            .split(outer[2]);

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(inner[0]);

        if app.skills_registry_loading {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  fetching skills.sh…",
                        Style::default().fg(theme().fg_dim),
                    )),
                ])
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" browse ")),
                outer[2],
            );
        } else {
            let items: Vec<ListItem> = app
                .skills_browse_indices
                .iter()
                .filter_map(|&i| app.skills_registry.get(i))
                .map(|s| ListItem::new(s.name.clone()))
                .collect();

            let title = if app.skills_browse_query.is_empty() {
                format!(" browse ({}) ", app.skills_browse_indices.len())
            } else {
                format!(
                    " browse — \"{}\" ({}) ",
                    app.skills_browse_query,
                    app.skills_browse_indices.len()
                )
            };

            let mut ls = app.skills_browse_list_state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(title))
                    .highlight_style(hl())
                    .highlight_symbol("> "),
                main[0],
                &mut ls,
            );

            let detail = app
                .skills_browse_list_state
                .selected()
                .and_then(|i| app.skills_browse_indices.get(i))
                .and_then(|&ri| app.skills_registry.get(ri))
                .map(|s| {
                    vec![
                        Line::from(Span::styled(
                            s.name.clone(),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("source   ", Style::default().fg(theme().fg_dim)),
                            Span::raw(s.source.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled("installs ", Style::default().fg(theme().fg_dim)),
                            Span::raw(format!("{}", s.installs)),
                        ]),
                        Line::from(""),
                        Line::from(Span::styled(
                            "press enter to install",
                            Style::default().fg(theme().fg_xdim),
                        )),
                    ]
                })
                .unwrap_or_default();

            frame.render_widget(
                Paragraph::new(detail)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );

            if app.skills_browse_query_active {
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("/", Style::default().fg(theme().accent)),
                        Span::raw(app.skills_browse_query.clone()),
                        Span::styled("█", Style::default().fg(theme().accent)),
                    ]))
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" search ")),
                    inner[1],
                );
            }
        }

        if let Some(msg) = &app.skills_install_message {
            let (icon, color) = if msg.starts_with("Error") {
                (I_CROSS, theme().red)
            } else {
                (I_CHECK, theme().green)
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("  {icon}  {msg}"),
                    Style::default().fg(color),
                ))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
                outer[3],
            );
        } else if app.skills_browse_query_active {
            frame.render_widget(
                Paragraph::new(footer(&[("esc", "cancel search"), ("enter", "confirm")])),
                outer[3],
            );
        } else {
            frame.render_widget(
                Paragraph::new(footer(&[
                    ("↑↓/jk", "navigate"),
                    ("/", "search"),
                    ("enter", "install"),
                    ("esc", "installed view"),
                ])),
                outer[3],
            );
        }
    } else {
        // ── Installed view ───────────────────────────────────────────────
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(outer[2]);

        let items: Vec<ListItem> = app
            .skills
            .iter()
            .map(|s| ListItem::new(s.name.clone()))
            .collect();

        if items.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  no skills installed",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  press b to browse skills.sh",
                        Style::default().fg(theme().fg_xdim),
                    )),
                ])
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" skills ")),
                outer[2],
            );
        } else {
            let mut ls = app.skills_list_state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" skills "))
                    .highlight_style(hl())
                    .highlight_symbol("> "),
                main[0],
                &mut ls,
            );

            let preview = app
                .skills_list_state
                .selected()
                .and_then(|i| app.skills.get(i))
                .map(|s| {
                    let mut lines = vec![
                        Line::from(Span::styled(
                            s.name.clone(),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("source  ", Style::default().fg(theme().fg_dim)),
                            Span::raw(s.source.clone()),
                        ]),
                    ];
                    if !s.description.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::from(Span::raw(s.description.clone())));
                    }
                    lines
                })
                .unwrap_or_default();

            frame.render_widget(
                Paragraph::new(preview)
                    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );
        }

        frame.render_widget(
            Paragraph::new(footer(&[
                ("↑↓/jk", "navigate"),
                ("b", "browse skills.sh"),
                ("esc", "back"),
                ("tab", "switch"),
                ("q", "quit"),
            ])),
            outer[3],
        );
    }
}

fn draw_pane(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let bottom_h = if app.pane_message.is_some() { 3 } else { 0 };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(bottom_h),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(outer[2]);
    let items = vec![ListItem::new("lazygit"), ListItem::new("yazi")];
    let mut ls = app.pane_list_state.clone();
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" tools "))
            .highlight_style(hl())
            .highlight_symbol("> "),
        main[0],
        &mut ls,
    );
    let detail = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  lazygit", Style::default().fg(Color::White)),
            Span::styled("  open repo git UI", Style::default().fg(theme().fg_dim)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  yazi", Style::default().fg(Color::White)),
            Span::styled("  browse project files", Style::default().fg(theme().fg_dim)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(detail).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" detail ")),
        main[1],
    );
    if let Some(msg) = &app.pane_message {
        let color = if msg.starts_with("Error:") {
            theme().red
        } else {
            theme().green
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("  {msg}"), Style::default().fg(color)))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
            outer[3],
        );
    }
    frame.render_widget(
        Paragraph::new(footer(&[
            ("↑↓/jk", "navigate"),
            ("enter", "launch"),
            ("esc", "back"),
            ("tab", "switch"),
            ("q", "quit"),
        ])),
        outer[4],
    );
}

fn handle_pane(app: &mut App, key: KeyCode) -> bool {
    const PANE_ITEMS: [&str; 2] = ["lazygit", "yazi"];
    match key {
        KeyCode::Down | KeyCode::Char('j') => {
            let n = (app.pane_list_state.selected().unwrap_or(0) + 1) % PANE_ITEMS.len();
            app.pane_list_state.select(Some(n));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let len = PANE_ITEMS.len();
            let n = app
                .pane_list_state
                .selected()
                .map(|i| if i == 0 { len - 1 } else { i - 1 })
                .unwrap_or(0);
            app.pane_list_state.select(Some(n));
        }
        KeyCode::Enter => {
            if let Some(idx) = app.pane_list_state.selected() {
                if let Some(project_idx) = app.active_project_idx {
                    if let Some(project) = app.projects.get(project_idx) {
                        let cwd = project.path.clone();
                        let command = match PANE_ITEMS[idx] {
                            "lazygit" => ExternalCommand {
                                program: "lazygit".to_string(),
                                args: vec![],
                                cwd,
                            },
                            "yazi" => ExternalCommand {
                                program: "yazi".to_string(),
                                args: vec![cwd.to_string_lossy().into_owned()],
                                cwd,
                            },
                            _ => return false,
                        };
                        app.pending_command = Some(command);
                        app.pane_message = None;
                    }
                }
            }
        }
        _ => {}
    }
    false
}

// ── Sessions persistence ──────────────────────────────────────────────────────

fn load_sessions(_project_path: &Path) -> Vec<AgentSession> {
    vec![]
}

// Attempt to resolve pending sessions by scanning agent storage.
// For Claude Code: ~/.claude/projects/{encoded-path}/{uuid}.jsonl
// Match unresolved sessions by finding JSONL files created within 120s of started_at.
// Find Claude project storage dirs for a given project path.
// Claude Code has used two encoding schemes over time:
//   v1: replace '/' with '-'  (underscores preserved)
//   v2: replace '/' and '_' with '-'
// We check both and return all that exist.
fn claude_project_dirs(project_path: &Path) -> Vec<PathBuf> {
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

// Resolve pending sessions AND import any existing Claude sessions not yet recorded.
fn resolve_sessions(sessions: &mut Vec<AgentSession>, project_path: &Path) {
    let claude_dirs = claude_project_dirs(project_path);

    if !claude_dirs.is_empty() {
        // Collect all JSONL files across all matching dirs
        let all_jsonl: Vec<(PathBuf, String, std::time::SystemTime)> = claude_dirs
            .iter()
            .flat_map(|dir| {
                jsonl_files_in(dir)
                    .into_iter()
                    .map(|(stem, mtime)| (dir.join(format!("{}.jsonl", stem)), stem, mtime))
                    .collect::<Vec<_>>()
            })
            .collect();

        // Resolve pending entries by timestamp proximity
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

        // Import sessions from Claude storage that aren't recorded yet
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

    // Import Codex sessions for this project
    import_codex_sessions(sessions, project_path);

    // Import Pi sessions for this project
    import_pi_sessions(sessions, project_path);

    // Import Gemini sessions for this project
    import_gemini_sessions(sessions, project_path);

    // Re-sort most recent first
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

    // Walk YYYY/MM/DD dirs
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

    // First line must be session_meta
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

    // Find first real user message
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

// Pi path encoding: strip leading '/', replace '/' with '-', wrap in '--'
// /Users/josh/Projects/_foo/bar → --Users-josh-Projects-_foo-bar--
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
        // Filename: <ISO8601-with-dashes>_<uuid>.jsonl — id is the uuid after the first '_'
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

    // Look up project name from projects.json
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
    // Also scan hex-named dirs in tmp/ (legacy SHA-256 dirs) — match by projectHash would require
    // sha256 computation; skip for now and rely on projects.json lookup.

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

// Parse ISO 8601 timestamp to unix seconds (simplified, no external crate)
fn chrono_parse(s: &str) -> Result<u64, ()> {
    // Expects format: 2026-03-29T14:23:00Z or similar
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
    // Rough unix seconds (good enough for 120s proximity matching)
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
    role: String,   // "user" or "assistant"
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

fn session_summary(turns: &[SessionTurn]) -> Vec<String> {
    // User messages only, as bullet points — what was asked/directed
    turns
        .iter()
        .filter(|t| t.role == "user")
        .map(|t| {
            let line: String = t.text.lines().next().unwrap_or("").chars().take(100).collect();
            format!("• {}", line)
        })
        .collect()
}

fn session_is_exported(session: &AgentSession, project_path: &Path) -> bool {
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

fn jsonl_path_for_session(session: &AgentSession, project_path: &Path) -> Option<PathBuf> {
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

fn unix_to_iso(secs: u64) -> String {
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

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    unix_to_iso(secs)
}

fn format_session_date(iso: &str) -> String {
    // Extract just the date and time portion for display
    iso.trim_end_matches('Z')
        .replacen('T', " ", 1)
        .chars()
        .take(16)
        .collect()
}

// ── Sessions handler ──────────────────────────────────────────────────────────

fn handle_sessions(app: &mut App, key: KeyCode) -> bool {
    match &app.sessions_state.clone() {
        SessionsState::NewPicker { agent_idx, prompt_idx } => {
            let agents = [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini, AgentKind::Pi];
            let agent_count = agents.len();
            match key {
                KeyCode::Esc => {
                    app.sessions_state = SessionsState::List;
                    app.sessions_message = None;
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    let new_idx = if *agent_idx == 0 { agent_count - 1 } else { agent_idx - 1 };
                    app.sessions_state = SessionsState::NewPicker { agent_idx: new_idx, prompt_idx: *prompt_idx };
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let new_idx = (agent_idx + 1) % agent_count;
                    app.sessions_state = SessionsState::NewPicker { agent_idx: new_idx, prompt_idx: *prompt_idx };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    // Cycle through prompts (None + project prompts)
                    let prompt_count = app.project_prompts.len();
                    let new_pidx = match prompt_idx {
                        None => {
                            if prompt_count > 0 { Some(prompt_count - 1) } else { None }
                        }
                        Some(i) => if *i == 0 { None } else { Some(i - 1) },
                    };
                    app.sessions_state = SessionsState::NewPicker { agent_idx: *agent_idx, prompt_idx: new_pidx };
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let prompt_count = app.project_prompts.len();
                    let new_pidx = match prompt_idx {
                        None => if prompt_count > 0 { Some(0) } else { None },
                        Some(i) => {
                            if i + 1 < prompt_count { Some(i + 1) } else { None }
                        }
                    };
                    app.sessions_state = SessionsState::NewPicker { agent_idx: *agent_idx, prompt_idx: new_pidx };
                }
                KeyCode::Enter | KeyCode::Char('y') => {
                    let agent = agents[*agent_idx].clone();
                    let prompt_name = prompt_idx.and_then(|i| app.project_prompts.get(i)).map(|p| p.name.clone());
                    let Some(proj_idx) = app.active_project_idx else { return false; };
                    let Some(project) = app.projects.get(proj_idx) else { return false; };
                    let path = project.path.clone();
                    let cmd = agent.launch_cmd(&path, prompt_name.as_deref());
                    // Write to clipboard
                    if let Ok(mut cb) = Clipboard::new() {
                        let _ = cb.set_text(&cmd);
                    }
                    // Record pending session
                    let session = AgentSession {
                        id: None,
                        agent,
                        started_at: now_iso(),
                        prompt: prompt_name,
                        first_message: None,
                    };
                    app.sessions.insert(0, session);
                    if app.sessions_list_state.selected().is_none() {
                        app.sessions_list_state.select(Some(0));
                    }
                    app.sessions_state = SessionsState::List;
                    app.sessions_message = Some(format!("copied: {}", cmd));
                }
                _ => {}
            }
        }
        SessionsState::List => {
            let len = app.sessions.len();
            match key {
                KeyCode::Char('n') => {
                    app.sessions_state = SessionsState::NewPicker { agent_idx: 0, prompt_idx: None };
                    app.sessions_message = None;
                }
                KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                    let n = (app.sessions_list_state.selected().unwrap_or(0) + 1) % len;
                    app.sessions_list_state.select(Some(n));
                    app.sessions_message = None;
                }
                KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                    let n = app.sessions_list_state.selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    app.sessions_list_state.select(Some(n));
                    app.sessions_message = None;
                }
                KeyCode::Enter | KeyCode::Char('y') => {
                    if let Some(i) = app.sessions_list_state.selected() {
                        if let Some(s) = app.sessions.get(i) {
                            if let Some(id) = &s.id {
                                let cmd = s.agent.resume_cmd(id);
                                if let Ok(mut cb) = Clipboard::new() {
                                    let _ = cb.set_text(&cmd);
                                }
                                app.sessions_message = Some(format!("copied: {}", cmd));
                            } else {
                                app.sessions_message = Some("no session id — run the session first".to_string());
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(i) = app.sessions_list_state.selected() {
                        let Some(proj_idx) = app.active_project_idx else { return false; };
                        let Some(_project) = app.projects.get(proj_idx) else { return false; };
                        app.sessions.remove(i);
                        if !app.sessions.is_empty() {
                            let new_sel = if i >= app.sessions.len() { app.sessions.len() - 1 } else { i };
                            app.sessions_list_state.select(Some(new_sel));
                        } else {
                            app.sessions_list_state.select(None);
                        }
                        app.sessions_message = Some("session removed from view".to_string());
                    }
                }
                KeyCode::Char('s') => {
                    // Inline summary view
                    let Some(i) = app.sessions_list_state.selected() else { return false; };
                    let Some(session) = app.sessions.get(i) else { return false; };
                    let Some(proj_idx) = app.active_project_idx else { return false; };
                    let Some(project) = app.projects.get(proj_idx) else { return false; };
                    if let Some(jsonl) = jsonl_path_for_session(session, &project.path) {
                        let turns = parse_session_turns(&jsonl);
                        let lines = session_summary(&turns);
                        if lines.is_empty() {
                            app.sessions_message = Some("no user messages found".to_string());
                        } else {
                            app.sessions_state = SessionsState::Summary { lines, scroll: 0 };
                        }
                    } else {
                        app.sessions_message = Some("session file not found locally".to_string());
                    }
                }
                KeyCode::Char('e') => {
                    app.sessions_message = Some("Read-only: pemguin observes sessions but does not export them into project directories.".to_string());
                }
                _ => {}
            }
        }
        SessionsState::Summary { lines, scroll } => {
            let lines = lines.clone();
            let scroll = *scroll;
            match key {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.sessions_state = SessionsState::List;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = lines.len().saturating_sub(1);
                    let new_scroll = (scroll + 1).min(max);
                    app.sessions_state = SessionsState::Summary { lines, scroll: new_scroll };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_scroll = scroll.saturating_sub(1);
                    app.sessions_state = SessionsState::Summary { lines, scroll: new_scroll };
                }
                _ => {}
            }
        }
    }
    false
}

// ── Sessions draw ─────────────────────────────────────────────────────────────

fn draw_sessions(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(header_row(app)), outer[0]);
    frame.render_widget(Paragraph::new(nav_row(app)), outer[1]);

    // Summary view
    if let SessionsState::Summary { lines, scroll } = &app.sessions_state {
        let visible: Vec<Line> = lines
            .iter()
            .skip(*scroll)
            .map(|l| {
                if l.starts_with("## ") {
                    Line::from(Span::styled(
                        l.trim_start_matches("## ").to_string(),
                        Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
                    ))
                } else if l.starts_with("• ") {
                    Line::from(vec![
                        Span::styled("• ", Style::default().fg(theme().fg_dim)),
                        Span::raw(l.trim_start_matches("• ").to_string()),
                    ])
                } else {
                    Line::from(l.as_str().to_string())
                }
            })
            .collect();
        frame.render_widget(
            Paragraph::new(visible)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" summary "))
                .wrap(Wrap { trim: false }),
            outer[2],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("j/k", Style::default().fg(theme().accent)),
                Span::raw(" scroll  "),
                Span::styled("esc", Style::default().fg(theme().fg_dim)),
                Span::raw(" back"),
            ])),
            outer[3],
        );
        return;
    }

    // Footer
    let footer = if let SessionsState::NewPicker { .. } = &app.sessions_state {
        Line::from(vec![
            Span::styled("←/→", Style::default().fg(theme().accent)),
            Span::raw(" agent  "),
            Span::styled("↑/↓", Style::default().fg(theme().accent)),
            Span::raw(" prompt  "),
            Span::styled("enter", Style::default().fg(theme().accent)),
            Span::raw(" copy & record  "),
            Span::styled("esc", Style::default().fg(theme().fg_dim)),
            Span::raw(" cancel"),
        ])
    } else {
        let selected_session = app.sessions_list_state.selected()
            .and_then(|i| app.sessions.get(i));
        let has_id = selected_session.and_then(|s| s.id.as_ref()).is_some();
        let mut spans = vec![
            Span::styled("n", Style::default().fg(theme().accent)),
            Span::raw(" new  "),
        ];
        if has_id {
            spans.extend([
                Span::styled("y", Style::default().fg(theme().accent)),
                Span::raw(" resume  "),
                Span::styled("s", Style::default().fg(theme().accent)),
                Span::raw(" summary  "),
                Span::styled("e", Style::default().fg(theme().accent)),
                Span::raw(" export  "),
            ]);
        }
        if !app.sessions.is_empty() {
            spans.extend([
                Span::styled("d", Style::default().fg(theme().fg_dim)),
                Span::raw(" delete"),
            ]);
        }
        Line::from(spans)
    };

    if let SessionsState::NewPicker { agent_idx, prompt_idx } = &app.sessions_state {
        let agents = ["claude", "codex", "gemini"];
        // Build agent selector line
        let agent_spans: Vec<Span> = agents.iter().enumerate().flat_map(|(i, a)| {
            let active = i == *agent_idx;
            let span = if active {
                Span::styled(a.to_string(), Style::default().fg(theme().sel_fg).bg(theme().accent).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(a.to_string(), Style::default().fg(theme().fg_dim))
            };
            vec![span, Span::raw("  ")]
        }).collect();

        // Build prompt selector
        let prompt_names: Vec<String> = std::iter::once("(none)".to_string())
            .chain(app.project_prompts.iter().map(|p| p.name.clone()))
            .collect();
        let selected_prompt_idx = prompt_idx.map(|i| i + 1).unwrap_or(0);
        let prompt_spans: Vec<Span> = prompt_names.iter().enumerate().flat_map(|(i, name)| {
            let active = i == selected_prompt_idx;
            let span = if active {
                Span::styled(name.clone(), Style::default().fg(theme().sel_fg).bg(theme().accent).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(name.clone(), Style::default().fg(theme().fg_dim))
            };
            vec![span, Span::raw("  ")]
        }).collect();

        // Preview the generated command
        let Some(proj_idx) = app.active_project_idx else { return; };
        let Some(project) = app.projects.get(proj_idx) else { return; };
        let agent_kind = [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini, AgentKind::Pi][*agent_idx].clone();
        let prompt_name = prompt_idx.and_then(|i| app.project_prompts.get(i)).map(|p| p.name.as_str());
        let cmd_preview = agent_kind.launch_cmd(&project.path, prompt_name);

        let content = vec![
            Line::from(""),
            Line::from(vec![Span::styled("  agent   ", Style::default().fg(theme().fg_dim))].into_iter().chain(agent_spans).collect::<Vec<_>>()),
            Line::from(""),
            Line::from(vec![Span::styled("  prompt  ", Style::default().fg(theme().fg_dim))].into_iter().chain(prompt_spans).collect::<Vec<_>>()),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("  $ ", Style::default().fg(theme().fg_dim)),
                Span::styled(cmd_preview, Style::default().fg(theme().green)),
            ]),
        ];

        frame.render_widget(
            Paragraph::new(content)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" new session ")),
            outer[2],
        );
    } else if app.sessions.is_empty() {
        let content = vec![
            Line::from(""),
            Line::from(Span::styled("  no sessions recorded", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from(Span::styled("  n  new session", Style::default().fg(theme().fg_xdim))),
        ];
        frame.render_widget(
            Paragraph::new(content)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" sessions ")),
            outer[2],
        );
    } else {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(outer[2]);

        let project_path = app.active_project_idx
            .and_then(|i| app.projects.get(i))
            .map(|p| p.path.clone());

        let items: Vec<ListItem> = app.sessions.iter().map(|s| {
            let date = format_session_date(&s.started_at);
            let exported = project_path.as_deref()
                .map(|p| session_is_exported(s, p))
                .unwrap_or(false);
            let export_mark = if exported { " ↓" } else { "" };
            let pending_mark = if s.id.is_none() { " ·" } else { "" };
            let spans = vec![
                Span::raw(format!("{}  {}", s.agent.label(), date)),
                Span::styled(export_mark.to_string(), Style::default().fg(theme().green)),
                Span::styled(pending_mark.to_string(), Style::default().fg(theme().fg_xdim)),
            ];
            ListItem::new(Line::from(spans))
        }).collect();

        let mut ls = app.sessions_list_state.clone();
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" sessions "))
                .highlight_style(hl())
                .highlight_symbol("> "),
            main[0],
            &mut ls,
        );

        // Detail pane
        let detail = app.sessions_list_state.selected()
            .and_then(|i| app.sessions.get(i))
            .map(|s| {
                let mut lines = vec![
                    Line::from(Span::styled(
                        s.agent.label().to_uppercase(),
                        Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("started   ", Style::default().fg(theme().fg_dim)),
                        Span::raw(format_session_date(&s.started_at)),
                    ]),
                ];
                if let Some(p) = &s.prompt {
                    lines.push(Line::from(vec![
                        Span::styled("prompt    ", Style::default().fg(theme().fg_dim)),
                        Span::raw(p.clone()),
                    ]));
                }
                if let Some(id) = &s.id {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("resume    ", Style::default().fg(theme().fg_dim)),
                        Span::styled(s.agent.resume_cmd(id), Style::default().fg(theme().green)),
                    ]));
                } else {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "  pending — id resolves after first session run",
                        Style::default().fg(theme().fg_xdim),
                    )));
                }
                if let Some(m) = &s.first_message {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("first msg ", Style::default().fg(theme().fg_dim)),
                        Span::raw(m.clone()),
                    ]));
                }
                lines
            })
            .unwrap_or_default();

        frame.render_widget(
            Paragraph::new(detail)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded))
                .wrap(Wrap { trim: true }),
            main[1],
        );
    }

    // Status/message bar
    let status = app.sessions_message.as_deref().map(|m| {
        Line::from(Span::styled(format!(" {}", m), Style::default().fg(theme().green)))
    }).unwrap_or(footer);
    frame.render_widget(Paragraph::new(status), outer[3]);
}

fn handle_skills(app: &mut App, key: KeyCode) -> bool {
    if app.skills_browse {
        // ── Browse mode key handling ──────────────────────────────────────
        if app.skills_browse_query_active {
            match key {
                KeyCode::Esc => {
                    app.skills_browse_query_active = false;
                }
                KeyCode::Enter => {
                    app.skills_browse_query_active = false;
                }
                KeyCode::Backspace => {
                    app.skills_browse_query.pop();
                    app.skills_browse_indices =
                        filter_registry(&app.skills_registry, &app.skills_browse_query);
                    let mut ls = ListState::default();
                    if !app.skills_browse_indices.is_empty() {
                        ls.select(Some(0));
                    }
                    app.skills_browse_list_state = ls;
                }
                KeyCode::Char(c) => {
                    app.skills_browse_query.push(c);
                    app.skills_browse_indices =
                        filter_registry(&app.skills_registry, &app.skills_browse_query);
                    let mut ls = ListState::default();
                    if !app.skills_browse_indices.is_empty() {
                        ls.select(Some(0));
                    }
                    app.skills_browse_list_state = ls;
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Esc => {
                if app.skills_install_message.is_some() {
                    app.skills_install_message = None;
                } else {
                    app.skills_browse = false;
                    app.skills_browse_query.clear();
                }
            }
            KeyCode::Char('b') => {
                app.skills_browse = false;
                app.skills_browse_query.clear();
                app.skills_install_message = None;
            }
            KeyCode::Char('/') => {
                app.skills_browse_query_active = true;
                app.skills_install_message = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = app.skills_browse_indices.len();
                if len > 0 {
                    let n = (app.skills_browse_list_state.selected().unwrap_or(0) + 1) % len;
                    app.skills_browse_list_state.select(Some(n));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let len = app.skills_browse_indices.len();
                if len > 0 {
                    let cur = app.skills_browse_list_state.selected().unwrap_or(0);
                    let n = if cur == 0 { len - 1 } else { cur - 1 };
                    app.skills_browse_list_state.select(Some(n));
                }
            }
            KeyCode::Enter => {
                app.skills_install_message = Some("Read-only: pemguin observes skills but does not install them.".to_string());
            }
            _ => {}
        }
        return false;
    }

    // ── Installed view key handling ───────────────────────────────────────
    if key == KeyCode::Char('b') {
        app.skills_browse = true;
        app.skills_install_message = None;
        if !app.skills_registry_loaded && !app.skills_registry_loading {
            app.skills_registry_loading = true;
            let tx = app.async_tx.clone();
            std::thread::spawn(move || {
                let skills = fetch_skills_registry();
                let _ = tx.send(AsyncResult::Registry(skills));
            });
        } else if app.skills_registry_loaded {
            app.skills_browse_indices =
                filter_registry(&app.skills_registry, &app.skills_browse_query);
            let mut ls = ListState::default();
            if !app.skills_browse_indices.is_empty() {
                ls.select(Some(0));
            }
            app.skills_browse_list_state = ls;
        }
        return false;
    }

    let len = app.skills.len();
    if len == 0 {
        return false;
    }
    match key {
        KeyCode::Down | KeyCode::Char('j') => {
            let n = (app.skills_list_state.selected().unwrap_or(0) + 1) % len;
            app.skills_list_state.select(Some(n));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let n = app
                .skills_list_state
                .selected()
                .map(|i| if i == 0 { len - 1 } else { i - 1 })
                .unwrap_or(0);
            app.skills_list_state.select(Some(n));
        }
        _ => {}
    }
    false
}

fn handle_agents(app: &mut App, key: KeyCode) -> bool {
    if app.skills_browse {
        return handle_skills(app, key);
    }
    if !matches!(&app.sessions_state, SessionsState::List) {
        return handle_sessions(app, key);
    }

    match key {
        KeyCode::Left | KeyCode::Char('h') => {
            app.agent_section = app.agent_section.prev();
            return false;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.agent_section = app.agent_section.next();
            return false;
        }
        _ => {}
    }

    match app.agent_section {
        AgentSection::Mcp => handle_mcp(app, key),
        AgentSection::Skills => handle_skills(app, key),
        AgentSection::Sessions => handle_sessions(app, key),
    }
}

fn handle_mcp(app: &mut App, key: KeyCode) -> bool {
    let len = app.mcp_servers.len();
    match key {
        KeyCode::Down | KeyCode::Char('j') if len > 0 => {
            let n = (app.mcp_list_state.selected().unwrap_or(0) + 1) % len;
            app.mcp_list_state.select(Some(n));
        }
        KeyCode::Up | KeyCode::Char('k') if len > 0 => {
            let n = app
                .mcp_list_state
                .selected()
                .map(|i| if i == 0 { len - 1 } else { i - 1 })
                .unwrap_or(0);
            app.mcp_list_state.select(Some(n));
        }
        KeyCode::Char('i') | KeyCode::Char('e') | KeyCode::Char('d') => {
            app.mcp_message = Some("Read-only: pemguin observes MCP config but does not create, edit, repair, or delete it.".to_string());
        }
        KeyCode::Char('r') => {
            app.refresh_mcp();
            app.mcp_message = Some("Reloaded MCP servers".to_string());
        }
        KeyCode::Char('y') | KeyCode::Enter if len > 0 => {
            if let Some(server) = app
                .mcp_list_state
                .selected()
                .and_then(|i| app.mcp_servers.get(i))
            {
                let cmd = if server.args.is_empty() {
                    server.command.clone()
                } else {
                    format!("{} {}", server.command, server.args.join(" "))
                };
                copy_to_clipboard(&cmd);
                app.mcp_message = Some(format!("copied: {cmd}"));
            }
        }
        _ => {}
    }
    false
}

fn handle_text_editor(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> bool {
    let Some(editor) = &mut app.text_editor else {
        return false;
    };
    let primary = modifiers.contains(KeyModifiers::CONTROL)
        || modifiers.contains(KeyModifiers::SUPER)
        || modifiers.contains(KeyModifiers::META);

    let clamp_col = |editor: &mut TextEditorState| {
        let len = editor.lines.get(editor.row).map(|l| l.len()).unwrap_or(0);
        editor.col = editor.col.min(len);
    };

    match key {
        KeyCode::Esc => {
            if selection_bounds(editor).is_some() {
                clear_selection(editor);
            } else {
                app.text_editor = None;
                app.reload_memories();
                app.reload_project_prompts();
            }
        }
        KeyCode::Char('s') if primary => {
            let _ = save_editor_state(editor);
            app.reload_memories();
            app.reload_project_prompts();
            app.refresh_setup();
        }
        KeyCode::Char('c') if primary => {
            if let Some(text) = selected_text(editor) {
                if let Ok(mut cb) = Clipboard::new() {
                    let _ = cb.set_text(text);
                    editor.status = Some("Copied.".to_string());
                }
            }
        }
        KeyCode::Char('x') if primary => {
            if let Some(text) = selected_text(editor) {
                if let Ok(mut cb) = Clipboard::new() {
                    let _ = cb.set_text(text);
                }
                let _ = delete_selection(editor);
                editor.status = Some("Cut.".to_string());
            }
        }
        KeyCode::Char('v') if primary => {
            if let Ok(mut cb) = Clipboard::new() {
                if let Ok(text) = cb.get_text() {
                    insert_text(editor, &text);
                    editor.status = Some("Pasted.".to_string());
                }
            }
        }
        KeyCode::Char('d') if primary => {
            duplicate_current_line(editor);
        }
        KeyCode::Char('k') if primary => {
            delete_current_line(editor);
        }
        KeyCode::Left if modifiers.contains(KeyModifiers::ALT) => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            move_word_left(editor);
        }
        KeyCode::Right if modifiers.contains(KeyModifiers::ALT) => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            move_word_right(editor);
        }
        KeyCode::Up => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            if editor.row > 0 {
                editor.row -= 1;
                clamp_col(editor);
            }
        }
        KeyCode::Down => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            if editor.row + 1 < editor.lines.len() {
                editor.row += 1;
                clamp_col(editor);
            }
        }
        KeyCode::Left => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            if editor.col > 0 {
                editor.col -= 1;
            } else if editor.row > 0 {
                editor.row -= 1;
                editor.col = editor.lines[editor.row].len();
            }
        }
        KeyCode::Right => {
            set_selection_mode(editor, modifiers.contains(KeyModifiers::SHIFT));
            let len = editor.lines[editor.row].len();
            if editor.col < len {
                editor.col += 1;
            } else if editor.row + 1 < editor.lines.len() {
                editor.row += 1;
                editor.col = 0;
            }
        }
        KeyCode::Backspace | KeyCode::Delete => {
            if delete_selection(editor) {
                return false;
            }
            set_selection_mode(editor, false);
            if editor.col > 0 {
                let line = &mut editor.lines[editor.row];
                line.remove(editor.col - 1);
                editor.col -= 1;
            } else if editor.row > 0 {
                let current = editor.lines.remove(editor.row);
                editor.row -= 1;
                editor.col = editor.lines[editor.row].len();
                editor.lines[editor.row].push_str(&current);
            }
        }
        KeyCode::Enter => {
            insert_text(editor, "\n");
        }
        KeyCode::Char(c) if !primary => {
            insert_text(editor, &c.to_string());
        }
        _ => {}
    }
    false
}

fn draw_text_editor(frame: &mut Frame, editor: &TextEditorState) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " 🐧 pm ",
                Style::default()
                    .fg(theme().sel_fg)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(&editor.title, Style::default().fg(Color::White)),
        ])),
        outer[0],
    );

    let height = outer[1].height.saturating_sub(2) as usize;
    let start = editor.row.saturating_sub(height.saturating_sub(1));
    let end = (start + height).min(editor.lines.len());
    let selection = selection_bounds(editor);
    let mut lines: Vec<Line> = Vec::new();
    for idx in start..end {
        let line = &editor.lines[idx];
        let mut spans: Vec<Span> = Vec::new();
        let line_len = line.len();
        let sel_range = selection.and_then(|((sr, sc), (er, ec))| {
            if idx < sr || idx > er {
                None
            } else {
                Some((
                    if idx == sr { sc.min(line_len) } else { 0 },
                    if idx == er {
                        ec.min(line_len)
                    } else {
                        line_len
                    },
                ))
            }
        });
        let mut chars: Vec<(usize, char)> = line.char_indices().collect();
        chars.push((line_len, '\0'));
        for window in chars.windows(2) {
            let byte_idx = window[0].0;
            let ch = window[0].1;
            let next_byte_idx = window[1].0;
            if idx == editor.row && byte_idx == editor.col.min(line_len) {
                let selected = sel_range
                    .map(|(s, e)| byte_idx >= s && byte_idx < e)
                    .unwrap_or(false);
                let style = if selected {
                    Style::default().fg(theme().sel_fg).bg(theme().accent)
                } else {
                    Style::default().fg(theme().accent)
                };
                spans.push(Span::styled("█", style));
            }
            if ch != '\0' {
                let selected = sel_range
                    .map(|(s, e)| byte_idx < e && next_byte_idx > s)
                    .unwrap_or(false);
                let mut style = Style::default();
                if selected {
                    style = style.bg(theme().purple).fg(Color::Black);
                }
                spans.push(Span::styled(ch.to_string(), style));
            }
        }
        if idx == editor.row && editor.col >= line_len {
            let selected = sel_range
                .map(|(s, e)| line_len >= s && line_len < e)
                .unwrap_or(false);
            let style = if selected {
                Style::default().fg(theme().sel_fg).bg(theme().accent)
            } else {
                Style::default().fg(theme().accent)
            };
            spans.push(Span::styled("█", style));
        }
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("█", Style::default().fg(theme().accent))));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" editor "))
            .wrap(Wrap { trim: false }),
        outer[1],
    );

    let footer_text = if let Some(status) = &editor.status {
        footer(&[("Ctrl+S", "save"), ("esc", "close")])
            .spans
            .into_iter()
            .chain(vec![Span::styled(
                format!("  {status}"),
                Style::default().fg(theme().green),
            )])
            .collect::<Vec<_>>()
    } else {
        footer(&[
            ("Ctrl+S", "save"),
            ("esc", "close"),
            ("←→↑↓", "move"),
            ("Opt+←→", "jump word"),
            ("Opt+Shift+←→", "select by word"),
            ("enter", "newline"),
            ("backspace", "delete"),
        ])
        .spans
    };
    frame.render_widget(Paragraph::new(Line::from(footer_text)), outer[2]);
}

fn draw_fill(
    frame: &mut Frame,
    app: &App,
    prompt_idx: usize,
    field_idx: usize,
    values: &HashMap<String, String>,
    input: &str,
) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let prompt = &app.prompts[prompt_idx];
    let auto = app.auto_values();
    let fillable: Vec<&String> = prompt
        .placeholders
        .iter()
        .filter(|p| !auto.contains_key(*p))
        .collect();

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " 🐧 pm ",
                Style::default()
                    .fg(theme().sel_fg)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(&prompt.name, Style::default().fg(Color::White)),
        ])),
        outer[0],
    );

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Fill in placeholders",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for placeholder in &prompt.placeholders {
        if let Some(v) = auto.get(placeholder) {
            lines.push(Line::from(vec![
                Span::raw(format!("  {placeholder:<14}")),
                Span::styled(v.clone(), Style::default().fg(theme().fg_dim)),
                Span::styled("  (auto)", Style::default().fg(theme().fg_xdim)),
            ]));
            continue;
        }
        let fi = fillable.iter().position(|p| *p == placeholder).unwrap_or(0);
        if fi < field_idx {
            let val = values.get(placeholder).map(|s| s.as_str()).unwrap_or("");
            lines.push(Line::from(vec![
                Span::raw(format!("  {placeholder:<14}")),
                Span::styled(val.to_string(), Style::default().fg(theme().green)),
                Span::styled(format!("  {I_CHECK}"), Style::default().fg(theme().green)),
            ]));
        } else if fi == field_idx {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {placeholder:<14}"),
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{input}█"), Style::default().fg(Color::White)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(format!("  {placeholder:<14}")),
                Span::styled("...", Style::default().fg(theme().fg_xdim)),
            ]));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)),
        outer[1],
    );
    frame.render_widget(
        Paragraph::new(footer(&[("enter", "confirm"), ("esc", "back")])),
        outer[2],
    );
}

fn draw_done(frame: &mut Frame, text: &str) {
    let area = frame.area();
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {I_CHECK}  "), Style::default().fg(theme().green)),
            Span::styled(
                "Copied to clipboard",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  ─────────────────────────────",
            Style::default().fg(theme().fg_xdim),
        )),
        Line::from(""),
    ];
    for line in text.lines().take(20) {
        lines.push(Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  any key to continue",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" pemguin ")),
        area,
    );
}

fn draw_delete_confirm(frame: &mut Frame, confirm: &DeleteConfirm) {
    let area = frame.area();
    let popup = centered_rect(62, 9, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL).border_type(BorderType::Rounded)
            .title(" confirm delete ")
            .style(Style::default().bg(Color::Black)),
        popup,
    );
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);
    frame.render_widget(
        Paragraph::new(Span::styled(&confirm.title, Style::default().fg(theme().red))),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            &confirm.detail,
            Style::default().fg(Color::White),
        )),
        inner[1],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "This cannot be undone from pemguin.",
            Style::default().fg(theme().fg_dim),
        )),
        inner[2],
    );
    frame.render_widget(
        Paragraph::new(footer(&[("y/enter", "delete"), ("n/esc", "cancel")])),
        inner[3],
    );
}

// ── Main ──────────────────────────────────────────────────────────────────────

pub fn start() -> io::Result<()> {
    let config = load_config();
    set_theme(&config.theme);
    let mut app = App::new(config);
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal, &mut app);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        reload_pemguin_theme_if_changed(&mut app.theme_mtime);
        app.process_async_results();
        terminal.draw(|f| draw(f, app))?;

        if let Some(path) = app.pending_editor.take() {
            app.open_text_editor(path);
            continue;
        }
        if let Some(command) = app.pending_command.take() {
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            let result = Command::new(&command.program)
                .current_dir(&command.cwd)
                .args(&command.args)
                .status();
            enable_raw_mode()?;
            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
            terminal.clear()?;
            app.refresh_setup();
            app.skills_loaded = false;
            app.pane_message = match result {
                Ok(status) if status.success() => Some("Tool exited.".to_string()),
                Ok(status) => Some(format!("Error: exited with status {status}")),
                Err(e) => Some(format!("Error: {e}")),
            };
            continue;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(app, key.code, key.modifiers) {
                        break;
                    }
                }
                Event::Paste(text) => {
                    if let Some(editor) = &mut app.text_editor {
                        insert_text(editor, &text);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
