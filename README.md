# project-index

Read-only work index for developers who work with AI agents.

`px` is a Ratatui TUI that gives humans and agents one terminal surface for observing local projects, git state, native agent context files, sessions, memories, skills, MCP config, and related system state.

**Observer-first. Read-only.** project-index reads from where git and agents naturally store things. It does not scaffold projects, write prompts, sync memories, repair config, or maintain project-local state.

## Install

```bash
# via Homebrew
brew tap whaleen/tap
brew install project-index

# or from source
git clone https://github.com/whaleen/project-index
cd project-index/cli
cargo install --path .
```

Requires: Rust stable, `gh` CLI for GitHub metadata/issues, Nerd Font terminal.

## Usage

```bash
px
```

Navigate with `↑↓` or `jk`. Press `enter` to open a project. `esc` goes back. `q` quits.

## Project tabs

| Key | Tab | What it shows |
|-----|-----|---------------|
| `1` | Home | Repo identity, README, recent commits, git/GitHub summary |
| `2` | Issues | Open GitHub issues via `gh` |
| `3` | Config | Observed context/config files: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json` |
| `4` | Memories | Native agent memory/config surfaces |
| `5` | Agents | MCP servers, skills, and per-project sessions |
| `6` | Pane | Legacy launcher surface |

## Sessions

Sessions are discovered directly from each agent's native storage:

| Agent | Storage location |
|-------|------------------|
| Claude Code | `~/.claude/projects/<encoded>/` JSONL |
| Codex | `~/.codex/sessions/YYYY/MM/DD/` JSONL |
| Gemini CLI | `~/.gemini/tmp/<project-name>/chats/` JSON |
| Pi | `~/.pi/agent/sessions/<encoded>/` JSONL |

## Memories / native config

project-index observes native agent locations. It does not copy or synchronize these into project-index-owned directories.

| Agent | Source |
|-------|--------|
| Claude Code | `~/.claude/projects/<encoded>/memory/` |
| Codex | `~/.codex/memories/<repo-name>/` |
| Gemini CLI | `~/.gemini/GEMINI.md` |
| Pi | in-session state and `~/.pi/agent/` config/session files |

## Configuration

`~/.project-index.toml`:

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

`~/.project-index/` contains observation cache such as GitHub metadata and avatars.

## MCP server

`px` can run as a local stdio MCP server:

```bash
px mcp serve
```

MCP tools are read-only inspection surfaces for agents.

Current tools:

- `px_project_inspect`
- `px_setup_plan`
- `px_agent_instructions`

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
project-index/
  cli/            Rust TUI + CLI source
  docs/adr/       product/architecture decisions
  docs/agents/    native storage interface docs
  docs/architecture/
```
