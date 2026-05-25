use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::project::{
    SetupItem, SetupStatus, scan_setup_all, setup_item_edit_path,
};

// ── CLI/MCP data types ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct CliProjectInspect {
    pub(crate) project_path: String,
    pub(crate) repo_root: bool,
    pub(crate) recommended_ok: usize,
    pub(crate) recommended_total: usize,
    pub(crate) setup_complete: bool,
    pub(crate) project_files: Vec<CliManagedPath>,
    pub(crate) items: Vec<CliSetupItem>,
    pub(crate) next_actions: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct CliManagedPath {
    pub(crate) path: String,
    pub(crate) kind: &'static str,
    pub(crate) required: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct CliSetupItem {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) category: &'static str,
    pub(crate) status: &'static str,
    pub(crate) required: bool,
    pub(crate) optional: bool,
    pub(crate) gitignore_path: Option<String>,
    pub(crate) gitignored: bool,
    pub(crate) edit_path: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct CliAgentInstructions {
    pub(crate) project_path: String,
    pub(crate) summary: String,
    pub(crate) inspect_command: String,
    pub(crate) notes: Vec<String>,
}

struct McpToolDef {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

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

pub(crate) fn inspect_project(path: &Path) -> CliProjectInspect {
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
        summary: "Use project-index as a read-only project and agent-state index.".to_string(),
        inspect_command: format!("px project inspect --json {}", path.display()),
        notes: vec![
            "Read `SPEC.md`, `AGENTS.md`, `CLAUDE.md`, or `GEMINI.md` when they exist.".to_string(),
            "Do not ask project-index to create, repair, or mutate project files.".to_string(),
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
    println!("project-index CLI");
    println!();
    println!("  px project inspect [--json] [PATH]");
    println!("  px context inspect [--json] [PATH]");
    println!("  px agent instructions [--json] [PATH]");
    println!("  px mcp serve");
    println!("  px mcp-server");
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

// ── MCP server ────────────────────────────────────────────────────────────────

fn mcp_tool_defs() -> Vec<McpToolDef> {
    vec![
        McpToolDef {
            name: "px_project_inspect",
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
            name: "px_setup_plan",
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
            name: "px_agent_instructions",
            title: "project-index Agent Instructions",
            description: "Return read-only project-index instructions an agent should follow when inspecting project context.",
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
        "px_project_inspect" => {
            let path = mcp_default_path(arguments)?;
            serde_json::to_value(inspect_project(&path)).map_err(|e| e.to_string())
        }
        "px_setup_plan" => {
            let path = mcp_default_path(arguments)?;
            serde_json::to_value(inspect_project(&path)).map_err(|e| e.to_string())
        }
        "px_agent_instructions" => {
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
                            "name": "project-index",
                            "title": "project-index",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "Use project-index tools as a read-only index of project and agent state. Inspect observed context; do not ask project-index to create, repair, or mutate files."
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
