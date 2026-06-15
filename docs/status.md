# project-index — Status

## What's Working

- `px` opens, scans projects, groups by org directory
- Background project scanning; TUI renders immediately, data hydrates as it arrives
- Opening a project is immediate; Home and Issues hydrate in the background, other tabs load lazily
- Home shows repo identity, recent commits, GitHub metadata, README, dirty files, and local summary data
- Issues reads open GitHub issues through `gh`
- Config tab observes native context files (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json`)
- Memories tab reads native agent memory:
  - Claude: `~/.claude/projects/<encoded>/memory/`
  - Codex: `~/.codex/memories/<repo-name>/`
  - Gemini: `~/.gemini/GEMINI.md`
- Agents tab consolidates MCP, skills, and sessions
  - Sessions discovered from native storage for Claude, Codex, Gemini, and Pi
  - Inline session summary for Claude and Pi JSONL sessions
- Tauri desktop companion now includes project Memories and Agents tabs:
  - Memories previews native Claude/Codex/Gemini/Pi memory/config files
  - Agents inventories project-local skills, `.mcp.json` servers, and native agent sessions
- MCP server: `px mcp serve` exposes read-only inspection tools
- Source split into focused modules: config, project, github, agent, skills, cli, lib

## Known Rough Edges

- `gh` errors degrade poorly in the UI
- Nerd Font glyphs have no fallback
- Codex session scan walks all historical session dirs on each project load
- Gemini summary viewer not yet implemented
- Desktop local refresh now has a lightweight filesystem watcher and 60s polling fallback, but it still uses frontend-triggered rescans rather than a backend observation store with per-resource snapshots

## What's Next

1. Continue ADR 0003: add backend-owned observation store with per-resource snapshots and bounded local refresh queues. GitHub cached inspection, freshness metadata, manual refresh buttons, serialized remote calls, and lightweight local watcher events are in place.
2. Agent inbox observation: refine the cross-project inbox dashboard, record detail view, filters, and copyable agent prompts
3. Global agent view: surface `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, `~/.agents` as a system-level tab
4. Improve sessions: cross-project view, faster Codex scan, Gemini summaries
5. Grok Build support (see issue #4)
6. Per-provider config/settings observation (see `provider-table.md` for planned locations)
7. Improve desktop Agents/Memories tabs with session detail summaries and global agent inventory
