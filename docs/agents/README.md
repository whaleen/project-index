# Agent Storage Interface Docs

Reference documents for how supported agents store data locally. These docs guide project-index's read-only storage readers.

project-index observes native storage. It should not copy agent data into project-local directories or ask agents to change their storage conventions.

## Supported Agents

| Agent | Binary | Sessions | Memory / config | Skills |
|-------|--------|----------|-----------------|--------|
| Claude Code | `claude` | `~/.claude/projects/<encoded>/` JSONL | `~/.claude/projects/<encoded>/memory/` | `~/.claude/skills/` + plugins |
| Codex | `codex` | `~/.codex/sessions/YYYY/MM/DD/` JSONL | `~/.codex/memories/<repo-name or sanitized-path>/` | `~/.codex/skills/` |
| Gemini CLI | `gemini` | `~/.gemini/tmp/<project-name>/chats/` JSON, from `~/.gemini/projects.json` | `~/.gemini/GEMINI.md` | `~/.gemini/skills/` + `~/.agents/skills/` |
| Pi | `pi` | `~/.pi/agent/sessions/<encoded>/` JSONL | `~/.pi/agent/` config/session state | `~/.pi/agent/skills/` + `~/.agents/skills/` |

## Shared Ecosystem

Skills are cross-agent. See `shared.md` for the `~/.agents/skills/` directory format, `SKILL.md` spec, `npx skills` toolchain, and per-agent behavior table.

## Maintenance Flow

When an agent updates its storage format:

1. Validate against real files on disk.
2. Update the relevant `docs/agents/<agent>.md` doc.
3. Update the corresponding reader in the relevant module under `cli/src/` (agent.rs, project.rs, skills.rs).
4. Keep behavior read-only.

Reader functions:

- Claude: `claude_project_dirs`, `resolve_sessions`, `claude_memory_path`
- Codex: `import_codex_sessions`, `parse_codex_session`, `codex_memory_dirs`
- Gemini: `import_gemini_sessions`, `gemini_memory_path`
- Pi: `pi_encode_path`, `import_pi_sessions`

## Path Encoding Summary

| Agent | Rule | Example (`/Users/josh/Projects/_foo`) |
|-------|------|---------------------------------------|
| Claude | checks two historical encodings: `/` → `-`, and `/` + `_` → `-` | `-Users-josh-Projects-_foo` or `-Users-josh-Projects--foo` |
| Codex | date-bucketed; match by `cwd` field in session meta | n/a |
| Gemini | project name from `~/.gemini/projects.json`; legacy SHA-256 dirs still need support | `astrds` |
| Pi | strip leading `/`, replace `/` with `-`, wrap in `--` | `--Users-josh-Projects-_foo--` |

## Session Format Summary

| Agent | File format | Project match method |
|-------|-------------|----------------------|
| Claude | JSONL | encoded path directory |
| Codex | JSONL | first-line `session_meta.payload.cwd` |
| Gemini | JSON | project mapping + chat JSON |
| Pi | JSONL | encoded path directory + first-line `cwd` |
