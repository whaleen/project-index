# ADR 0002: Add a Tauri desktop companion for product UI discovery

Date: 2026-06-14
Status: accepted

## Context

project-index currently has a Rust CLI/TUI/MCP surface. The TUI is useful for terminal-native inspection, but the product is still discovering its information architecture: projects, git state, context files, agent sessions, skills, MCP configuration, and cross-project workflow signals such as agent inbox records.

Iterating on that shape directly in Ratatui is slow because rich layout, density, navigation, and visual hierarchy are still changing. A browser web app would improve iteration speed, but project-index is inherently local-first: its primary data sources are local repositories and native agent storage under locations such as `~/.claude`, `~/.codex`, `~/.gemini`, `~/.pi/agent`, and `~/.agents`. A browser app would require a separate local daemon/API to read those sources.

## Decision

Add a Tauri desktop companion under `app/` as the primary rich UI discovery surface for project-index.

The desktop app may:

- render project-index observations using React, TypeScript, Tailwind, and shadcn/ui-style components
- call read-only Tauri commands backed by Rust project-index readers
- observe project context files, git/GitHub state, agent sessions, memories, skills, MCP config, and `.agent/inbox/` records
- help refine the product information architecture before proven views are ported back to the Ratatui TUI

The desktop app must preserve the existing read-only contract. It must not install tools, scaffold projects, edit project files, mutate agent config, write inbox records, or otherwise become the project mutation surface.

The existing `px` CLI/TUI/MCP remains supported:

- CLI/MCP: machine-readable and agent-facing inspection
- TUI: fast terminal surface, revisited after the app clarifies the product UI
- Tauri app: rich local dashboard for product/UI iteration

## Consequences

- The repo gains an `app/` workspace containing a Tauri v2 + React frontend.
- Shared reader logic should eventually move out of the TUI-heavy `cli/src/lib.rs` into a reusable core crate so both `cli/` and `app/` can call the same observation code.
- Initial app commands can be thin read-only wrappers while the shared core boundary is extracted.
- Agent inbox installation belongs in the separate `agent-inbox` package/repo. project-index only observes `.agent/inbox/` state and may show copyable commands or docs links.
- Any future one-click install/repair/mutation feature requires a new ADR and SPEC change because it would violate the current observer-only contract.
