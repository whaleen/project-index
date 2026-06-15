# ADR 0003: Observation refresh architecture

## Status

Accepted

## Context

The desktop companion is becoming a long-running project/agent dashboard. Users expect it to stay open while work happens across multiple repositories and to update when observed local state changes.

The current app has ad hoc loading paths: initial overview scan, lazy project-specific requests, and GitHub requests triggered by navigation. This is fragile for two reasons:

1. Local state can become stale unless the user reloads the app.
2. Navigating between projects can accidentally create a burst of remote GitHub calls, making the app feel slow or causing macOS spinning-pinwheel behavior.

project-index remains read-only. Refreshing observations must never mutate project files or remote GitHub state.

## Decision

Introduce a production-oriented observation system with a clear split between local observations and remote observations.

The backend owns observation state, refresh policy, caching, and filesystem watches. The frontend renders snapshots and requests refreshes; it does not independently orchestrate expensive scans or GitHub fetches.

## Principles

1. **Local-first and reactive** — local filesystem/git/agent state should update from watches and cheap bounded refreshes.
2. **Remote data is stale-while-revalidate** — cached GitHub data is always safe to show with `last updated` metadata.
3. **Navigation must not stampede** — switching projects should not directly fire unbounded GitHub calls.
4. **Bounded work** — every scanner has debounce, timeout, concurrency, and cancellation/coalescing rules.
5. **Observable freshness** — every rendered resource can expose observed/refreshed time, stale status, and last error.
6. **Read-only forever** — refresh commands observe only; they do not repair, install, mutate, or write project-local files.

## Architecture

### 1. Observation resource model

Represent every refreshable unit as a resource key:

```text
ProjectList
ProjectLocal(project_path)
ProjectGit(project_path)
ProjectReadme(project_path)
ProjectContext(project_path)
ProjectInbox(project_path)
ProjectAgents(project_path)
ProjectIcon(project_path)
GitHubRepo(repo)
GitHubIssues(repo)
AgentLibrary
```

Each resource snapshot carries metadata:

```text
observed_at      local observation time
refreshed_at     remote fetch/cache update time, when applicable
source           filesystem | git | github-cache | github-live
stale            boolean
refreshing       boolean
error            optional last error
```

### 2. Backend observation store

The Tauri backend maintains an in-memory observation store:

- keyed by resource key
- populated at startup from cheap local scans and existing caches
- updated by filesystem watcher events and explicit refresh requests
- emitted to frontend via Tauri events such as `observation://updated`

The frontend requests a single `app_snapshot` at startup, then subscribes to updates.

### 3. Local watchers

Use `notify` to watch:

- configured project roots for repo add/remove
- each observed repo root for:
  - `.git/` state relevant to branch/dirty/latest commit
  - README files
  - context files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.mcp.json`, `SPEC.md`)
  - `.agent/inbox/`
  - project-local skill dirs
  - Tauri/web icon files
- native agent storage roots where practical:
  - `~/.claude/projects/`
  - `~/.codex/memories/`
  - `~/.gemini/GEMINI.md`, `~/.gemini/projects.json`, `~/.gemini/tmp/`
  - `~/.pi/agent/`
  - `~/.agents/skills/`
- agent library root

Watcher events are debounced and coalesced per resource key. A burst of file changes should produce one refresh, not dozens.

Suggested debounce windows:

```text
ProjectGit:      500–1000 ms
ProjectInbox:    300–750 ms
Readme/context:  300–750 ms
Agent sessions:  1000–2000 ms
ProjectList:     1500–3000 ms
```

### 4. Bounded local refresh

Local refreshes run in a bounded work queue:

```text
max local concurrency: 4
per git command timeout: 1–2 seconds
per agent session scan budget: bounded by project + recent files first
```

Codex session scanning must be incremental/cached because historical date-bucket scans can be expensive. The initial implementation can limit the desktop app to recent Codex files first and expose `partial: true` if the full history was not scanned.

### 5. GitHub refresh policy

GitHub resources are remote and must not be tied directly to navigation.

Rules:

- Show cached GitHub data immediately when available.
- Display `last updated at`, `stale`, and `last error` where relevant.
- Manual refresh buttons force a refresh for that repo/resource.
- Automatic refresh is TTL-based and queue-based, never navigation-stampede-based.
- Remote queue concurrency is low:

```text
max github concurrency: 1
minimum delay between calls: 500–1000 ms
repo metadata TTL: 60 minutes
issues TTL: 5–15 minutes
OpenGraph image URL TTL: 24 hours unless manually refreshed
```

Navigation may enqueue a low-priority refresh if data is missing or stale, but the visible UI should continue to render cached/local data.

### 6. Frontend responsibilities

The frontend should:

- render snapshots from backend state
- show freshness metadata on GitHub cards/issues
- show local observation timestamps where helpful
- provide explicit refresh buttons for GitHub repo/issues surfaces
- avoid issuing GitHub fetches in `useEffect` on every project navigation
- keep selected project stable while observations update

### 7. Commands and events

Proposed backend API:

```text
app_snapshot() -> AppSnapshot
refresh_resource(key, force) -> queued status
refresh_project(path) -> local resources only
refresh_github_repo(repo, force) -> queued status
refresh_github_issues(repo, force) -> queued status
```

Proposed events:

```text
observation://updated { key, snapshot }
observation://refresh-started { key }
observation://refresh-finished { key, stale, error }
```

## Migration plan

1. Introduce resource metadata types and include freshness fields in existing app DTOs.
2. Move current ad hoc frontend lazy-loading into backend refresh commands with stale-while-revalidate semantics.
3. Add manual GitHub refresh buttons and last-updated display.
4. Add backend observation store and Tauri events.
5. Add filesystem watchers for local project resources.
6. Add bounded local/remote refresh queues.
7. Add Codex/session incremental cache to avoid historical scans on every project view.
8. Remove frontend navigation-triggered GitHub fetch stampedes.

## Consequences

- The backend becomes the source of truth for observation freshness.
- The app becomes suitable as a long-running dashboard.
- GitHub data may be stale by design, but freshness is visible and manually refreshable.
- The architecture remains read-only and compatible with the CLI/TUI/MCP observer contract.
