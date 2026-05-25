use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Skill {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) description: String,
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

pub(crate) fn load_skills(path: &Path) -> Vec<Skill> {
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
