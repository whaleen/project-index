# project-index — Status

## What's Working

- `px` opens, scans projects, groups by org directory
- Background project scanning; TUI renders immediately, data hydrates as it arrives
- Opening a project is immediate; Home and Issues hydrate in the background, other tabs load lazily
- Home shows repo identity, recent commits, GitHub metadata, README, dirty files, and local summary data
- Issues reads open GitHub issues through `gh`
- Config/Context surfaces observe native context files plus `docs/` and `docs/adr/` as standard documentation/ADR locations
- Memories tab reads native agent memory:
  - Claude: `~/.claude/projects/<encoded>/memory/`
  - Codex: `~/.codex/memories/<repo-name>/`
  - Gemini: `~/.gemini/GEMINI.md`
- Agents tab consolidates MCP, skills, and sessions
  - Sessions discovered from native storage for Claude, Codex, Gemini, and Pi
  - Inline session summary for Claude and Pi JSONL sessions
- Tauri desktop companion project pages now use focused tabs: Overview, Work, Context, GitHub, Agents, Activity
  - Work keeps Agent Inbox and GitHub Issues separate but colocated
  - Context owns README/docs/ADR/context file surfaces
  - Agents combines memories, skills, MCP servers, and native sessions
- MCP server: `px mcp serve` exposes read-only inspection tools
- Source split into focused modules: config, project, github, agent, skills, cli, lib

## Known Rough Edges

- `gh` errors degrade poorly in the UI
- Nerd Font glyphs have no fallback
- Codex session scan walks all historical session dirs on each project load
- Gemini summary viewer not yet implemented
- Desktop local refresh now has a lightweight filesystem watcher, targeted project refresh events with backend-observed project snapshots, an in-memory observation store, a local SQLite observation cache at `~/.project-index/project-index.sqlite`, a top-level Activity log, a basic storage inspector/count badge, and 60s polling fallback.

## What's Next

1. Continue ADR 0003/0004 polish: expand Activity filters/details, improve the storage inspector, and add richer local freshness UI. GitHub caches, local observation snapshots, and bounded observation events are now persisted to SQLite with JSON cache fallback/migration for existing GitHub cache files.
2. Agent inbox observation: refine the cross-project inbox dashboard, record detail view, filters, and copyable agent prompts
3. Global agent view: surface `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, `~/.agents` as a system-level tab
4. Improve sessions: cross-project view, faster Codex scan, Gemini summaries
5. Grok Build support (see issue #4)
6. Per-provider config/settings observation (see `provider-table.md` for planned locations)
7. Improve desktop Agents/Memories tabs with session detail summaries and global agent inventory
