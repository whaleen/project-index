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
- MCP server: `px mcp serve` exposes read-only inspection tools
- Source split into focused modules: config, project, github, agent, skills, cli, lib

## Known Rough Edges

- `gh` errors degrade poorly in the UI
- Nerd Font glyphs have no fallback
- Codex session scan walks all historical session dirs on each project load
- Gemini summary viewer not yet implemented
- Project selection resets after full rescan

## What's Next

1. Global agent view: surface `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, `~/.agents` as a system-level tab
2. Improve sessions: cross-project view, faster Codex scan, Gemini summaries
3. Grok Build support (see issue #4)
4. Per-provider config/settings observation (see `provider-table.md` for planned locations)
5. Skills surface: per-agent skill dirs beyond shared `~/.agents/skills/`
