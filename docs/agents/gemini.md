# Gemini CLI — Agent Storage Interface

**Source**: geminicli.com/docs, github.com/google-gemini/gemini-cli  
**Binary**: `gemini`  
**Config root**: `~/.gemini/`

---

## User-Level Directory Layout

```
~/.gemini/
  GEMINI.md                             # global memory (Gemini-written via save_memory tool)
  settings.json                         # global configuration
  skills/                               # user-level skills
    <name>/
      SKILL.md
  extensions/                           # installed extensions (user scope)
    <extension-name>/
      gemini-extension.json             # manifest (required)
      .env                              # extension config / API keys
      commands/                         # custom slash commands (TOML files)
      hooks/
        hooks.json
      skills/                           # skills bundled with this extension
      agents/                           # sub-agents (preview)
      policies/                         # safety rules
      themes/                           # UI themes
  storage/                              # persistent storage for global tools/extensions
  tmp/
    <project-hash>/                     # SHA-256 of project path (legacy, older sessions)
      chats/
        session-<timestamp>-<id-prefix>.json
    <project-name>/                     # human-readable name (current behavior)
      chats/
        session-<timestamp>-<id-prefix>.json
      tool-outputs/
      logs.json
  history/
    <project-name>/                     # mirrors tmp structure
  projects.json                         # registry: absolute path → project name
  oauth_creds.json
  google_accounts.json
  state.json
  trustedFolders.json
  installation_id
```

---

## Project-Level Directory Layout

```
<project-root>/
  GEMINI.md                  # project context (user-written, committed to git)
  .gemini/
    settings.json            # project configuration
    GEMINI.md                # alternative location for project context
    skills/                  # project-level skills (prefer .agents/skills/)
      <name>/
        SKILL.md
    extensions/              # project-scoped extensions
      <extension-name>/
        gemini-extension.json
        ...

<any-subdirectory>/
  GEMINI.md                  # JIT (just-in-time) context — loaded when files
                             # in that directory are accessed during a session
```

---

## What Gemini CLI Writes (Agent Default Behavior)

- `~/.gemini/GEMINI.md` — global memory appended by the `save_memory` tool (Claude-equivalent: `~/.claude/projects/<enc>/memory/`)
- `~/.gemini/tmp/<project-name>/chats/session-*.json` — conversation sessions
- `~/.gemini/projects.json` — registers path→name mapping on first project open

**Important**: `~/.gemini/GEMINI.md` is the primary persistent memory mechanism. The `save_memory` tool appends facts as bulleted items under a `## Gemini Added Memories` section. It is also user-editable and updated via `/memory add`.

---

## What Gemini CLI Reads (Context Discovery)

**GEMINI.md files** (all loaded; more specific supplements global):
1. `~/.gemini/GEMINI.md` — global context (includes saved memories)
2. `<project-root>/GEMINI.md` or `<project-root>/.gemini/GEMINI.md` — project context
3. `<subdirectory>/GEMINI.md` — JIT context, loaded on demand when files in that dir are accessed

Filename is configurable via `context.fileName` in `settings.json`.

**Skills** (discovery order, higher tiers override lower; within a tier `.agents/` takes precedence over `.gemini/`):
1. Built-in skills (shipped with CLI)
2. Extension-bundled skills
3. User skills: `~/.agents/skills/` (preferred) or `~/.gemini/skills/`
4. Workspace skills: `.agents/skills/` (preferred) or `.gemini/skills/` (project root)

**Auto memory inbox**: After sessions idle ≥3 hours with ≥10 user messages, Gemini drafts new skill files for review via `/memory inbox`. Promoted skills are saved to the project memory directory.

---

## Session File Format

**Format: JSON (single object, not JSONL)**

```json
{
  "sessionId": "<uuid>",
  "projectHash": "<sha256-hex>",
  "startTime": "2026-01-10T07:03:20.031Z",
  "lastUpdated": "2026-01-10T07:05:12.689Z",
  "messages": [
    {
      "id": "<uuid>",
      "timestamp": "...",
      "type": "user",
      "content": "..."
    },
    {
      "id": "<uuid>",
      "timestamp": "...",
      "type": "gemini",
      "content": "",
      "toolCalls": [ ... ],
      "status": "success"
    }
  ]
}
```

Message types: `user`, `gemini`  
Tool calls are embedded in `gemini` messages as a `toolCalls` array.

**Filename**: `session-<startTime-with-dashes>-<first8-of-sessionId>.json`

---

## Project Lookup

1. Read `~/.gemini/projects.json` to get the `name` for a given absolute path
2. Check `~/.gemini/tmp/<name>/chats/` — current location (human-readable)
3. Also check `~/.gemini/tmp/<sha256(path)>/chats/` — legacy location for older sessions

If a project has never been opened with Gemini, it won't be in `projects.json` and no session directory will exist.

---

## Extensions

Extensions are installed per-scope into `~/.gemini/extensions/<name>/` (user) or `.gemini/extensions/<name>/` (project). The manifest `gemini-extension.json` is required. Update with `gemini extensions update`.

---

## Settings: `settings.json`

```json
{
  "context": { "fileName": "GEMINI.md" },
  "mcpServers": { ... },
  "memory": { "maxAge": 30, "maxCount": 50 }
}
```

---

## Interface Validation Checklist

- [ ] `~/.gemini/projects.json` exists for project name lookup
- [ ] `~/.gemini/tmp/<name>/chats/` exists (current) OR `~/.gemini/tmp/<sha256>/chats/` (legacy)
- [ ] Session files have `.json` extension (not `.jsonl`), single JSON object
- [ ] `~/.gemini/GEMINI.md` exists (Gemini-written memories; may be absent if no saves yet)
- [ ] `GEMINI.md` at project root (user-written context; may not exist)
- [ ] `~/.gemini/skills/` or `~/.agents/skills/` for user skills
- [ ] `~/.gemini/extensions/` for installed extensions
