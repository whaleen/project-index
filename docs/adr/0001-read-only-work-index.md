# ADR 0001: project-index is a read-only work index

Date: 2026-05-25
Status: accepted

## Context

project-index has carried several product shapes: project bootstrapper, prompt/memory manager, terminal project manager, and agent session dashboard. Those histories left project-owned state (`.pemguin/`, `.memory/`, `.prompts/`), setup templates, and mutation paths in the codebase.

The useful enduring shape is an observation surface: a single local index of projects, git state, native agent context, native agent sessions, skills, MCP config, and other agent-related system state.

## Decision

project-index is a read-only work index for humans and agents.

project-index may:

- discover local git repositories
- read git state, GitHub metadata, issues, and docs
- read native agent context files such as `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `SPEC.md`, and `.mcp.json`
- read native agent storage under locations such as `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, and `~/.agents`
- cache observation-only presentation data such as GitHub metadata, avatars, and desktop observation snapshots under `~/.project-index/`
- expose read-only MCP/CLI inspection tools

project-index must not:

- scaffold or initialize projects
- create, edit, delete, reset, repair, or migrate project files
- install or remove MCP servers, skills, prompts, or agent config
- maintain project-local project-index state as a source of truth
- store or manage prompts
- sync agent memories into project-index-owned directories

`SPEC.md` remains a valuable project contract artifact, but project-index only observes it. Contract enforcement belongs in agent workflows/skills, not in project-index.

## Consequences

- Project-local `.pemguin/`, `.memory/`, and `.prompts/` are legacy artifacts, not canonical storage.
- The Prompt manager and setup/apply/reset/delete flows should be removed or replaced with read-only views.
- The Config tab should become a Context tab: present/missing/stale observations only.
- MCP tools should be inspection-only.
- Docs and code should prefer native agent storage readers over project-index-owned registries.
