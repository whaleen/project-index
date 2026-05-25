# project-index

One terminal surface for observing everything an AI-assisted dev workflow touches — projects, git state, agent sessions, memories, skills, and MCP config. Read-only and observer-first.

`px` is a Ratatui TUI built in Rust. It reads from where git and agents naturally store things (native `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi` storage) and never writes project files.

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

## Tabs

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
root = "~/Projects"   # scanned 2 levels deep for .git dirs

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

Set `PROJECT_INDEX_PROJECTS_DIR` to override `projects.root` via env.

`~/.project-index/` holds observation cache (GitHub metadata, avatars).

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
  docs/adr/         architecture decisions
  docs/agents/      native agent storage docs
  .github/workflows/release.yml
```
