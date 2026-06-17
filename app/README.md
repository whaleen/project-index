# project-index desktop companion

Tauri v2 + React + Tailwind CSS v4 + shadcn/ui companion app for iterating on the rich project-index UI.

This app is a read-only observer. It may display local project, context, docs, git/GitHub, agent, skills, MCP, Agent Library, and `.agent/inbox/` state, but it must not install, repair, scaffold, or mutate project files.

## Run

```bash
pnpm install
pnpm tauri dev

# production validation
pnpm build
cd src-tauri && cargo check
```

## Current shape

- scans the configured projects root (`PROJECT_INDEX_PROJECTS_DIR` or `~/Projects`)
- lists git repositories up to two levels deep, sorted by latest commit
- shows repo identity, app icons/OpenGraph imagery where available, branch, dirty/ahead/behind state, and latest commit metadata
- observes project context files: `SPEC.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.mcp.json`, `docs/`, `docs/adr/`, and `.agent/inbox/` files
- previews README and ADR/documentation state on project Context surfaces
- observes `.agent/inbox/` docs/schema/feedback JSONL, summarizes status counts, and renders active record detail without writing records
- renders cross-project Dashboard tabs for Projects, Agent Inbox, and GitHub Issues
- renders a top-level Activity page backed by the local observation cache
- renders a separate Agent Library page for prompts, recipes, and skills from the configured/local agent-library checkout
- renders selected project tabs: Overview, Work, Context, GitHub, Agents, Activity
- keeps GitHub repo metadata/issues cached, shows freshness metadata, and refreshes GitHub only through explicit refresh actions
- persists observation snapshots, GitHub caches, and bounded observation events to SQLite at `~/.project-index/project-index.sqlite`, with legacy JSON cache fallback/migration
- uses lightweight filesystem watchers plus polling fallback to refresh local project observations

See:

- `../docs/adr/0002-tauri-desktop-companion.md`
- `../docs/adr/0003-observation-refresh-architecture.md`
- `../docs/adr/0004-local-observation-storage.md`
