# CLI reference — stacksdapp

Syntax and flags for Scaffold Stacks 0.2.x. Walkthroughs: https://scaffoldstacks.mintlify.app/

## Global flags

| Flag | Description |
|------|-------------|
| `-v`, `--verbosity` | Repeat for more debug (walk-up root, deploy steps) |
| `-q`, `--quiet` | Suppress non-error output |
| `--json` | JSON success/failure payloads; implies quiet human logs |
| `--color auto\|always\|never` | Terminal colors (default `auto`) |
| `--root PATH` | Project root; env `STACKSDAPP_ROOT` |

## Exit codes

| Code | `code` field (JSON) | When |
|------|---------------------|------|
| 0 | — | Success |
| 1 | `error` | Generic / unexpected |
| 2 | `project` | No project found, invalid `--root` |
| 3 | `prerequisite` | `doctor` fail, missing clarinet/node/rust |
| 4 | `aborted` | User declined confirmation |
| 5 | `validation` | Invalid project/contract name, bad flags |
| 6 | `check` | Clarity type-check failed |
| 7 | `test` | Vitest failed |
| 8 | `deploy` | Broadcast/plan/mnemonic failure |
| 10 | `generate` | Codegen / ABI export failed |

## Commands

### `stacksdapp new <name>`

Scaffold monorepo: contracts, frontend, git hooks, agent skill, default counter contract.

| Flag | Description |
|------|-------------|
| `--no-git` | Skip `git init` |

### `stacksdapp init`

Adopt existing Clarinet project. Adds frontend (if missing), support files, bindings, git hooks, agent skill.

Expects `Clarinet.toml` at repo root **or** `contracts/Clarinet.toml`. Normalizes standard Clarinet layout into scaffold layout.

### `stacksdapp upgrade`

Refresh npm deps, regenerate bindings, refresh git hooks and agent skill. Non-destructive to user contracts.

### `stacksdapp doctor`

Checks Rust 1.75+, Node 20+, Clarinet 3.23+ (C6 devnet), Docker (for devnet), git hooks path.

| Flag | Description |
|------|-------------|
| `--strict` | Warnings → non-zero exit |

### `stacksdapp add <name>`

Add contract to `contracts/contracts/<name>.clar`, update `Clarinet.toml`, regenerate bindings.

| Flag | Default | Description |
|------|---------|-------------|
| `--template` | `blank` | `blank`, `sip010`, `sip009` |
| `--clarity-version` | `6` | `4`, `5`, or `6` — sets matching epoch |

### `stacksdapp remove <name>`

Delete `contracts/contracts/<name>.clar`, its Vitest file, and the `[contracts.<name>]` block in `contracts/Clarinet.toml`, then regenerate bindings.

### `stacksdapp check`

Run Clarinet type-checker on all contracts.

### `stacksdapp generate`

Export ABIs → TypeScript bindings + debug UI. Uses `contracts/.cache/` when sources unchanged.

| Flag | Description |
|------|-------------|
| `--watch` | Regenerate on `.clar` changes |

### `stacksdapp test`

Run Vitest in `contracts/` (initSimnet) and frontend tests.

### `stacksdapp clean`

Remove generated frontend bindings and local devnet cache files. Does not stop a running devnet. When a devnet is running, clean restores that chain to the snapshot so the next deploy uses the current contract name.

| Flag | Description |
|------|-------------|
| `--force` | Skip confirmation |

### `stacksdapp dev`

| Flag | Description |
|------|-------------|
| (no flag) | Clarinet devnet in this terminal (Docker) |
| `--network devnet\|testnet\|mainnet` | Next.js only. `devnet` expects `stacksdapp dev` already running |
| `--auto-deploy` | Unused. Deploy with `stacksdapp deploy` |
| `--keep-state` | Unused. Reset the chain with `stacksdapp clean` |

### `stacksdapp deploy`

| Flag | Description |
|------|-------------|
| `--network` | `devnet`, `testnet`, `mainnet` (default from `stacksdapp.toml`) |
| `--contract NAME` | Single contract |
| `--dry-run` | Plan + fee, no broadcast |
| `-y`, `--yes` | Non-interactive (required for CI/agents) |
| `--wait-confirm` | Poll until confirmed on-chain (testnet/mainnet) |
| `--no-auto-version` | Fail instead of renaming on collision |

### `stacksdapp completions <shell>`

Generate shell completions: bash, zsh, fish, powershell, elvish.

## Configuration files

| Path | Role |
|------|------|
| `stacksdapp.toml` | Project marker; `defaults.network` |
| `contracts/Clarinet.toml` | Contract registry, clarity_version, epoch |
| `contracts/settings/Devnet.toml` | Local devnet accounts (public test mnemonics) |
| `contracts/settings/Testnet.toml` | Testnet deployer mnemonic |
| `contracts/settings/Mainnet.toml` | Mainnet deployer mnemonic |
| `frontend/.env.local` | `NEXT_PUBLIC_NETWORK`, optional node URL |
| `contracts/.cache/` | Cached ABIs |
| `frontend/src/generated/*` | Generated — do not edit |
| `.githooks/pre-commit` | Blocks likely seed phrases in Testnet/Mainnet settings |
| `.cursor/skills/scaffold-stacks/` | Bundled agent skill |

## Name validation

- **Project name:** single segment, letters/digits/`-`/`_`, no `..` or absolute paths
- **Contract name:** Clarity identifier, letter first, max 40 chars

## JSON output

With `--json`, success payloads include `"ok": true` and command-specific fields. Failures include `"ok": false`, `"error"`, `"code"`, `"exit_code"`.

Example: `stacksdapp doctor --json`

## Prerequisites

| Tool | Minimum |
|------|---------|
| Rust | 1.75+ |
| Node.js | 20+ |
| Clarinet | 3.23+ (C6 devnet) |
| Docker | Devnet only |
| Wallet | Leather or Xverse for testnet/mainnet UI |

Install CLI: `cargo install stacksdapp`
