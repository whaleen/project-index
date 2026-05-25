# pemguin — Agent Context

## What This Project Is

Pemguin (`pm` / `pemguin`) is a read-only terminal work index built with Ratatui. It gives humans and agents one place to observe local projects, git state, native agent context, sessions, memories, skills, MCP config, and related system state.

It is deliberately **observer-first and read-only**. Do not add scaffolding, setup, repair, prompt-storage, memory-sync, or project-file mutation behavior.

Read first:

1. `docs/adr/0001-read-only-work-index.md`
2. `SPEC.md`
3. `docs/status.md`

## Stack

- **Language**: Rust stable
- **TUI**: Ratatui 0.29 + Crossterm 0.28
- **External tools**: `git`, `gh` CLI, optional `chafa`, optional `$EDITOR` for legacy paths
- **Config**: `~/.pemguin.toml` for projects root + theme
- **Runtime cache**: `~/.pemguin/` for observation cache such as avatars/GitHub metadata

## Running Locally

```bash
cd cli
cargo run --bin pm           # dev build
cargo install --path .       # install ~/.cargo/bin/{pm,pemguin}
```

Prerequisites: Rust stable, `gh` CLI authenticated for GitHub data, Nerd Font terminal.

## Architecture Notes

Current implementation is mostly in `cli/src/lib.rs`. This is a known cleanup target. Keep changes small and behavior-focused until modules are split.

Important reader areas:

- project scanning: `scan_projects`, `project_info`, `git_status_summary`
- context/config observation: `scan_setup_unfiltered`
- memories: `claude_memory_path`, `codex_memory_dirs`, `gemini_memory_path`
- sessions: `resolve_sessions`, `import_codex_sessions`, `import_pi_sessions`, `import_gemini_sessions`
- MCP/CLI inspection: `inspect_project`, `mcp_tool_defs`, `mcp_tool_result`

## Product Rules

- Do not write project files from Pemguin UI/CLI/MCP.
- Do not create or maintain project-local `.pemguin/`, `.memory/`, or `.prompts/` state.
- Do not add prompt management.
- Do not install/repair/remove MCP servers or skills.
- `SPEC.md` is a contract artifact that Pemguin observes; enforcement belongs in an agent skill/workflow.
- Prefer native agent storage readers over Pemguin-owned registries.

## Gotchas

- `cli/src/lib.rs` still contains setup-era and prompt-manager code. Treat it as legacy to remove, not as product direction.
- Session path encodings differ by agent; consult `docs/agents/` before changing readers.
- Codex scans can be expensive because they walk date-bucketed history.
- `gh` failures should be visible but non-fatal.
- Theme is hot-reloaded from `~/.pemguin.toml`; dotfiles generate that config.
