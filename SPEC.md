# project-index — Product Contract

`SPEC.md` is the lifecycle contract for this project: what project-index must be, what it must not become, and what behavior should be preserved as the code changes.

## Product Thesis

project-index is a read-only work index for humans and agents. It gives one terminal surface for observing local projects, git state, native agent context, native agent sessions, skills, MCP config, and related system state.

project-index is not a scaffold tool, setup wizard, prompt manager, memory manager, or agent configuration writer.

See `docs/adr/0001-read-only-work-index.md` and `docs/adr/0002-tauri-desktop-companion.md`.

## Non-Negotiable Constraints

- ✅ Read-only by default and by design
- ✅ No project scaffolding or initialization
- ✅ No creating, editing, deleting, resetting, repairing, or migrating project files
- ✅ No prompt storage or prompt-template management
- ✅ No project-index-owned project state as source of truth
- ✅ Observe `SPEC.md`; do not enforce or maintain it from project-index
- ✅ MCP/CLI surfaces are inspection-only

## Product Surfaces

- ✅ `px` CLI/TUI/MCP remains the fast terminal and agent-facing inspection surface
- 📋 Tauri desktop companion under `app/` for rich local project/agent dashboard iteration
- 📋 Use the desktop companion to refine information architecture before porting proven views back to the Ratatui TUI
- ❌ Desktop app install/repair/scaffold/mutation flows without a new ADR and explicit contract change

## Observed Project Index

- ✅ Scan project root up to 2 levels deep for git repositories
- ✅ Group projects by parent directory
- ✅ Show repo name, branch, dirty count, ahead/behind counts
- ✅ Rescan on demand
- 📋 Search/filter projects
- 📋 Pin/favorite projects
- 📋 Configurable scan depth
- 📋 Include worktrees and detect generated agent worktrees distinctly

## Git / Repository Surface

- ✅ Recent commits
- ✅ Dirty file list on project home
- ✅ Ahead/behind counts
- 📋 Dedicated Git tab with dirty files, stash count, remotes, and branch details
- 📋 Surface repos requiring attention across all tracked projects

## GitHub Surface

project-index should become a read-only terminal mirror of the essential GitHub repository surfaces normally checked in the browser, while preserving the observer-only contract.

- ✅ Read GitHub description, homepage, repo URL, topics, stars/forks/license/open PR counts through `gh`
- ✅ Read open issues through `gh`
- 📋 Clear degraded state when `gh` is unavailable or unauthenticated
- ✅ Read-only issue list in desktop dashboard/project surfaces
- 📋 Read-only issue detail and issue search/filter
- 📋 Read open pull requests with status/check summaries and changed-file previews
- 📋 Read GitHub Actions workflow/run status
- 📋 Read releases, tags, labels, milestones, default branch, and repository metadata
- 📋 Copy/open GitHub URLs without mutating GitHub state
- ❌ Creating, editing, closing, merging, labeling, assigning, dispatching, or otherwise mutating GitHub resources

## Context Surface

Observe, never create or repair:

- ✅ `CLAUDE.md`
- ✅ `AGENTS.md`
- ✅ `GEMINI.md`
- ✅ `.mcp.json`
- 📋 `SPEC.md`
- ✅ `docs/`
- ✅ `docs/adr/` as the standard architecture decision record location
- 📋 `.pi/settings.json`, `.pi/agents/`, `.pi/chains/`
- 📋 `.agents/skills/`, `skills-lock.json`
- 📋 `.agent/inbox/README.md`, `.agent/inbox/schema.md`, `.agent/inbox/feedback.jsonl`
- 📋 stale/legacy markers such as `.pemguin/`, `.memory/`, `.prompts/`, `.cntx/`

## Agent Sessions Surface

- ✅ Claude Code sessions from `~/.claude/projects/<encoded>/`
- ✅ Codex sessions from `~/.codex/sessions/YYYY/MM/DD/`
- ✅ Pi sessions from `~/.pi/agent/sessions/<encoded>/`
- ✅ Gemini sessions from `~/.gemini/projects.json` + `~/.gemini/tmp/<project>/chats/`
- ✅ Inline summary for Claude and Pi JSONL sessions
- 📋 Gemini session summary viewer
- 📋 Legacy Gemini SHA-256 session dirs
- 📋 Cache/index Codex scans incrementally
- 📋 Cross-project global session view

## Agent Memory / Config Surface

Observe native locations only:

- ✅ Claude memory from `~/.claude/projects/<encoded>/memory/`
- ✅ Codex memory from `~/.codex/memories/<repo-name or sanitized-path>/`
- ✅ Gemini global memory from `~/.gemini/GEMINI.md`
- 📋 Global config summary for `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, `~/.agents`
- 📋 Read-only preview of relevant memory/config files

## Skills / MCP Surface

- ✅ Project `.mcp.json` read-only server list
- ✅ Project `skills-lock.json` + `.agents/skills/` reader
- 📋 Merge global `~/.agents/.skill-lock.json`
- 📋 Merge per-agent skill dirs: `~/.claude/skills`, `~/.codex/skills`, `~/.gemini/skills`, `~/.pi/agent/skills`
- 📋 Read global Claude MCP/config where available

## Removed / Legacy Behavior

These behaviors are intentionally out of scope and should be removed when encountered:

- ❌ `.pemguin/` project setup state
- ❌ `.pemguin/sessions.toml` registry
- ❌ `.pemguin/memory` or `.memory` memory sync
- ❌ `.pemguin/prompts`, `.prompts`, or bundled prompt management
- ❌ setup/apply/delete/reset/repair flows
- ❌ MCP install/repair/edit/delete flows
- ❌ template scaffolding for project docs/context

## Agent Inbox Surface

project-index observes local agent inbox installations as workflow state. Installation and mutation belong to the separate `agent-inbox` package/skill, not project-index.

- 📋 Detect `.agent/inbox/` per project
- 📋 Summarize active record counts by status: `new`, `planned`, `accepted`, `in_progress`, `done`, `wontfix`
- 📋 Read active records from `.agent/inbox/feedback.jsonl` when present
- 📋 Surface missing inbox docs/schema as observation-only project health
- 📋 Show copyable prompts or install commands without executing them
- ❌ Creating, editing, deleting, planning, accepting, or marking inbox records from project-index
- ❌ Installing agent-inbox into projects from project-index without a new ADR/contract change

## Architecture Contract

- ✅ Split `cli/src/lib.rs` by concern: config, project scan, agent readers, git/GitHub readers, app state, tab renderers, key handlers, CLI/MCP.
- 📋 Add reusable read-only core boundary shared by `cli/` and `app/`
- 📋 Desktop app refresh must follow `docs/adr/0003-observation-refresh-architecture.md`: backend-owned observation store, local filesystem watchers, bounded refresh queues, stale-while-revalidate GitHub data, visible freshness metadata, and no navigation-triggered GitHub stampedes.
- 📋 Desktop app local persistence must follow `docs/adr/0004-local-observation-storage.md`: SQLite under `~/.project-index/` is an observation cache only, never source-of-truth for project files, prompts, memory, inboxes, or GitHub state.
- 📋 Keep native agent storage docs current under `docs/agents/`.
- 📋 Add regression checks for read-only behavior: no UI/CLI/MCP/app path should write project files.

## Known Issues




- Codex session scan can be slow on large histories.
- Project selection resets after full rescan.
