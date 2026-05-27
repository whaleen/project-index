---
name: project-index-release
description: Release or locally publish project-index (`px`). Use when the user explicitly asks to release, publish, bump, tag, prepare a project-index version, or install the current checkout locally for testing.
---

# project-index Release

Use this skill to release `project-index` / `px`, or to locally publish the current checkout for quick iteration. The public release pipeline is already in place; follow it exactly and do not improvise.

## Non-negotiable safety

- Distinguish **local publish** from **public release** before acting.
- Local publish means installing the current checkout's `px` binary locally for testing. It must not bump versions, commit, tag, push, edit GitHub Releases, or touch the Homebrew tap.
- Never public-release without explicit user confirmation of the target version.
- Never tag or push tags until the version bump commit is correct and the user has confirmed.
- Require a clean working tree except for the intentional version bump before creating a public release commit. A local publish may run from a dirty tree when the user is intentionally testing local edits.
- Do not edit GitHub Actions, Homebrew tap files, secrets, or release artifacts unless the user explicitly asks to fix the pipeline.
- Do not use `npm`, `pnpm`, or Tauri release assumptions. This project is a Rust CLI/TUI released through Cargo + GitHub Actions + Homebrew.

## Release facts

- Crate/app version lives in `cli/Cargo.toml`.
- Binary names: `px` and `project-index`.
- Release tags use `vX.Y.Z`.
- Main branch is `master`.
- GitHub Actions builds macOS binaries for:
  - `aarch64-apple-darwin`
  - `x86_64-apple-darwin`
- GitHub Releases receive tarballs named like `project-index-vX.Y.Z-<target>.tar.gz`.
- The release workflow updates `whaleen/homebrew-tap` after successful build so `brew upgrade project-index` works.
- The Homebrew formula is in the tap repo, not this repo.

## Local publish for iteration

Use this path when the user wants to restart `px` and see current local edits without using the public release pipeline.

From repo root:

```bash
cargo check --manifest-path cli/Cargo.toml
cargo install --path cli --bin px --force
```

Then restart `px` from the shell. This installs to Cargo's bin directory, usually `~/.cargo/bin/px`. If the user's shell resolves a different binary first, check:

```bash
which px
px --version || px --help
```

Do not commit, tag, push, bump versions, or update Homebrew for a local publish unless the user separately asks for a public release.

## Public release preflight

From repo root:

```bash
git status --short
git branch --show-current
grep '^version' cli/Cargo.toml
cargo check --manifest-path cli/Cargo.toml
```

If the working tree is dirty for unrelated reasons, stop and ask the user how to handle it. Do not mix unrelated work into a release commit.

## Version bump

1. Ask/confirm the exact semantic version, e.g. `0.2.2`.
2. Edit only `cli/Cargo.toml` for the version bump unless the user explicitly asks for more.
3. Run validation:

```bash
cargo check --manifest-path cli/Cargo.toml
```

4. Inspect the diff:

```bash
git diff -- cli/Cargo.toml
git status --short
```

5. Commit the version bump:

```bash
git add cli/Cargo.toml
git commit -m "chore: bump to vX.Y.Z"
```

## Tag and publish

Only after the user confirms the release commit and version:

```bash
git tag vX.Y.Z
git push origin master --tags
```

If the commit was already pushed separately, pushing the tag is sufficient:

```bash
git push origin vX.Y.Z
```

## Verify release

After pushing:

1. Watch GitHub Actions for `whaleen/project-index`.
2. Confirm the GitHub Release exists for `vX.Y.Z`.
3. Confirm both macOS artifacts exist.
4. Confirm `whaleen/homebrew-tap` was updated by the workflow.
5. Once Homebrew has updated locally:

```bash
brew update
brew upgrade project-index
px --version || px --help
```

## Failure handling

- If the workflow fails before publishing artifacts, inspect logs and fix only the failing pipeline issue.
- If a tag points at the wrong commit, stop and ask before deleting/recreating tags.
- If a GitHub Release or Homebrew tap update partially succeeded, do not overwrite/delete anything without explicit approval.
- If the Homebrew tap update fails, inspect the tap repo state and workflow logs before retrying.
