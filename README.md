# scaffold-stacks

A Rust-powered CLI (`stacksdapp`) and Next.js template for building full-stack Stacks (Bitcoin L2) dApps — with auto-generated TypeScript contract bindings, a live debug UI, and one-command testnet deployment.

---

## Prerequisites

| Tool | Install | Required for |
|---|---|---|
| **Rust** 1.75+ | [rustup.rs](https://rustup.rs) | Building the CLI |
| **Node.js** 20+ | [nodejs.org](https://nodejs.org) | Frontend + contract tests |
| **Clarinet** 3.23+ | `brew install clarinet` | Contract toolchain (C6 devnet requires 3.23+) |
| **Leather or Xverse** | [leather.io](https://leather.io) | Wallet for testnet/mainnet |
| **Docker Desktop** | [docker.com](https://docker.com) | Local devnet only |

```bash
rustc --version      # rustc 1.75+
node --version       # v20+
clarinet --version   # clarinet 3.21+
```

---

## Install via Crates.io:
```bash 
cargo install stacksdapp
stacksdapp --version
```

## Or build from source:

```bash
git clone https://github.com/scaffold-stack/scaffold-stack.git
cd scaffold-stack
cargo install --path cli
stacksdapp --version
```

---

## Quickstart — Testnet in 5 Steps

No Docker needed. Contracts run on Hiro's testnet infrastructure.

### 1 — Scaffold

```bash
stacksdapp new my-app
cd my-app
```

### 2 — Get testnet STX

```
https://explorer.hiro.so/sandbox/faucet?chain=testnet
```

Add your deployer mnemonic to `contracts/settings/Testnet.toml`:

```toml
[accounts.deployer]
mnemonic = "your 24 words here"
```

### 3 — Deploy to testnet

```bash
stacksdapp deploy --network testnet
# deploy a single contract only
stacksdapp deploy --network testnet --contract counter
# preview deployment without broadcasting
stacksdapp deploy --network testnet --dry-run
```

```
Deploying to testnet (https://api.testnet.hiro.so)
[deploy] Generating deployment plan...
[deploy] Applying deployment plan to testnet...
  ✔ counter | txid 0x86fa3030... | address ST3JAE....counter
[deploy] Written to frontend/src/generated/deployments.json
```

### 4 — Start the frontend

```bash
stacksdapp dev --network testnet
```

Opens [http://localhost:3000](http://localhost:3000) with your wallet connected to testnet. The Debug Contracts panel shows every function in your contracts with typed inputs ready to call.

### 5 — Connect your wallet

Click **Connect Wallet** and connect Leather or Xverse set to **Testnet**. Every public function opens a wallet popup to sign and broadcast. Every read-only function calls the node directly — no wallet needed.

---

## Developer Workflow

### Edit contracts → see live updates

Open any `.clar` file and add a function:

```clarity
(define-public (multiply (n uint))
  (begin
    (var-set counter (* (var-get counter) n))
    (ok (var-get counter))))
```

Run generate to update bindings:

```bash
stacksdapp generate
```

The `multiply` card appears in the debug UI immediately.

### Add a new contract

```bash
stacksdapp add relayer            # blank contract
stacksdapp add token --template sip010   # SIP-010 fungible token
stacksdapp add nft   --template sip009   # SIP-009 NFT
```

Each command creates the `.clar` file, updates `Clarinet.toml`, and regenerates all TypeScript bindings.

### Run tests

```bash
stacksdapp test
# Runs Vitest in contracts/ (Clarinet SDK — no Docker needed)
# Runs Vitest in frontend/
```

Contract tests run entirely in Node via `initSimnet()` — no Docker, no devnet required. Contract ABIs are cached in `contracts/.cache/` and only re-exported when sources change.

### Type-check contracts

```bash
stacksdapp check
```

### Iterate and redeploy

Because Stacks contracts are immutable, redeploying after changes auto-versions the contract name (`counter` → `counter-v2` → `counter-v3`). The CLI handles this automatically — no manual renaming needed. Use `--no-auto-version` to fail instead of renaming.

**Testnet/mainnet deploy semantics:** By default, `deploy` exits once transactions are **broadcast** to the mempool (fast; no frozen terminal). Verify txids on the [Hiro explorer](https://explorer.hiro.so). Use `--wait-confirm` when you need the CLI to block until contracts are on chain (CI gates). Devnet always waits for local core confirmation.

---

## Mainnet Workflow

```bash
# 1. Test thoroughly on testnet first
# 2. Add mnemonic to contracts/settings/Mainnet.toml
# 3. Ensure sufficient STX for fees

stacksdapp deploy --network mainnet
stacksdapp dev --network mainnet
```

---

## Local Devnet (Optional)

For offline development or simulating the full Bitcoin + Stacks stack locally. Requires Docker Desktop.

```bash
# Terminal 1 — start local chain + frontend + watcher
stacksdapp dev

# Terminal 2 — deploy to local chain (once node is ready ~30s)
stacksdapp deploy --network devnet
```

Pre-funded accounts from `contracts/settings/Devnet.toml` are available immediately. No real STX or wallet needed — the debug UI uses the devnet burner accounts.

```bash
stacksdapp clean   # stop devnet and reset generated files
```

---

## Project Structure

```
my-app/
├── contracts/
│   ├── Clarinet.toml
│   ├── settings/
│   │   ├── Devnet.toml          # pre-funded local accounts
│   │   ├── Testnet.toml         # add your mnemonic here
│   │   └── Mainnet.toml         # add your mnemonic here
│   ├── contracts/
│   │   └── counter.clar
│   └── tests/
│       └── counter.test.ts
└── frontend/
    ├── .env.local               # NEXT_PUBLIC_NETWORK=testnet (auto-managed)
    └── src/
        ├── app/
        ├── components/
        │   └── WalletConnect.tsx
        └── generated/           # ← never edit by hand
            ├── contracts.ts
            ├── hooks.ts
            ├── DebugContracts.tsx
            └── deployments.json
```

---

## Command Reference

| Command | Description |
|---|---|
| `stacksdapp new <name>` | Scaffold a new monorepo workspace |
| `stacksdapp init` | Adopt an existing Clarinet project in the current directory |
| `stacksdapp doctor [--strict] [--json]` | Check prerequisites (Rust, Node, Clarinet, Docker, …) |
| `stacksdapp upgrade` | Refresh dependencies and regenerate bindings |
| `stacksdapp completions <shell>` | Print shell completions (`bash`, `zsh`, `fish`, `powershell`, `elvish`) |
| `stacksdapp dev --network testnet` | Run frontend against testnet (no Docker) |
| `stacksdapp dev --network mainnet` | Run frontend against mainnet (no Docker) |
| `stacksdapp dev` | Start local devnet + frontend + watcher (Docker required) |
| `stacksdapp dev --auto-deploy` | Devnet + auto-deploy contracts once the chain is ready |
| `stacksdapp deploy --network testnet` | Deploy to testnet |
| `stacksdapp deploy --network testnet --contract <name>` | Deploy only one contract by name |
| `stacksdapp deploy --network testnet --dry-run` | Generate plan + estimated fee without broadcasting |
| `stacksdapp deploy --network testnet -y` | Non-interactive deploy (skip confirmation / Clarinet fee prompts) |
| `stacksdapp deploy --network testnet --wait-confirm` | Poll until contracts appear on chain (default: exit after mempool broadcast) |
| `stacksdapp deploy --network testnet --no-auto-version` | Fail on name conflict instead of auto-renaming (`counter` → `counter-v2`) |
| `stacksdapp deploy --network mainnet` | Deploy to mainnet |
| `stacksdapp deploy --network devnet` | Deploy to local devnet |
| `stacksdapp generate [--watch]` | Parse ABIs → regenerate TS bindings + debug UI |
| `stacksdapp add <name>` | Add a blank Clarity contract |
| `stacksdapp add <name> --template sip010` | Add a SIP-010 fungible token |
| `stacksdapp add <name> --template sip009` | Add a SIP-009 NFT |
| `stacksdapp test` | Run contract + frontend tests |
| `stacksdapp check` | Type-check all Clarity contracts |
| `stacksdapp clean [--force]` | Remove generated files and devnet state |

### Global flags

| Flag | Description |
|---|---|
| `-v` / `-vv`… | Increase diagnostic verbosity |
| `-q` / `--quiet` | Suppress non-error human logs |
| `--color auto\|always\|never` | Color control (default `auto`) |
| `--json` | Machine-readable stdout (single JSON object per command; `dev` emits once when the frontend is **ready**, errors always emit JSON) |
| `--root <PATH>` | Project root (or set `STACKSDAPP_ROOT`); otherwise walks up for `stacksdapp.toml` / `contracts/Clarinet.toml` |

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Generic / unexpected error |
| `2` | Project not found or invalid `--root` |
| `3` | Missing / failing prerequisite (`doctor`, clarinet, node, …) |
| `4` | User aborted (confirmations) |
| `5` | Input / argument validation |
| `6` | Contract type-check failed |
| `7` | Tests failed |
| `8` | Deploy failed |
| `10` | Generate / codegen failed |

---

## Crate Architecture

```
cli/                              # Binary — clap CLI entrypoint
crates/
  shell/                          # verbosity / quiet / color / JSON + project root discovery
  scaffold/                       # stacksdapp new + init + add + upgrade
    frontend-template/            # copied into every new project's frontend/
    agent-skill-template/         # AI agent skill → .cursor/skills/scaffold-stacks/
  parser/                         # Clarity ABI → Rust structs
  codegen/                        # Rust structs → TypeScript via Tera
    templates/
      contracts.ts.tera
      hooks.ts.tera
      debug_ui.tsx.tera
  watcher/                        # notify file watcher + debounce
  deployer/                       # clarinet deployments generate + apply
  process_supervisor/             # orchestrates dev per network
```

---

## AI agents (Cursor, Claude Code, Codex)

Every project from `stacksdapp new` or `stacksdapp init` includes a bundled agent skill — **no extra setup**.

| Path | Purpose |
|---|---|
| `.cursor/skills/scaffold-stacks/SKILL.md` | Cursor auto-discovery; read this first in other agents |
| `AGENTS.md` | Short pointer at project root for non-Cursor tools |

The skill covers CLI commands, Clarity version rules, testnet-first workflows, devnet caveats, and troubleshooting. `stacksdapp upgrade` refreshes it to match your CLI version.

**Docs for agents:** https://scaffoldstacks.mintlify.app/llms.txt

To change the skill, edit `crates/scaffold/agent-skill-template/` in this repo (source of truth).

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build, test, release, and PR guidelines.

```bash
git clone https://github.com/scaffold-stack/scaffold-stack.git
cd scaffold-stack
cargo build -p stacksdapp
cargo test --all
bash scripts/ci-smoke.sh
```

Release notes live in [CHANGELOG.md](CHANGELOG.md).

---

## License

MIT