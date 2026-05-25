# pemguin 🐧

Read-only work index for developers who work with AI agents.

`pm` / `pemguin` is a Ratatui TUI that gives humans and agents one terminal surface for observing local projects, git state, native agent context files, sessions, memories, skills, MCP config, and related system state.

**Observer-first. Read-only.** Pemguin reads from where git and agents naturally store things. It does not scaffold projects, write prompts, sync memories, repair config, or maintain project-local Pemguin state.

## Install

```bash
git clone https://github.com/whaleen/pemguin
cd pemguin/cli
cargo install --path .
```

Installs both `pm` and `pemguin`.

Requires: Rust stable, `gh` CLI for GitHub metadata/issues, Nerd Font terminal.

## Usage

```bash
pm
pemguin
```

Navigate with `↑↓` or `jk`. Press `enter` to open a project. `esc` goes back. `q` quits.

## Project tabs

| Key | Tab | What it shows |
|-----|-----|---------------|
| `1` | Home | Repo identity, README, recent commits, git/GitHub summary |
| `2` | Issues | Open GitHub issues via `gh` |
| `3` | Config | Observed context/config files: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json` |
| `4` | Prompts | Legacy prompt browser; slated for removal |
| `5` | Memories | Native agent memory/config surfaces |
| `6` | Agents | MCP servers, skills, and per-project sessions |
| `7` | Pane | Legacy launcher surface; slated for reconsideration under read-only rules |

## Sessions

Sessions are discovered directly from each agent's native storage:

| Agent | Storage location |
|-------|------------------|
| Claude Code | `~/.claude/projects/<encoded>/` JSONL |
| Codex | `~/.codex/sessions/YYYY/MM/DD/` JSONL |
| Gemini CLI | `~/.gemini/tmp/<project-name>/chats/` JSON |
| Pi | `~/.pi/agent/sessions/<encoded>/` JSONL |

## Memories / native config

Pemguin observes native agent locations. It should not copy or synchronize these into Pemguin-owned project directories.

| Agent | Source |
|-------|--------|
| Claude Code | `~/.claude/projects/<encoded>/memory/` |
| Codex | `~/.codex/memories/<repo-name>/` |
| Gemini CLI | `~/.gemini/GEMINI.md` |
| Pi | in-session state and `~/.pi/agent/` config/session files |

## Configuration

`~/.pemguin.toml`:

```toml
[projects]
root = "~/Projects"   # scanned 2 levels deep for .git dirs

[theme]
accent  = "#e8b887"   # hot-reloaded on file change
sel_fg  = "#101010"
fg_dim  = "#A0A0A0"
fg_xdim = "#7E7E7E"
green   = "#90b99f"
red     = "#f5a191"
yellow  = "#e6b99d"
purple  = "#aca1cf"
```

Set `PEMGUIN_PROJECTS_DIR` to override `projects.root` via env.

`~/.pemguin/` may contain observation cache such as GitHub metadata and avatars. Project-local `.pemguin/` directories are legacy artifacts, not canonical Pemguin storage.

## MCP server

`pm` can run as a local stdio MCP server:

```bash
pm mcp serve
```

MCP tools are intended to be read-only inspection surfaces for agents.

Current tools:

- `pemguin_project_inspect`
- `pemguin_setup_plan` — legacy naming; should become inspection-only or be removed
- `pemguin_agent_instructions`

## Supported agents

| Agent | Binary | Sessions | Memory / config | Skills |
|-------|--------|----------|-----------------|--------|
| Claude Code | `claude` | `~/.claude/projects/` | `~/.claude/projects/<enc>/memory/` | `~/.claude/skills/` + plugins |
| Codex | `codex` | `~/.codex/sessions/` | `~/.codex/memories/` | `~/.codex/skills/` |
| Gemini CLI | `gemini` | `~/.gemini/tmp/` | `~/.gemini/GEMINI.md` | `~/.gemini/skills/` + `~/.agents/skills/` |
| Pi | `pi` | `~/.pi/agent/sessions/` | `~/.pi/agent/` | `~/.pi/agent/skills/` + `~/.agents/skills/` |

See `docs/agents/` for storage interface docs.

## Project structure

```text
pemguin/
  cli/            Rust TUI + CLI source
  docs/adr/       product/architecture decisions
  docs/agents/    native storage interface docs
  docs/architecture/
```
