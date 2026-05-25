mod config;
mod github;
mod project;
mod agent;
mod skills;
mod cli;

use std::collections::HashMap;
use std::fs;
use std::io;
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

use crate::config::{
    Config,
    load_config, set_theme, theme, reload_theme_if_changed,
    I_BRANCH, I_CHECK, I_CROSS, I_WARN, I_BULLET, I_COMMIT, I_ISSUES, I_SETUP,
    I_PROJECTS, I_MEMORY, I_MCP, I_PANE,
};
use crate::project::{
    RepoMeta, Issue, Project, ProjectEntry, HomeData, SetupStatus, SetupItem,
    scan_setup, setup_item_edit_path, setup_template_count,
    load_home_data, load_home_data_local, load_issues,
    scan_projects, project_info, build_project_entries,
};
use crate::github::{
    load_meta_cache, save_meta_cache, refresh_project_meta,
    lang_short, relative_date,
    fetch_avatar, ansi_to_lines,
};
use crate::agent::{
    MemoryAgent, AgentSection, AgentKind, AgentSession, SessionsState, MemoriesView, MemoryFile,
    claude_memory_path, load_memory_files, codex_memory_dirs, gemini_memory_path,
    load_sessions, resolve_sessions,
    session_summary_from_path, session_is_exported, jsonl_path_for_session,
    now_iso, format_session_date,
};
use crate::skills::{Skill, McpServer, ExternalCommand, load_skills, load_mcp_servers};

pub use crate::cli::run_cli;

struct TextEditorState {
    path: PathBuf,
    title: String,
    lines: Vec<String>,
    row: usize,
    col: usize,
    selection_anchor: Option<(usize, usize)>,
    status: Option<String>,
}

struct App {
    config: Config,
    screen: Screen,
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
    pending_command: Option<ExternalCommand>,
    // Skills
    skills: Vec<Skill>,
    skills_list_state: ListState,
    skills_loaded: bool,
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
}


// ── Screens ───────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
enum ProjectTab {
    Home,
    Config,
    Issues,
    Memories,
    Agents,
    Pane,
}

#[derive(PartialEq)]
enum Screen {
    Projects,
    InProject(ProjectTab),
}


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

// ── App ───────────────────────────────────────────────────────────────────────

impl App {
    fn new(config: Config) -> Self {
        let (async_tx, async_rx) = mpsc::channel();
        let projects = vec![];
        let project_entries = vec![];

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

        let mut app = App {
            config,
            screen: Screen::Projects,
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
            pending_command: None,
            skills: vec![],
            skills_list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            skills_loaded: false,
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

    fn open_text_editor(&mut self, path: PathBuf) {
        match load_editor_state(&path) {
            Ok(editor) => {
                self.text_editor = Some(editor);
            }
            Err(e) => {
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
                if !self.setup_items.is_empty() {
                    self.setup_list_state.select(Some(0));
                }
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
            home.sessions_count = load_sessions(&project.path).len();
            home.skills_count = load_skills(&project.path).len();
        }

        self.setup_on_open = false;
    }

    fn switch_project(&mut self, idx: usize) {
        let Some(project) = self.projects.get(idx) else {
            return;
        };
        self.repo = project.repo.clone();
        self.context = make_context(&project.repo, &project.branch);
        self.active_project_idx = Some(idx);
        let path = project.path.clone();
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
        self.setup_on_open = false;
        self.screen = Screen::InProject(ProjectTab::Home);
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


// ── Event handling ────────────────────────────────────────────────────────────

fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> bool {
    if app.text_editor.is_some() {
        return handle_text_editor(app, key, modifiers);
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
            let in_flow = app.memory_inputting;
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
                            Screen::InProject(ProjectTab::Issues) => ProjectTab::Memories,
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
                        app.set_project_tab(ProjectTab::Memories);
                        return false;
                    }
                    KeyCode::Char('5') => {
                        app.set_project_tab(ProjectTab::Agents);
                        return false;
                    }
                    KeyCode::Char('6') => {
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
                ProjectTab::Memories => handle_memories(app, key),
                ProjectTab::Agents => handle_agents(app, key),
                ProjectTab::Pane => handle_pane(app, key),
            }
        }
    }
}

fn handle_home(app: &mut App, key: KeyCode) -> bool {
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
            app.home_save_msg = Some("Read-only: project-index observes GitHub metadata but does not edit it.".to_string());
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
                copy_to_clipboard(&number);
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

fn copy_to_clipboard(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text);
    }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &App) {
    if let Some(editor) = &app.text_editor {
        draw_text_editor(frame, editor);
        return;
    }
    match &app.screen {
        Screen::Projects => draw_projects(frame, app),
        Screen::InProject(ProjectTab::Home) => draw_home(frame, app),
        Screen::InProject(ProjectTab::Config) => draw_setup(frame, app),
        Screen::InProject(ProjectTab::Issues) => draw_issues(frame, app),
        Screen::InProject(ProjectTab::Memories) => draw_memories(frame, app),
        Screen::InProject(ProjectTab::Agents) if !matches!(&app.sessions_state, SessionsState::List) => draw_sessions(frame, app),
        Screen::InProject(ProjectTab::Agents) => draw_agents(frame, app),
        Screen::InProject(ProjectTab::Pane) => draw_pane(frame, app),
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
        " px",
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
        (I_MEMORY, "memories", 4, *active_tab == ProjectTab::Memories),
        (I_MCP, "agents", 5, *active_tab == ProjectTab::Agents),
        (I_PANE, "pane", 6, *active_tab == ProjectTab::Pane),
    ];
    for (icon, label, n, active) in tabs {
        spans.extend(tab_span(icon, label, *n, *active));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn draw_home(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let bottom_h = if app.home_save_msg.is_some() {
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

    if let Some(msg) = &app.home_save_msg {
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

    frame.render_widget(
        Paragraph::new(footer(&[
            ("c", "config"),
            ("r", "reload"),
            ("y", "copy url"),
            ("jk", "scroll readme"),
            ("esc", "back"),
        ])),
        outer[4],
    );
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
                app.memory_message = Some("Read-only: project-index observes memories but does not create them.".to_string());
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
            app.memory_message = Some("Read-only: project-index observes memories but does not edit, create, or delete them.".to_string());
        }
        KeyCode::Char('r') => {
            app.reload_memories();
        }
        _ => {}
    }
    false
}

enum SetupRenderRow {
    GroupHeader(String),
    Item(usize),
}

fn setup_render_rows(items: &[SetupItem]) -> Vec<SetupRenderRow> {
    let mut rows = Vec::new();
    let mut last_cat: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        let cat = item.category.as_str();
        if last_cat != Some(cat) {
            rows.push(SetupRenderRow::GroupHeader(cat.to_string()));
            last_cat = Some(cat);
        }
        rows.push(SetupRenderRow::Item(i));
    }
    rows
}

fn selected_setup_item(items: &[SetupItem], ls: &ListState) -> Option<usize> {
    let row_idx = ls.selected()?;
    let rows = setup_render_rows(items);
    if let Some(SetupRenderRow::Item(i)) = rows.get(row_idx) {
        Some(*i)
    } else {
        None
    }
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
            app.setup_message = Some("Read-only: project-index observes context files but does not edit, create, delete, reset, or repair them.".to_string());
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
                        "  Read-only: create or repair files outside project-index if desired.",
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
                vec![ListItem::new("project-index")]
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
            let px_installed = app
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
                        Span::styled("project-index  ", Style::default().fg(theme().fg_dim)),
                        Span::styled(
                            if px_installed { "installed" } else { "not installed" },
                            Style::default().fg(if px_installed { theme().green } else { theme().yellow }),
                        ),
                    ]),
                ]
            } else {
                vec![
                    Line::from(Span::styled("No MCP servers configured.", Style::default().fg(theme().fg_xdim))),
                    Line::from(""),
                    Line::from(Span::styled("Use `i` to install the project-index MCP server.", Style::default().fg(theme().fg_dim))),
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

// ── Sessions handler ──────────────────────────────────────────────────────────

fn handle_sessions(app: &mut App, key: KeyCode) -> bool {
    match &app.sessions_state.clone() {
        SessionsState::NewPicker { agent_idx, prompt_idx: _ } => {
            let agents = [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini, AgentKind::Pi];
            let agent_count = agents.len();
            match key {
                KeyCode::Esc => {
                    app.sessions_state = SessionsState::List;
                    app.sessions_message = None;
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    let new_idx = if *agent_idx == 0 { agent_count - 1 } else { agent_idx - 1 };
                    app.sessions_state = SessionsState::NewPicker { agent_idx: new_idx, prompt_idx: None };
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let new_idx = (agent_idx + 1) % agent_count;
                    app.sessions_state = SessionsState::NewPicker { agent_idx: new_idx, prompt_idx: None };
                }
                KeyCode::Enter | KeyCode::Char('y') => {
                    let agent = agents[*agent_idx].clone();
                    let Some(proj_idx) = app.active_project_idx else { return false; };
                    let Some(project) = app.projects.get(proj_idx) else { return false; };
                    let path = project.path.clone();
                    let cmd = agent.launch_cmd(&path, None);
                    if let Ok(mut cb) = Clipboard::new() {
                        let _ = cb.set_text(&cmd);
                    }
                    let session = AgentSession {
                        id: None,
                        agent,
                        started_at: now_iso(),
                        prompt: None,
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
                        let lines = session_summary_from_path(&jsonl);
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
                    app.sessions_message = Some("Read-only: project-index observes sessions but does not export them into project directories.".to_string());
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

    if let SessionsState::NewPicker { agent_idx, .. } = &app.sessions_state {
        let agents = ["claude", "codex", "gemini", "pi"];
        let agent_spans: Vec<Span> = agents.iter().enumerate().flat_map(|(i, a)| {
            let active = i == *agent_idx;
            let span = if active {
                Span::styled(a.to_string(), Style::default().fg(theme().sel_fg).bg(theme().accent).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(a.to_string(), Style::default().fg(theme().fg_dim))
            };
            vec![span, Span::raw("  ")]
        }).collect();

        let Some(proj_idx) = app.active_project_idx else { return; };
        let Some(project) = app.projects.get(proj_idx) else { return; };
        let agent_kind = [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini, AgentKind::Pi][*agent_idx].clone();
        let cmd_preview = agent_kind.launch_cmd(&project.path, None);

        let content = vec![
            Line::from(""),
            Line::from(vec![Span::styled("  agent   ", Style::default().fg(theme().fg_dim))].into_iter().chain(agent_spans).collect::<Vec<_>>()),
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
            app.mcp_message = Some("Read-only: project-index observes MCP config but does not create, edit, repair, or delete it.".to_string());
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
            }
        }
        KeyCode::Char('s') if primary => {
            let _ = save_editor_state(editor);
            app.reload_memories();
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
                " px",
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
        reload_theme_if_changed(&mut app.theme_mtime);
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
