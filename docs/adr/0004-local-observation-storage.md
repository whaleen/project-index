# ADR 0004: Local observation storage

## Status

Accepted

## Context

The desktop companion is a long-running local dashboard. It observes project, git, agent, and GitHub state, but the app currently mixes in-memory snapshots with ad hoc JSON files under `~/.project-index/cache/`.

The product remains read-only. project-index must not become source-of-truth for project state, prompts, memory, inboxes, or remote state. Persistent storage may only cache observations and freshness metadata.

Cloud storage is explicitly out of scope at this stage.

## Decision

Use a local SQLite database as the durable observation cache for the desktop companion and future shared read-only surfaces.

Default path:

```text
~/.project-index/project-index.sqlite
```

SQLite stores cached observations, resource freshness metadata, and remote/GitHub data. Source-of-truth data continues to live in native locations: repositories, git, `.agent/inbox/`, native agent directories, and GitHub.

## Principles

1. **Local-only** — no cloud, no hosted sync, no account requirement.
2. **Observation cache, not authority** — SQLite stores what project-index last observed.
3. **Read-only relative to projects/remotes** — writes are limited to project-index-owned cache files under `~/.project-index/`.
4. **Resource-keyed** — storage mirrors ADR 0003 resource keys and freshness metadata.
5. **Migration-friendly** — schema changes are explicit migrations via `PRAGMA user_version` or equivalent.
6. **Fast startup** — app can hydrate cached observations quickly and revalidate asynchronously.

## Initial schema

```sql
CREATE TABLE observations (
  resource_key TEXT PRIMARY KEY,
  resource_type TEXT NOT NULL,
  project_path TEXT,
  source TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  observed_at INTEGER NOT NULL,
  stale_after INTEGER,
  last_error TEXT
);

CREATE TABLE github_repos (
  repo TEXT PRIMARY KEY,
  fetched_at INTEGER NOT NULL,
  payload_json TEXT NOT NULL,
  last_error TEXT
);

CREATE TABLE github_issues (
  repo TEXT PRIMARY KEY,
  fetched_at INTEGER NOT NULL,
  payload_json TEXT NOT NULL,
  last_error TEXT
);

CREATE TABLE observation_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  occurred_at INTEGER NOT NULL,
  source TEXT NOT NULL,
  resource_key TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  project_path TEXT,
  action TEXT NOT NULL,
  status TEXT NOT NULL,
  message TEXT,
  detail_json TEXT
);
```

## Migration plan

1. Add SQLite dependency and initialize `~/.project-index/project-index.sqlite`.
2. Store GitHub repo/issues caches in SQLite, while preserving read-only JSON cache fallback for existing users.
3. Store app/project observation snapshots by resource key.
4. Store bounded observation activity events for the desktop Activity page.
5. Hydrate startup from SQLite where useful, then refresh via watchers/commands.
5. Retire JSON cache writes after confidence period; keep JSON read fallback longer if harmless.

## Consequences

- There is one durable local observation cache instead of scattered JSON files.
- GitHub cache and local observation freshness can share consistent metadata.
- The app remains local-first and read-only with respect to observed projects and remotes.
