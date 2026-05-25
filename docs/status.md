# project-index — Status

## Current Direction

project-index is a read-only work index for humans and agents. It should observe local projects, git state, native agent context, sessions, memories, skills, MCP config, and global agent storage without writing project files.

See `docs/adr/0001-read-only-work-index.md` and `SPEC.md`.

## What's Working

- `px` opens, scans projects, groups by org directory
- Background project scanning; TUI renders immediately, data hydrates as it arrives
- Opening a project is immediate; Home and Issues hydrate in the background, other tabs load lazily
- 7 tabs navigate correctly; Esc returns to the project list
- Home shows repo identity, recent commits, GitHub metadata, README, dirty files, and local summary data
- Issues reads open GitHub issues through `gh`
- Config tab observes native context files (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json`)
- Memories tab reads native agent memory/config sources:
  - Claude: `~/.claude/projects/<encoded>/memory/`
  - Codex: `~/.codex/memories/<repo-name>/`
  - Gemini: `~/.gemini/GEMINI.md`
- Agents tab consolidates MCP, skills, and sessions
  - Sessions discovered from native storage for Claude, Codex, Gemini, and Pi
  - Inline session summary exists for Claude and Pi JSONL sessions
- MCP server: `px mcp serve` exposes inspection-style tools

## Cleanup In Progress

- Remove setup/apply/delete/reset behavior from the Config tab
- Remove prompt management (done)
- Remove project-local state assumptions (done)
- Rename setup-era CLI/MCP surfaces to inspection-only language
- Bring `docs/agents/` in sync with current storage readers
- Split `cli/src/lib.rs` by concern

## Known Rough Edges

- `cli/src/lib.rs` is still large but modules are now split


- `gh` errors degrade poorly in the UI
- Nerd Font glyphs have no fallback
- Codex session scan walks all historical session dirs on each project load
- Gemini summary viewer and legacy SHA-256 session dirs need work
- Project selection on rescan resets instead of preserving focus

## What's Next

1. Finish read-only conversion of Config/Context behavior.
2. Remove prompt manager and scaffolding templates.
3. Add a System/global-agent view for `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, and `~/.agents`.
4. Improve sessions as an index: cross-project view, faster Codex scan, Gemini summaries.

