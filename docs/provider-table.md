# Provider Compatibility Table

What project-index observes, per agent. Updated alongside `docs/agents/<agent>.md`.

**Status key**: ✅ implemented · 📋 planned · ❓ unknown (format not yet researched) · ➖ not applicable

---

## Binaries & Install

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build |
|---|---|---|---|---|---|
| Binary | `claude` | `codex` | `gemini` | `pi` | `grok` |
| Install | `npm i -g @anthropic-ai/claude-code` | `npm i -g @openai/codex` | `npm i -g @google/gemini-cli` | `npm i -g @pi-agent/pi` | `curl \| bash` |
| Config root | `~/.claude/` | `~/.codex/` | `~/.gemini/` | `~/.pi/agent/` | `~/.grok/` |

---

## Sessions

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build |
|---|---|---|---|---|---|
| Location | `~/.claude/projects/<enc>/` | `~/.codex/sessions/YYYY/MM/DD/` | `~/.gemini/tmp/<name>/chats/` | `~/.pi/agent/sessions/<enc>/` | `~/.grok/` (subdir TBD) |
| Format | JSONL | JSONL | JSON | JSONL | ❓ |
| px reads | ✅ | ✅ | ✅ | ✅ | 📋 |
| Inline summary | ✅ | ➖ | 📋 | ✅ | 📋 |
| Path encoding | `/`→`-` (2 variants) | match by `cwd` field | `projects.json` name lookup | strip `/`, wrap `--` | ❓ |

---

## Memory

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build |
|---|---|---|---|---|---|
| Location | `~/.claude/projects/<enc>/memory/` | `~/.codex/memories/<repo>/` | `~/.gemini/GEMINI.md` | `~/.pi/agent/` | `~/.grok/` (experimental) |
| Format | Markdown files | Markdown files | Single `.md` | ❓ | ❓ |
| px reads | ✅ | ✅ | ✅ | 📋 | 📋 |

---

## Global Config

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build |
|---|---|---|---|---|---|
| Settings | `~/.claude/settings.json` | ❓ | `~/.gemini/settings.json` | ❓ | `~/.grok/config.toml` |
| App state / auth | `~/.claude.json` | ❓ | `~/.gemini/oauth_creds.json` | ❓ | ❓ |
| Global instructions | `~/.claude/CLAUDE.md` | ➖ | `~/.gemini/GEMINI.md` | ❓ | ❓ |
| px reads | 📋 | 📋 | 📋 | 📋 | 📋 |

---

## Skills

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build | Shared |
|---|---|---|---|---|---|---|
| User skills | `~/.claude/skills/` | `~/.codex/skills/` | `~/.gemini/skills/` | `~/.pi/agent/skills/` | ❓ | `~/.agents/skills/` |
| Project skills | `.claude/skills/` | ❓ | `.gemini/skills/` | ❓ | ❓ | `.agents/skills/` |
| px reads | 📋 | 📋 | 📋 | 📋 | 📋 | ✅ (via `skills-lock.json`) |

---

## MCP Config

| | Claude Code | Codex | Gemini CLI | Pi | Grok Build |
|---|---|---|---|---|---|
| Project MCP | `.mcp.json` | ➖ | `.gemini/settings.json` | ➖ | `.grok/sandbox.toml` |
| Global MCP | `~/.claude.json` `mcpServers` | ➖ | `~/.gemini/settings.json` | ➖ | `~/.grok/config.toml` |
| px reads | ✅ `.mcp.json` | ➖ | 📋 | ➖ | 📋 |

---

## Project Context Files

| File | Claude Code | Codex | Gemini CLI | Pi | Grok Build | px reads |
|---|---|---|---|---|---|---|
| `CLAUDE.md` / `AGENTS.md` | ✅ | ✅ | ➖ | ✅ | ✅ | ✅ |
| `GEMINI.md` | ➖ | ➖ | ✅ | ➖ | ➖ | ✅ |
| `SPEC.md` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `.mcp.json` | ✅ | ➖ | ➖ | ➖ | ➖ | ✅ |
| `skills-lock.json` | ➖ | ➖ | ➖ | ✅ | ➖ | ✅ |
| `GROK.md` | ➖ | ➖ | ➖ | ➖ | ❓ | 📋 |

---

## Plugins / Extensions

| | Claude Code | Gemini CLI | Others |
|---|---|---|---|
| Registry | `~/.claude/plugins/installed_plugins.json` | `~/.gemini/extensions/<name>/` | ➖ |
| px reads | 📋 | 📋 | ➖ |

---

## Subagents

| | Claude Code | Gemini CLI | Others |
|---|---|---|---|
| Definitions | `~/.claude/agents/` · `.claude/agents/` | ❓ | ➖ |
| Memory | `~/.claude/agent-memory/` | ❓ | ➖ |
| px reads | 📋 | 📋 | ➖ |

---

*See `docs/agents/<agent>.md` for full storage interface details and path encoding rules.*
