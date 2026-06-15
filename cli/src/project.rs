use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agent::load_sessions;
use crate::config::{Config, dirs_home};
use crate::skills::load_skills;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RepoMeta {
    pub(crate) language: Option<String>,
    pub(crate) topics: Vec<String>,
    pub(crate) pushed_at: Option<String>,
    pub(crate) open_issues: Option<u32>,
}

pub(crate) struct Issue {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) labels: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct Project {
    pub(crate) path: PathBuf,
    pub(crate) group: String,
    pub(crate) repo: String,
    pub(crate) branch: String,
    pub(crate) dirty_count: u32,
    pub(crate) commits_ahead: u32,
    pub(crate) commits_behind: u32,
    pub(crate) recommended_ok: usize,
    pub(crate) recommended_total: usize,
    pub(crate) mcp_ready: bool,
    pub(crate) template_count: usize,
}

#[derive(Clone)]
pub(crate) enum ProjectEntry {
    Group(String),
    Item(usize),
}

pub(crate) struct RecentCommit {
    pub(crate) hash: String,
    pub(crate) date_label: String,
    pub(crate) time_label: String,
    pub(crate) subject: String,
}

pub(crate) struct HomeData {
    pub(crate) gh_description: Option<String>,
    pub(crate) homepage: Option<String>,
    pub(crate) url: String,
    pub(crate) recent_commits: Vec<RecentCommit>,
    pub(crate) recommended_ok: usize,
    pub(crate) recommended_total: usize,
    pub(crate) stack: Option<String>,
    pub(crate) stars: Option<u32>,
    pub(crate) forks: Option<u32>,
    pub(crate) license: Option<String>,
    pub(crate) open_prs: Option<u32>,
    pub(crate) readme: Option<String>,
    pub(crate) dirty_files: Vec<String>,
    pub(crate) mcp_ready: bool,
    pub(crate) sessions_count: usize,
    pub(crate) skills_count: usize,
}

// ── Setup types ───────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
pub(crate) enum SetupStatus {
    Ok,
    Missing,
    Stale,
    Template,
}

impl SetupStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            SetupStatus::Ok => "ok",
            SetupStatus::Missing => "missing",
            SetupStatus::Stale => "stale",
            SetupStatus::Template => "template",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupCategory {
    Recommended,
}

impl SetupCategory {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            SetupCategory::Recommended => "recommended",
        }
    }
}

#[derive(Clone)]
pub(crate) struct SetupItem {
    pub(crate) label: &'static str,
    pub(crate) detail: String,
    pub(crate) category: SetupCategory,
    pub(crate) status: SetupStatus,
    pub(crate) gitignore_path: Option<&'static str>,
    pub(crate) gitignored: bool,
}

pub(crate) fn scan_setup(path: &Path) -> Vec<SetupItem> {
    scan_setup_all(path)
}

pub(crate) fn scan_setup_all(path: &Path) -> Vec<SetupItem> {
    scan_setup_unfiltered(path)
}

pub(crate) fn scan_setup_unfiltered(path: &Path) -> Vec<SetupItem> {
    let spec_ok = path.join("SPEC.md").exists();
    let claude_ok = {
        let p = path.join("CLAUDE.md");
        p.is_symlink() || p.exists()
    };
    let agents_ok = {
        let p = path.join("AGENTS.md");
        p.is_symlink() || p.exists()
    };
    let gemini_ok = {
        let p = path.join("GEMINI.md");
        p.is_symlink() || p.exists()
    };
    let docs_ok = path.join("docs").exists();
    let adr_ok = path.join("docs").join("adr").exists();
    let mcp_ok = path.join(".mcp.json").exists();

    vec![
        SetupItem {
            label: "SPEC.md",
            detail: "project contract artifact".to_string(),
            category: SetupCategory::Recommended,
            status: if spec_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "CLAUDE.md",
            detail: "Claude Code context file".to_string(),
            category: SetupCategory::Recommended,
            status: if claude_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "AGENTS.md",
            detail: "Codex / Pi context file".to_string(),
            category: SetupCategory::Recommended,
            status: if agents_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "GEMINI.md",
            detail: "Gemini CLI context file".to_string(),
            category: SetupCategory::Recommended,
            status: if gemini_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "docs/",
            detail: "project documentation directory".to_string(),
            category: SetupCategory::Recommended,
            status: if docs_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: "docs/adr/",
            detail: "architecture decision records".to_string(),
            category: SetupCategory::Recommended,
            status: if adr_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
        SetupItem {
            label: ".mcp.json",
            detail: "MCP server definitions".to_string(),
            category: SetupCategory::Recommended,
            status: if mcp_ok {
                SetupStatus::Ok
            } else {
                SetupStatus::Missing
            },
            gitignore_path: None,
            gitignored: false,
        },
    ]
}

pub(crate) fn setup_item_edit_path(project_path: &Path, item: &SetupItem) -> Option<PathBuf> {
    match item.label {
        "SPEC.md" => Some(project_path.join("SPEC.md")),
        "CLAUDE.md" => Some(project_path.join("CLAUDE.md")),
        "AGENTS.md" => Some(project_path.join("AGENTS.md")),
        "GEMINI.md" => Some(project_path.join("GEMINI.md")),
        "docs/" => Some(project_path.join("docs")),
        "docs/adr/" => Some(project_path.join("docs").join("adr")),
        ".mcp.json" => Some(project_path.join(".mcp.json")),
        _ => None,
    }
}

pub(crate) fn setup_template_count(items: &[SetupItem]) -> usize {
    items
        .iter()
        .filter(|item| item.status == SetupStatus::Template)
        .count()
}

// ── Home data loading ─────────────────────────────────────────────────────────

pub(crate) fn load_home_data(path: &Path, repo: &str) -> HomeData {
    let (gh_description, homepage, stars, forks, license, open_prs) = if !repo.is_empty() {
        let out = Command::new("gh")
            .args([
                "repo",
                "view",
                repo,
                "--json",
                "description,homepageUrl,stargazerCount,forkCount,licenseInfo,pullRequests",
            ])
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

    let recent_commits = load_recent_commits(path);
    let items = scan_setup(path);
    let recommended_ok = items.iter().filter(|i| i.status == SetupStatus::Ok).count();
    let recommended_total = items.len();
    let mcp_ready = path.join(".mcp.json").exists();
    let sessions_count = load_sessions(path).len();
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

pub(crate) fn load_home_data_local(path: &Path, repo: &str) -> HomeData {
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
    let sessions_count = load_sessions(path).len();
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

// ── Project scanning ──────────────────────────────────────────────────────────

pub(crate) fn scan_projects(config: &Config) -> Vec<Project> {
    let bases: Vec<PathBuf> = if let Ok(env_dir) = std::env::var("PROJECT_INDEX_PROJECTS_DIR") {
        vec![PathBuf::from(env_dir)]
    } else {
        config
            .projects
            .effective_roots()
            .iter()
            .map(|r| PathBuf::from(expand_tilde(r)))
            .collect()
    };

    let mut candidates: Vec<(PathBuf, String)> = Vec::new();

    for base in &bases {
        let Ok(level1) = fs::read_dir(base) else {
            continue;
        };
        let base_name = base
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let mut level1_dirs: Vec<_> = level1
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        level1_dirs.sort_by_key(|e| e.file_name());

        for entry in level1_dirs {
            let path = entry.path();
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if dir_name.starts_with('.') {
                continue;
            }
            if path.join(".git").is_dir() {
                candidates.push((path, base_name.clone()));
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

    projects.sort_by(|a, b| {
        let ga = if a.group.is_empty() { "\x7f" } else { &a.group };
        let gb = if b.group.is_empty() { "\x7f" } else { &b.group };
        ga.cmp(gb).then(a.repo.cmp(&b.repo))
    });
    projects
}

pub(crate) fn project_info(path: &Path, group: String) -> Option<Project> {
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
    let recommended_ok = setup_items
        .iter()
        .filter(|i| i.status == SetupStatus::Ok)
        .count();
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

pub(crate) fn git_status_summary(path: &Path) -> (String, u32, u32, u32) {
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

pub(crate) fn build_project_entries(projects: &[Project]) -> Vec<ProjectEntry> {
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

// ── Issue loading ─────────────────────────────────────────────────────────────

pub(crate) fn load_issues(repo: &str) -> Result<Vec<Issue>, String> {
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

// ── Git helpers ───────────────────────────────────────────────────────────────

pub(crate) fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(crate) fn parse_repo(url: &str) -> String {
    let url = url.trim().trim_end_matches(".git");
    if url.starts_with("https://") || url.starts_with("http://") {
        let prefix = if url.starts_with("https://") { 8 } else { 7 };
        if let Some(slash) = url[prefix..].find('/') {
            return url[prefix + slash + 1..].to_string();
        }
    }
    if let Some(pos) = url.rfind(':') {
        let after = &url[pos + 1..];
        if after.contains('/') {
            return after.to_string();
        }
    }
    url.to_string()
}

pub(crate) fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}
