# scaffold-stacks

A Rust-powered CLI (`stacksdapp`) and Next.js template for building full-stack Stacks (Bitcoin L2) dApps — with auto-generated TypeScript contract bindings, a live debug UI, and one-command testnet deployment.

---

## Prerequisites

| Tool | Install | Required for |
|---|---|---|
| **Rust** 1.75+ | [rustup.rs](https://rustup.rs) | Building the CLI |
| **Node.js** 20+ | [nodejs.org](https://nodejs.org) | Frontend + contract tests |
| **Clarinet** | `brew install clarinet` | Contract toolchain |
| **Leather or Xverse** | [leather.io](https://leather.io) | Wallet for testnet/mainnet |
| **Docker Desktop** | [docker.com](https://docker.com) | Local devnet only |

```bash
rustc --version      # rustc 1.75+
node --version       # v20+
clarinet --version   # clarinet 3.x
```

---

## Install the CLI

```bash
git clone https://github.com/scaffold-stack/scaffold-stack.git
cd stackscaffold
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
```

```
🚀 Deploying to testnet (https://api.testnet.hiro.so)
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

Contract tests run entirely in Node via `initSimnet()` — no Docker, no devnet required.

### Type-check contracts

```bash
stacksdapp check
```

### Iterate and redeploy

Because Stacks contracts are immutable, redeploying after changes auto-versions the contract name (`counter` → `counter-v2` → `counter-v3`). The CLI handles this automatically — no manual renaming needed.

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
| `stacksdapp new <name>` | Scaffold a new project |
| `stacksdapp dev --network testnet` | Run frontend against testnet (no Docker) |
| `stacksdapp dev --network mainnet` | Run frontend against mainnet (no Docker) |
| `stacksdapp dev` | Start local devnet + frontend + watcher (Docker required) |
| `stacksdapp deploy --network testnet` | Deploy to testnet |
| `stacksdapp deploy --network mainnet` | Deploy to mainnet |
| `stacksdapp deploy --network devnet` | Deploy to local devnet |
| `stacksdapp generate` | Parse ABIs → regenerate TS bindings + debug UI |
| `stacksdapp add <name>` | Add a blank Clarity contract |
| `stacksdapp add <name> --template sip010` | Add a SIP-010 fungible token |
| `stacksdapp add <name> --template sip009` | Add a SIP-009 NFT |
| `stacksdapp test` | Run contract + frontend tests |
| `stacksdapp check` | Type-check all Clarity contracts |
| `stacksdapp clean` | Remove generated files and devnet state |

---

## How Auto-Codegen Works

`stacksdapp generate` runs in four stages:

1. **Parse** — `export-abi.mjs` calls `initSimnet()` to extract the ABI of every contract in `Clarinet.toml`
2. **Normalise** — maps Clarity types to TypeScript (`uint128` → `bigint`, `string-ascii` → `string`, tuples → typed objects)
3. **Render** — Tera templates produce `contracts.ts` (typed call wrappers), `hooks.ts` (React hooks), and `DebugContracts.tsx` (live debug panel)
4. **Write** — SHA-256 hashes new vs existing output; only writes if content changed, keeping Next.js hot-reload fast

The file watcher calls this pipeline automatically on every `.clar` save during `stacksdapp dev`.

---

## Crate Architecture

```
cli/                    # Binary — clap CLI entrypoint
crates/
  scaffold/             # stacksdapp new + stacksdapp add
  parser/               # Clarity ABI → Rust structs
  codegen/              # Rust structs → TypeScript via Tera
  watcher/              # notify file watcher + debounce
  deployer/             # clarinet deployments generate + apply
  process_supervisor/   # orchestrates dev per network
templates/
  contracts.ts.tera
  hooks.ts.tera
  debug_ui.tsx.tera
frontend-template/      # copied into every new project
```

---

## Contributing

```bash
git clone https://github.com/scaffold-stack/scaffold-stack.git
cd stackscaffold
cargo build
cargo test --all
```

---

## License

MIT