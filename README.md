# project-index

One local dashboard for observing everything an AI-assisted dev workflow touches — projects, git state, GitHub issues/repo metadata, agent inbox records, sessions, memories, skills, MCP config, and reusable agent library assets. Read-only and observer-first.

`px` started as a Ratatui TUI built in Rust. It reads from where git and agents naturally store things (native `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi` storage) and never writes project files.

The repo now also includes a Tauri desktop companion under `app/`. The desktop app is the rich UI discovery surface for project-index: project cards sorted by latest commit, per-project dashboards, GitHub/OpenGraph cards, Agent Inbox views, Memories/Agents tabs, and a separate Agent Library page. It follows the same observer-first contract: rich local dashboard, no project mutation.

## Install

### Homebrew (macOS)

```bash
brew tap whaleen/tap
brew install project-index
```

### From source

```bash
git clone https://github.com/whaleen/project-index
cd project-index/cli
cargo install --path .
```

Requires: Rust stable, `gh` CLI (for GitHub metadata/issues), Nerd Font terminal.

## Usage

```bash
px
```

Navigate with `↑↓` or `jk`. `enter` opens a project. `esc` goes back. `q` quits.

## Desktop companion

```bash
cd app
pnpm install
pnpm tauri dev

# production validation
pnpm build
cd src-tauri && cargo check
```

The desktop companion is the rich UI discovery surface for project-index. Use it to iterate on project dashboards, context health, agent sessions, skills/MCP, GitHub issue/repo views, Agent Inbox observations, and Agent Library inventory before porting proven views back to the TUI.

Refresh architecture is defined in [`docs/adr/0003-observation-refresh-architecture.md`](./docs/adr/0003-observation-refresh-architecture.md): local observations should be filesystem-watch driven and bounded; GitHub data should be stale-while-revalidate with visible `last updated` metadata and explicit refresh buttons.

Local desktop storage is defined in [`docs/adr/0004-local-observation-storage.md`](./docs/adr/0004-local-observation-storage.md): project-index uses a local SQLite observation cache at `~/.project-index/project-index.sqlite`, while project files, native agent files, and GitHub remain the source of truth. The desktop Activity page reads this cache to show recent observation events.

Desktop pages:

| Page | What it shows |
|------|---------------|
| Dashboard | Detailed project cards sorted by latest commit, plus cross-project Agent Inbox and GitHub issue tabs |
| Agent Library | Unified prompts, recipes, and skills from `/Users/josh/Projects/_whaleen/agent-library` |
| Project Overview | Repo/app identity, GitHub OpenGraph card, README, suggested actions, git health, Agent Inbox and GitHub summaries |
| Project Agent Inbox | Read-only inbox records with copyable planning/implementation prompts |
| Project Context | Observed context files (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json`) |
| Project Memories | Native Claude/Codex/Gemini/Pi memory/config file previews |
| Project Agents | Project-local skills, `.mcp.json` servers, and native agent sessions |
| Project GitHub | GitHub repo metadata and open issues |

## TUI tabs

| Key | Tab | What it shows |
|-----|-----|---------------|
| `1` | Home | Repo identity, README, recent commits, git/GitHub summary |
| `2` | Issues | Open GitHub issues via `gh` |
| `3` | Config | Observed context files: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.mcp.json` |
| `4` | Memories | Native agent memory/config surfaces |
| `5` | Agents | MCP servers, skills, and per-project sessions |
| `6` | Pane | Terminal launcher surface |

## Sessions

Discovered from each agent's native storage — no registration or sync required.

| Agent | Storage |
|-------|---------|
| Claude Code | `~/.claude/projects/<encoded>/` JSONL |
| Codex | `~/.codex/sessions/YYYY/MM/DD/` JSONL |
| Gemini CLI | `~/.gemini/tmp/<project>/chats/` JSON |
| Pi | `~/.pi/agent/sessions/<encoded>/` JSONL |

## Memories

Observed in-place. Nothing is copied or synced.

| Agent | Location |
|-------|----------|
| Claude Code | `~/.claude/projects/<encoded>/memory/` |
| Codex | `~/.codex/memories/<repo-name>/` |
| Gemini CLI | `~/.gemini/GEMINI.md` |
| Pi | `~/.pi/agent/` |

## Configuration

`~/.project-index.toml`:

```toml
[projects]
roots = ["~/code", "~/work", "~/personal"]  # watch as many dirs as you want

[theme]
accent  = "#e8b887"   # hot-reloaded on file change
sel_fg  = "#101010"
fg_dim  = "#A0A0A0"
fg_xdim = "#7E7E7E"
green   = "#90b99f"
red     = "#f5a191"
yellow  = "#e6b99d"
purple  = "#aca1cf"
```

Each root is scanned 2 levels deep for `.git` directories. Set `PROJECT_INDEX_PROJECTS_DIR` to override via env (single path).

`~/.project-index/` holds observation cache (GitHub issues, GitHub repo metadata/OpenGraph data, avatars).

## MCP server

`px` speaks stdio MCP for agent use:

```bash
px mcp serve
```

Tools: `px_project_inspect`, `px_agent_instructions`, `px_setup_plan`

## Supported agents

| Agent | Sessions | Memory | Skills |
|-------|----------|--------|--------|
| Claude Code | `~/.claude/projects/` | `~/.claude/projects/<enc>/memory/` | `~/.claude/skills/` |
| Codex | `~/.codex/sessions/` | `~/.codex/memories/` | `~/.codex/skills/` |
| Gemini CLI | `~/.gemini/tmp/` | `~/.gemini/GEMINI.md` | `~/.agents/skills/` |
| Pi | `~/.pi/agent/sessions/` | `~/.pi/agent/` | `~/.pi/agent/skills/` |

## Releasing

Releases are built and published automatically via GitHub Actions when a version tag is pushed. The Homebrew tap is updated automatically after each successful build.

```bash
# 1. Bump version in cli/Cargo.toml
# 2. Commit and tag
git add cli/Cargo.toml
git commit -m "chore: bump to vX.Y.Z"
git tag vX.Y.Z
git push origin master --tags
```

The workflow builds binaries for `aarch64-apple-darwin` and `x86_64-apple-darwin`, publishes them as a GitHub Release, and pushes the updated SHA256s to `whaleen/homebrew-tap` so `brew upgrade project-index` picks up the new version automatically.

## Project structure

```
project-index/
  cli/              Rust source (bin: px)
  app/              Tauri + React desktop companion
  docs/adr/         architecture decisions
  docs/agents/      native agent storage docs
  .github/workflows/release.yml
```
