# ADR 0001: Pemguin is a read-only work index

Date: 2026-05-25
Status: accepted

## Context

Pemguin has carried several product shapes: project bootstrapper, prompt/memory manager, terminal project manager, and agent session dashboard. Those histories left project-owned state (`.pemguin/`, `.memory/`, `.prompts/`), setup templates, and mutation paths in the codebase.

The useful enduring shape is an observation surface: a single local index of projects, git state, native agent context, native agent sessions, skills, MCP config, and other agent-related system state.

## Decision

Pemguin is a read-only work index for humans and agents.

Pemguin may:

- discover local git repositories
- read git state, GitHub metadata, issues, and docs
- read native agent context files such as `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `SPEC.md`, and `.mcp.json`
- read native agent storage under locations such as `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, and `~/.agents`
- cache observation-only presentation data such as GitHub metadata and avatars under `~/.pemguin`
- expose read-only MCP/CLI inspection tools

Pemguin must not:

- scaffold or initialize projects
- create, edit, delete, reset, repair, or migrate project files
- install or remove MCP servers, skills, prompts, or agent config
- maintain project-local Pemguin state as a source of truth
- store or manage prompts
- sync agent memories into Pemguin-owned directories

`SPEC.md` remains a valuable project contract artifact, but Pemguin only observes it. Contract enforcement belongs in agent workflows/skills, not in Pemguin.

## Consequences

- Project-local `.pemguin/`, `.memory/`, and `.prompts/` are legacy artifacts, not canonical storage.
- The Prompt manager and setup/apply/reset/delete flows should be removed or replaced with read-only views.
- The Config tab should become a Context tab: present/missing/stale observations only.
- MCP tools should be inspection-only.
- Docs and code should prefer native agent storage readers over Pemguin-owned registries.
