use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct Skill {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) content: String,
    pub(crate) scope: String,
    pub(crate) path: PathBuf,
    pub(crate) seen_by: Vec<String>,
}

pub(crate) struct McpServer {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
}

pub(crate) struct ExternalCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
}

struct SkillDirSpec<'a> {
    rel: &'a str,
    scope: &'a str,
    seen_by: &'a [&'a str],
    allow_root_md: bool,
}

pub(crate) fn load_skills(path: &Path) -> Vec<Skill> {
    let lock_sources = load_skill_lock_sources(path);
    let specs = [
        SkillDirSpec {
            rel: ".agents/skills",
            scope: "shared project",
            seen_by: &["codex", "gemini", "pi"],
            allow_root_md: false,
        },
        SkillDirSpec {
            rel: ".claude/skills",
            scope: "claude project",
            seen_by: &["claude"],
            allow_root_md: false,
        },
        SkillDirSpec {
            rel: ".codex/skills",
            scope: "codex project",
            seen_by: &["codex"],
            allow_root_md: false,
        },
        SkillDirSpec {
            rel: ".gemini/skills",
            scope: "gemini project",
            seen_by: &["gemini"],
            allow_root_md: false,
        },
        SkillDirSpec {
            rel: ".pi/skills",
            scope: "pi project",
            seen_by: &["pi"],
            allow_root_md: true,
        },
    ];

    let mut skills = Vec::new();
    for spec in specs {
        let dir = path.join(spec.rel);
        skills.extend(load_skills_from_dir(&dir, spec, &lock_sources));
    }

    skills.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| a.path.cmp(&b.path))
    });
    skills
}

fn load_skill_lock_sources(path: &Path) -> BTreeMap<String, String> {
    let lock_path = path.join("skills-lock.json");
    let content = match fs::read_to_string(&lock_path) {
        Ok(c) => c,
        Err(_) => return BTreeMap::new(),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return BTreeMap::new(),
    };
    let Some(skills_obj) = json.get("skills").and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };

    skills_obj
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

fn load_skills_from_dir(
    dir: &Path,
    spec: SkillDirSpec<'_>,
    lock_sources: &BTreeMap<String, String>,
) -> Vec<Skill> {
    if !dir.is_dir() {
        return vec![];
    }

    let mut skills = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e: &std::fs::DirEntry| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let fallback_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("skill")
                    .to_string();
                skills.push(read_skill_file(
                    &skill_md,
                    fallback_name,
                    spec.scope,
                    spec.seen_by,
                    lock_sources,
                ));
            }
        } else if spec.allow_root_md && path.extension().and_then(|s| s.to_str()) == Some("md") {
            let fallback_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string();
            skills.push(read_skill_file(
                &path,
                fallback_name,
                spec.scope,
                spec.seen_by,
                lock_sources,
            ));
        }
    }

    skills
}

fn read_skill_file(
    path: &Path,
    fallback_name: String,
    scope: &str,
    seen_by: &[&str],
    lock_sources: &BTreeMap<String, String>,
) -> Skill {
    let content = fs::read_to_string(path).unwrap_or_default();
    let name = frontmatter_value(&content, "name").unwrap_or(fallback_name);
    let source = lock_sources
        .get(&name)
        .cloned()
        .unwrap_or_else(|| "filesystem".to_string());

    Skill {
        name,
        source,
        content,
        scope: scope.to_string(),
        path: path.to_path_buf(),
        seen_by: seen_by.iter().map(|s| s.to_string()).collect(),
    }
}

fn frontmatter_value(content: &str, key: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            return Some(
                value
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string(),
            );
        }
    }
    None
}

pub(crate) fn load_mcp_servers(path: &Path) -> Vec<McpServer> {
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
