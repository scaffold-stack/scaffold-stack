# Changelog

All notable changes to **stacksdapp** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Version history is reconstructed from git tags, `cli/Cargo.toml` version bumps, and commit messages.

> **Note:** Only **`v0.1.9`** is git-tagged today. Intermediate versions (`0.1.0`–`0.1.8`) were published to crates.io without tags in this repository.

---

## [0.2.0] - Unreleased

### Added

- Stable CLI exit codes via typed `CliError` (`thiserror`): `2` project not found, `3` prerequisite/doctor, `4` user aborted, `5` validation, `6` type-check, `7` tests, `8` deploy, `10` generate.
- New crate **`stacksdapp-shell`**: shared `-v` / `-q` / `--color` / `--json` output, project-root discovery, and spinner-safe `println_human_safe`.
- Global flags: `-v` / `-vv…`, `-q`, `--color auto|always|never`, `--json`, `--root` (`STACKSDAPP_ROOT`).
- Project root walk-up via `stacksdapp.toml` or `contracts/Clarinet.toml`.
- `stacksdapp completions <shell>` (alias `com`) for bash, zsh, fish, powershell, elvish.
- `doctor --strict` — treat warnings as failures (exit `3`).
- `clean --force` — skip confirmation prompt.
- `deploy -y` / `--yes` — non-interactive deploy and Clarinet fee prompts.
- GitHub Actions **Release** workflow (multi-target binaries + GitHub Release on `v*` tags).
- CI: Clarinet pin **3.21.1**, `cargo clippy`, `verify-e2e.sh`, audit-fix regression checks, frontend build/typecheck, and smoke integration (`scripts/ci-smoke.sh`).
- `scripts/check-versions.sh`, `CONTRIBUTING.md`, MIT `LICENSE`, and local verify scripts under `scripts/`.
- `--json` success payloads for `new`, `init`, `add`, `deploy`, `dev`, and `upgrade`.
- Parser unit tests, scaffold proptest fuzz on name validation, and init rollback tests.
- Devnet readiness panel: local URL, tip height, and deploy hint after `stacksdapp dev`.
- Devnet deploy waits for Clarinet epoch burn height before broadcasting Clarity 5 contracts (epoch 3.4 / burn ≥ 150).
- Devnet deploy success points to the running local app (`http://localhost:3000` or `:3001`) instead of restarting dev.

### Changed

- `pub.sh` resolves repo root from script path, publishes `stacksdapp-shell`, requires clean git worktree by default (`--allow-dirty` opt-in).
- README command table: `init`, `doctor`, `upgrade`, `completions`, global flags, exit codes.
- JSON error payloads include `code` and `exit_code`; doctor JSON includes `exit_code`.
- Default scaffold `Devnet.toml`: snapshot-friendly (no PoX stacking orders, explorers off, `bitcoin_controller_block_time = 15_000` for stable Nakamoto block production).
- `stacksdapp dev` starts Clarinet with `--no-dashboard` and `--from-genesis` to avoid snapshot stalls and Clarinet 3.21 / stacks-core 3.4 config skew.
- Devnet deploy uses `@stacks/transactions` v7 `broadcastTransaction` against stacks core (`:20443`) instead of removed `broadcastRawTransaction`.
- Devnet node readiness and deploy confirmation poll stacks core (`:20443`), not stacks-api alone.
- Workspace crate versions aligned to `0.2.0`.

### Fixed

- `doctor` returns meaningful non-zero exit when checks fail (was always `0`).
- `new` / `add` reject path traversal, absolute paths, and invalid Clarity identifiers.
- Devnet Node broadcast no longer passes private keys on argv (stdin payload).
- False `REDEPLOYMENT REQUIRED` when `deployments.json` is empty or `network=""`.
- JSON mode avoids double-printing error objects.
- Production audit remediation: deploy dry-run snapshot/restore, fail-closed txid parsing, init/add rollback, quiet/json output in deploy UI and watcher, partial deploy state recovery (devnet + remote Clarinet apply).
- Testnet deploy no longer hangs after broadcast — exit Clarinet once all contracts are in the mempool when confirmation is not required.
- `defaults.network` validation from `stacksdapp.toml`; invalid values rejected at CLI dispatch.
- Human error output uses `{e:#}` instead of Debug formatting.
- `reorder_clarinet_toml` restores `Clarinet.toml` on deploy pipeline failure.
- **Devnet boot:** auto-answer Clarinet snapshot “continue? (y/N)” prompt; strip incompatible `pox_5_*` keys from generated `Stacks.toml`; fail fast when stacks-node container exits; stop stale Clarinet Docker on `dev` and `clean`.
- **Devnet UX:** spinner no longer corrupted by Clarinet/codegen logs (`STACKSDAPP_QUIET`, stdout lock); wait for sustained tip advancement before “ready”.
- **Devnet deploy:** epoch-gated publishes no longer accepted to mempool then left unmined; skip re-broadcast when contract already on core; reject broadcast API error responses; omit misleading Hiro explorer link on devnet success.

---

## [0.1.9] — 2026-07-05

Released on crates.io; git tag **`v0.1.9`** (2026-07-06).

### Added

- `stacksdapp dev --auto-deploy` — deploy contracts once local devnet is ready.
- Devnet chain health monitoring and prefixed dev logs.
- Contract ABI caching (`contracts/.cache/`) to skip redundant `initSimnet` runs during `generate`.
- `npm ci` in `generate` when `package-lock.json` exists.
- Committed `contracts/package-lock.json` in scaffold template for reproducible installs.
- Devnet mnemonic safety warning in CLI output.

### Changed

- **Clarinet 3.21+** required (`doctor` check and template alignment).
- Frontend template updated for Clarinet SDK 3.21 (`@stacks/clarinet-sdk`).
- Run Next.js and Vitest via `node` to avoid `.bin` permission errors.
- Bump workspace crates: `stacksdapp-codegen` 0.1.6, `stacksdapp-deployer` 0.1.4, `stacksdapp-process-supervisor` 0.1.6, `stacksdapp-scaffold` 0.1.8.

### Fixed

- Treat transient tx poll 404s as pending instead of hard errors.
- Node modules unload fix during generate.

---

## [0.1.8] — 2026-05-17

### Added

- Frontend Vitest runs in `stacksdapp test` (`vitest run --passWithNoTests`).
- Vercel deployment config for frontend template (`vercel.json`, `public/` directory).
- Pinned `package-lock.json` in frontend template.
- Hiro API key support via `getReadOnlyNetwork` (`@stacks/network` v7).

### Changed

- Migrate frontend to **`@stacks/network` v7** and **`@stacks/transactions` v7**.
- Wallet provider simplified to Jotai-based sync (removed unused `WalletContext`).
- Consistent `stacksdapp` branding in CLI messages, codegen hints, and dev supervisor output.
- README clone path and codegen template docs corrected.
- MIT `LICENSE` file committed (removed from `.gitignore`).

---

## [0.1.7] — 2026-05-03

### Added

- Branded CLI banner (FIGlet-style `stacks` / `dapp` wordmark, boxed tagline).
- `.githooks/pre-commit` — blocks likely mnemonic commits in `Testnet.toml` / `Mainnet.toml` (override via env).
- Colored init/upgrade step output.

### Changed

- Extended `--help` with examples and clearer command descriptions.
- Root `.gitignore` narrowed to generated Clarinet settings only.

---

## [0.1.6] — 2026-05-02

### Added

- **`stacksdapp init`** — adopt an existing Clarinet project (adds frontend, bindings, debug UI).
- **`stacksdapp upgrade`** — refresh deps and regenerate bindings non-destructively.
- **`stacksdapp generate --watch`** — regenerate bindings on `.clar` changes.
- Debug UI: transaction status, errors, and explorer links for contract writes.
- Generated hooks poll transaction lifecycle after write calls.
- Optional Hiro API key headers for read-only Stacks calls (`NEXT_PUBLIC_HIRO_API_KEY`).
- Complex ABI argument JSON handling in debug UI.
- Mainnet deploy confirmation prompt (interactive safety gate).

### Changed

- `init` normalizes root Clarinet layout and shows npm install progress spinners.
- `scaffold.config.ts` validates `NEXT_PUBLIC_NETWORK`; network badge reads from validated config.
- Dev runtime: supervised child processes, debounced watcher codegen, clean Ctrl+C shutdown.

### Fixed

- Deploy safety checks hardened for testnet/mainnet.

---

## [0.1.5] — 2026-04-22

### Added

- **`stacksdapp deploy --dry-run`** — generate deployment plan and fee estimate without broadcasting.
- **`stacksdapp deploy --contract <name>`** — deploy a single contract by name.
- Live npm dependency feedback spinner during scaffold npm installs.

### Changed

- Performance: skip redundant frontend install check in dev; optimize codegen npm install step.

---

## [0.1.4] — 2026-04-22

### Added

- CLI integration test suite and CI workflow.
- Release workflow scaffolding (`bin: release workflow`).
- npm compatibility fixes for generated projects.

### Changed

- Workspace crate versions aligned across `Cargo.toml` files and lockfile.

---

## [0.1.3] — 2026-04-13

Git tag **`v0.1.3`** (2026-04-16).

### Changed

- Support **`@stacks/transactions` 7.4.0**.
- Crates.io version control and dependency pinning across workspace crates.

### Fixed

- Frontend template test fixes.

---

## [0.1.2] — 2026-04-13

### Added

- CLI integration tests (foundation for CI smoke).

### Fixed

- Template test reliability fixes.

---

## [0.1.1] — 2026-04-12

First crates.io-ready release wave.

### Added

- **`stacksdapp doctor`** — prerequisite checks (Rust, Node, Clarinet, Docker, git).
- **`stacksdapp add --template sip010|sip009`** — SIP-010 fungible token and SIP-009 NFT templates.
- Network-aware **`stacksdapp dev --network devnet|testnet|mainnet`** (devnet spins local chain; testnet/mainnet runs frontend only).
- Devnet burner wallet flow in frontend template.
- Responsive header, footer, wallet connect, and debug UI layout.
- `build-tx.mjs` for testnet/mainnet transaction signing.
- Stale deployment warning after `generate` when on-chain state diverges.
- Prefetch Clarinet requirements before devnet starts.
- Per-arg typed inputs in generated debug UI.
- crates.io naming convention (`stacksdapp`, `stacksdapp-scaffold`, etc.).

### Changed

- Flattened `add` command (removed nested subcommand).
- Default deploy network set to **devnet**.
- Next.js 15, `@stacks/clarinet-sdk` v3, `@stacks/connect` v8 in frontend template.
- Default new contracts to **Clarity v4** and `epoch = "latest"`.
- Full README developer guide with network workflows.

### Fixed

- Multi-contract deployment dependency ordering.
- Auto-versioning on testnet/mainnet redeploy conflicts (`counter` → `counter-v2`).
- Devnet deploy stability and stale chain state reset.
- Deployer: node readiness wait, stdin `y` piping to Clarinet, real txid parsing.
- Codegen: Tera filters, read-only call generation, `deployments.json` seeding, hot-reload-safe writes.
- Parser: Clarity ABI type renames (`string-ascii`, `string-utf8`, buffer variant).
- Frontend: wallet connect consistency, read-only API calls, post-condition mode defaults.
- Deploy hang and interactive prompt issues.

---

## [0.1.0] — 2026-03-12

Initial public CLI draft.

### Added

- **`stacksdapp`** binary with core commands:
  - `new` — scaffold monorepo (contracts + Next.js frontend)
  - `dev` — devnet + frontend + file watcher
  - `generate` — parse ABIs and regenerate TypeScript bindings
  - `add` — add Clarity contract
  - `deploy` — deploy to devnet / testnet / mainnet
  - `test` — contract and frontend tests
  - `check` — Clarinet type-check
  - `clean` — remove generated files and devnet state
- Workspace crates: `scaffold`, `parser`, `codegen`, `watcher`, `deployer`, `process_supervisor`.
- Next.js frontend template with wallet connect and generated debug UI.
- Tera templates for `contracts.ts`, `hooks.ts`, `DebugContracts.tsx`.

[Unreleased]: https://github.com/scaffold-stack/scaffold-stack/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/scaffold-stack/scaffold-stack/compare/v0.1.9...v0.2.0
[0.1.9]: https://github.com/scaffold-stack/scaffold-stack/compare/v0.1.3...v0.1.9
[0.1.3]: https://github.com/scaffold-stack/scaffold-stack/releases/tag/v0.1.3
