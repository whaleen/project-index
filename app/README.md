# project-index desktop companion

Tauri v2 + React + Tailwind CSS v4 + shadcn/ui companion app for iterating on the rich project-index UI.

This app is a read-only observer. It may display local project, context, agent, skills, MCP, and `.agent/inbox/` state, but it must not install, repair, scaffold, or mutate project files.

## Run

```bash
pnpm install
pnpm tauri dev
```

## Current MVP

- scans the configured projects root (`PROJECT_INDEX_PROJECTS_DIR` or `~/Projects`)
- lists git repositories up to two levels deep
- shows context file presence
- observes `.agent/inbox/` docs/schema/feedback JSONL
- summarizes inbox statuses without writing records
- renders a cross-project active inbox dashboard
- renders a cross-project GitHub issues dashboard
- shows selected inbox/issue detail and read-only copyable agent prompts

See `../docs/adr/0002-tauri-desktop-companion.md`.
