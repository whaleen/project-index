# project-index — Architecture Overview

## Design Philosophy

project-index is a read-only work index. It observes local repos and native agent/system storage so humans and agents can share situational awareness.

It must not scaffold, repair, mutate, or synchronize project files. See `docs/adr/0001-read-only-work-index.md`.

## Structure

Code is split across `cli/src/` modules for the terminal/MCP surface and `app/` for the Tauri desktop companion.

Entry surfaces:

- **TUI**: `project_index::start()`
- **CLI**: `project_index::run_cli(&args)`
- **MCP**: `px mcp serve`
- **Desktop app**: `app/` Tauri v2 + React companion, with Rust commands in `app/src-tauri/src/lib.rs`

`cli/src/main.rs` and `cli/src/bin/px.rs` route to CLI when subcommands are present, otherwise start the TUI. The desktop app is run through Tauri from `app/`.

## Screen Model

```text
Screen::Projects          — root project list
Screen::InProject(tab)    — selected project
```

Current TUI tabs: `Home | Issues | Config | Memories | Agents | Pane`.

Current desktop project tabs: `Overview | Work | Context | GitHub | Agents | Activity`. The desktop app is the rich UI discovery surface; proven views can later be ported back to Ratatui.

## Data Sources

### Projects / Git

`scan_projects()` walks up to 2 levels from the configured root. `project_info()` reads branch, dirty count, ahead/behind, and context presence.

### GitHub

The TUI Home and Issues surfaces shell out to `gh`. GitHub metadata and avatars may be cached under `~/.project-index/` for display performance.

The desktop app keeps GitHub repo metadata/OpenGraph and issues as stale-while-revalidate observations. Navigation reads cached data; explicit refresh actions perform live GitHub calls and update SQLite-backed cache records with freshness metadata.

### Context files

The Config/Context surface observes project files such as:

- `SPEC.md`
- `AGENTS.md`
- `CLAUDE.md`
- `GEMINI.md`
- `.mcp.json`
- agent/project skill files when present

These are observations only. project-index should not create, delete, reset, repair, or edit them.

### Native agent storage

Reader docs live under `docs/agents/`.

Sessions:

- Claude: `~/.claude/projects/<encoded>/`
- Codex: `~/.codex/sessions/YYYY/MM/DD/`
- Gemini: `~/.gemini/projects.json` + `~/.gemini/tmp/<project>/chats/`
- Pi: `~/.pi/agent/sessions/<encoded>/`

Memory/config:

- Claude: `~/.claude/projects/<encoded>/memory/`
- Codex: `~/.codex/memories/<repo-name or sanitized-path>/`
- Gemini: `~/.gemini/GEMINI.md`
- Pi: `~/.pi/agent/` config/session state

Skills/MCP:

- project `.mcp.json`
- project `.agents/skills` and `skills-lock.json`
- global `~/.agents`, `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent` sources as the implementation expands

### Desktop observation cache

The desktop app stores observation snapshots, GitHub caches, and bounded activity events in SQLite at `~/.project-index/project-index.sqlite`. This is a cache only; repositories, native agent files, `.agent/inbox/`, and GitHub remain the source of truth.

Lightweight filesystem watchers observe project roots, `.git`, context/docs, `.agent/inbox/`, and skill directories, then emit `observation://updated` events to the frontend. A polling fallback keeps the dashboard moving if watcher events are missed.

## Read-only Contract

No UI, CLI, MCP, or desktop-app path should write project files. If a feature needs mutation, it belongs outside project-index or must be explicitly re-approved with a new ADR.

Legacy mutation paths to keep out of scope:

- setup apply/reset/delete
- MCP install/repair/edit/delete
- prompt creation/editing/storage
- memory creation/editing/deletion/sync
- project-local `.project-index/`, `.pemguin/`, `.memory/`, or `.prompts/` state as source of truth

## Module Split Target

When cleanup stabilizes behavior, split `cli/src/lib.rs` into modules like:

- `config`
- `theme`
- `git`
- `github`
- `project_scan`
- `agent_readers`
- `context_surface`
- `app_state`
- `render`
- `handlers`
- `cli_mcp`
