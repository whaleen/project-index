# project-index — Architecture Overview

## Design Philosophy

project-index is a read-only work index. It observes local repos and native agent/system storage so humans and agents can share situational awareness.

It must not scaffold, repair, mutate, or synchronize project files. See `docs/adr/0001-read-only-work-index.md`.

## Structure

Code is split across `cli/src/` modules: config, project, github, agent, skills, cli, lib.

Entry surfaces:

- **TUI**: `project_index::start()`
- **CLI**: `project_index::run_cli(&args)`
- **MCP**: `px mcp serve`

`src/main.rs` and `src/bin/px.rs` route to CLI when subcommands are present, otherwise start the TUI.

## Screen Model

```text
Screen::Projects          — root project list
Screen::InProject(tab)    — selected project
```

Current tabs: `Home | Issues | Config | Prompts | Memories | Agents | Pane`.

Target tabs may change as cleanup removes prompt/setup behavior. The intended durable surfaces are Overview/Git, Issues, Context, Sessions, Agents/System.

## Data Sources

### Projects / Git

`scan_projects()` walks up to 2 levels from the configured root. `project_info()` reads branch, dirty count, ahead/behind, and context presence.

### GitHub

Home and Issues shell out to `gh`. GitHub metadata and avatars may be cached under `~/.project-index/` for display performance.

### Context files

The Config/Context surface observes project files such as:

- `SPEC.md`
- `AGENTS.md`
- `CLAUDE.md`
- `GEMINI.md`
- `.mcp.json`
- agent/project skill files when present

These are observations only. Pemguin should not create, delete, reset, repair, or edit them.

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

## Read-only Contract

No UI, CLI, or MCP path should write project files. If a feature needs mutation, it belongs outside Pemguin or must be explicitly re-approved with a new ADR.

Legacy mutation paths to remove:

- setup apply/reset/delete
- MCP install/repair/edit/delete
- prompt creation/editing/storage
- memory creation/editing/deletion/sync
- project-local `.project-index/` session registries/exports

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
