# project-index — Agent Context

## What This Project Is

project-index (`px`) is a read-only terminal work index built with Ratatui. It gives humans and agents one place to observe local projects, git state, native agent context, sessions, memories, skills, MCP config, and related system state.

It is deliberately **observer-first and read-only**. Do not add scaffolding, setup, repair, prompt-storage, memory-sync, or project-file mutation behavior.

Read first:

1. `docs/adr/0001-read-only-work-index.md`
2. `docs/adr/0002-tauri-desktop-companion.md`
3. `SPEC.md`
4. `docs/status.md`

## Stack

- **Language**: Rust stable
- **TUI**: Ratatui 0.29 + Crossterm 0.28
- **Desktop companion**: Tauri v2 + React + TypeScript + Tailwind/shadcn-style UI under `app/`
- **External tools**: `git`, `gh` CLI, optional `chafa`, optional `$EDITOR` for legacy paths
- **Config**: `~/.project-index.toml` for projects root + theme
- **Runtime cache**: `~/.project-index/` for observation cache such as avatars/GitHub metadata

## Running Locally

```bash
cd cli
cargo run --bin px           # dev build
cargo install --path .       # install ~/.cargo/bin/px

cd ../app
pnpm install                 # desktop companion deps
pnpm tauri dev               # rich UI discovery surface
```

Prerequisites: Rust stable, `gh` CLI authenticated for GitHub data, Nerd Font terminal.

## Architecture Notes

Implementation is split across `cli/src/` modules (config, project, github, agent, skills, cli, lib).

Important reader areas:

- project scanning: `scan_projects`, `project_info`, `git_status_summary`
- context/config observation: `scan_setup_unfiltered`
- memories: `claude_memory_path`, `codex_memory_dirs`, `gemini_memory_path`
- sessions: `resolve_sessions`, `import_codex_sessions`, `import_pi_sessions`, `import_gemini_sessions`
- MCP/CLI inspection: `inspect_project`, `mcp_tool_defs`, `mcp_tool_result`

## Product Rules

- Do not write project files from project-index UI/CLI/MCP/app.
- Do not create or maintain project-local `.project-index/`, `.memory/`, or `.prompts/` state.
- Do not add prompt management.
- Do not install/repair/remove MCP servers, skills, or agent-inbox files.
- `SPEC.md` is a contract artifact that project-index observes; enforcement belongs in an agent skill/workflow.
- Prefer native agent storage readers over project-index-owned registries.

## Gotchas


- Session path encodings differ by agent; consult `docs/agents/` before changing readers.
- Codex scans can be expensive because they walk date-bucketed history.
- `gh` failures should be visible but non-fatal.
- Theme is hot-reloaded from `~/.project-index.toml`; dotfiles generate that config.
