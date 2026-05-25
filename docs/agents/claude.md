# Claude Code — Agent Storage Interface

**Source**: docs.anthropic.com, code.claude.com  
**Binary**: `claude`  
**Config root**: `~/.claude/` (overrideable via `CLAUDE_CONFIG_DIR`)

---

## User-Level Directory Layout

```
~/.claude/
  projects/
    <encoded-path>/          # one dir per project CWD
      <uuid>.jsonl           # conversation transcript (JSONL)
      <uuid>/
        tool-results/        # large tool outputs spilled to disk
      memory/                # auto-memory (Claude-written notes)
        MEMORY.md            # index (first 200 lines loaded at session start)
        *.md                 # topic files (loaded on demand)
  CLAUDE.md                  # global instructions (user-written)
  rules/                     # user-level path-scoped instructions
    *.md
  skills/                    # user-level skills
    <name>/
      SKILL.md
  commands/                  # user-level slash commands (legacy; prefer skills)
    <name>.md
  agents/                    # user-level subagent definitions
    <name>.md
  agent-memory/              # user-level subagent memory
    <agent-name>/
      MEMORY.md
  output-styles/             # user-level output styles
    <name>.md
  themes/                    # custom UI themes
    <name>.json
  keybindings.json           # custom keyboard shortcuts
  plugins/
    installed_plugins.json   # active plugin registry
    known_marketplaces.json  # registered plugin marketplaces
    cache/
      <marketplace>/
        <name>/<version>/    # full plugin repo (skills, commands, agents, hooks)
  settings.json              # global settings
  history.jsonl              # all prompts ever typed (never auto-deleted)
  stats-cache.json           # token/cost aggregates
  file-history/              # checkpoint file snapshots (auto-cleaned, 30d)
  plans/                     # plan mode output (auto-cleaned, 30d)
  debug/                     # debug logs (auto-cleaned)
  paste-cache/               # cached pastes (auto-cleaned)
  tasks/                     # session task lists (auto-cleaned)
  shell-snapshots/           # captured shell state (auto-cleaned)
  session-env/               # per-session env metadata (auto-cleaned)
~/.claude.json               # app state: OAuth, MCP servers, trust decisions
```

---

## Project-Level Directory Layout

```
<project-root>/
  CLAUDE.md                  # project instructions (committed to git)
  CLAUDE.local.md            # personal project overrides (gitignored)
  .mcp.json                  # team MCP server definitions (committed)
  .worktreeinclude           # gitignored files to copy into worktrees
  .claude/
    settings.json            # team settings (committed)
    settings.local.json      # personal settings (gitignored)
    CLAUDE.md                # alternative location for project instructions
    rules/                   # path-scoped instruction files
      *.md
    skills/                  # project-level skills
      <name>/
        SKILL.md
    commands/                # project-level slash commands (legacy)
      <name>.md
    agents/                  # project subagent definitions
      <name>.md
    agent-memory/            # project subagent memory (committed)
      <agent-name>/
        MEMORY.md
    agent-memory-local/      # personal subagent memory (gitignored)
    output-styles/
      <name>.md
```

---

## What Claude Code Writes (Agent Default Behavior)

- `~/.claude/projects/<encoded>/memory/` — auto memory from sessions
- `~/.claude/projects/<encoded>/<uuid>.jsonl` — conversation transcripts
- `.claude/agent-memory/<name>/MEMORY.md` — subagent memory
- `~/.claude/history.jsonl` — prompt history (persistent)
- `~/.claude/stats-cache.json` — token/cost stats
- `~/.claude.json` — app state (OAuth, trust, MCP)

Everything under `plans/`, `debug/`, `paste-cache/`, `file-history/`, `tasks/`, `shell-snapshots/`, `session-env/` is auto-cleaned after 30 days.

---

## What Claude Code Reads (Context Discovery)

**CLAUDE.md files** (all loaded, more specific overrides broader):
1. Enterprise managed policy (`/Library/Application Support/ClaudeCode/CLAUDE.md`)
2. User-level: `~/.claude/CLAUDE.md`
3. Project: `<project-root>/CLAUDE.md` or `<project-root>/.claude/CLAUDE.md`
4. Personal project override: `<project-root>/CLAUDE.local.md`

**Rules** (from `.claude/rules/` at user and project level): path-scoped markdown files loaded on demand when matching files are accessed.

**Skills** (discovery order):
1. Enterprise skills
2. Plugin-contributed skills (`~/.claude/plugins/cache/<mp>/<name>/skills/`)
3. User skills (`~/.claude/skills/<name>/SKILL.md`)
4. Project skills (`.claude/skills/<name>/SKILL.md`)

---

## Session File: `<uuid>.jsonl`

One JSON object per line. First line is always `type: "last-prompt"`.

Key line types: `last-prompt`, `user`, `assistant`, `system`, `attachment`, `ai-title`, `file-history-snapshot`, `permission-mode`

**Path encoding**: absolute path with all non-alphanumeric chars → `-`. Two variants exist:
- v1: `_` preserved  
- v2: `_` → `-`  
Always check both when looking up a project.

---

## Auto Memory: `memory/`

Claude-written notes at `~/.claude/projects/<encoded>/memory/`:
- `MEMORY.md` — index, loaded at session start (first 200 lines / 25KB)
- `*.md` — topic files, loaded on demand

User-configurable via `autoMemoryEnabled` and `autoMemoryDirectory` settings.

---

## MCP Servers

| Location | Scope |
|----------|-------|
| `<project-root>/.mcp.json` | Team (committed to git) |
| `~/.claude.json` under `mcpServers` | Personal (all projects) |

---

## Plugins

Registry at `~/.claude/plugins/installed_plugins.json`. Marketplaces in `known_marketplaces.json`. Active plugins toggled in `settings.json` under `enabledPlugins`. Each plugin is a GitHub repo cached to `~/.claude/plugins/cache/<marketplace>/<name>/<version>/` and may contribute skills, commands, agents, and hooks.

---

## Subagents

Definitions at `~/.claude/agents/<name>.md` (user) or `.claude/agents/<name>.md` (project). Persistent memory at `~/.claude/agent-memory/<name>/MEMORY.md`.

---

## Interface Validation Checklist

- [ ] `~/.claude/projects/` exists
- [ ] Project encoded-path dir exists (check v1 and v2 encoding)
- [ ] Session JSONL files: first line `type: "last-prompt"`
- [ ] `memory/` subdir may be absent (no memories yet)
- [ ] `~/.claude/plugins/installed_plugins.json` for plugin list
- [ ] `~/.claude/settings.json` `enabledPlugins` for active plugins
- [ ] CLAUDE.md files exist at expected scopes (project root / `~/.claude/`)
- [ ] `.claude/agents/` for subagent definitions
- [ ] `.claude/rules/` for path-scoped rules
