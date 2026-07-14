# Contributing to scaffold-stacks

Thanks for helping improve `stacksdapp`. This guide covers local development, checks we expect before a PR, and how releases are cut.

## Prerequisites

| Tool | Notes |
|---|---|
| Rust 1.75+ | via [rustup](https://rustup.rs) |
| Node.js 20+ | frontend + Vitest contract tests |
| Clarinet 3.21+ | contract toolchain (`brew install clarinet` or CI-style install) |
| Docker Desktop | only for local `stacksdapp dev` (devnet) |

Run `stacksdapp doctor` after building to verify your machine.

## Setup

```bash
git clone https://github.com/scaffold-stack/scaffold-stack.git
cd scaffold-stack
cargo build -p stacksdapp
./target/debug/stacksdapp doctor
```

## Day-to-day checks

```bash
# Unit / lib tests across the workspace
cargo test --all

# Fast CI-shaped smoke (doctor, name rejection, new → check → generate → add → test)
bash scripts/ci-smoke.sh
```

## Project layout

- `cli/` — `stacksdapp` binary (clap, exit codes, command dispatch)
- `crates/shell/` — verbosity / quiet / color / JSON + root discovery
- `crates/scaffold/` — `new` / `init` / `add` / `upgrade` + frontend template
- `crates/codegen/`, `parser/`, `deployer/`, `watcher/`, `process_supervisor/` — domain crates

Prefer small, focused PRs. Match existing naming and error style; use `CliError` (or messages classified in `cli/src/error.rs`) when adding failure paths scripts need to distinguish.

## Exit codes

Scripts should rely on stable codes documented in the README (project `2`, prerequisite/`doctor` `3`, aborted `4`, validation `5`, check `6`, test `7`, deploy `8`, generate `10`).

## Releases

1. Bump crate versions in the relevant `Cargo.toml` files (CLI version is the user-facing release).
2. Run `bash scripts/check-versions.sh` and update `CHANGELOG.md`.
3. Commit on a clean tree, tag `vX.Y.Z` matching `cli` version, and push the tag.
4. GitHub Actions **Release** builds platform binaries and attaches them to a GitHub Release.
5. Publish crates.io with `./pub.sh` (clean worktree by default; `--allow-dirty` only for emergencies).

Do not hardcode machine-local paths in release scripts.

## Pull requests

- Describe **why**, not only what changed.
- Include a short test plan (commands you ran).
- Avoid committing secrets, large `target/` artifacts, or one-off local verify scripts that belong in `.gitignore`.
