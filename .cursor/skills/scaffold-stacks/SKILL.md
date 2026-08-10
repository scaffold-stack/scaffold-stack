---
name: scaffold-stacks
description: >-
  Build, test, deploy, and debug full-stack Stacks (Bitcoin L2) dApps with the
  stacksdapp CLI — Clarity contracts, auto-generated React hooks, TypeScript
  bindings, Next.js custom frontend, testnet/mainnet/devnet deployment. Use when
  the user mentions stacksdapp, Scaffold Stacks, Clarity, Stacks dApp, Clarinet,
  generated hooks, custom frontend, SIP-010, SIP-009, devnet, testnet deploy, or
  works in a project with stacksdapp.toml or contracts/Clarinet.toml.
---

# Scaffold Stacks

Scaffold Stacks = **Rust CLI** (`stacksdapp`) + **monorepo** (Clarity + Next.js). Auto-generates TypeScript from contract ABIs, ships a debug UI, deploys to testnet/mainnet/devnet.

**Docs:** https://scaffoldstacks.mintlify.app/ · **Index:** https://scaffoldstacks.mintlify.app/llms.txt

## When to use this skill

- User is in a `stacksdapp new` or `stacksdapp init` project
- Tasks involve contracts, deploy, devnet, bindings, React hooks, or custom frontend UI
- User asks about Clarity + frontend integration on Stacks
- User asks about SIP-010, SIP-009, or Clarity language syntax

**Deep language / spec detail:** [clarity-language.md](clarity-language.md) · [sip-standards.md](sip-standards.md) · [Stacks docs](https://docs.stacks.co/llms.txt)

## Project root

Commands resolve the root by walking up for `stacksdapp.toml` or `contracts/Clarinet.toml`. From subdirs use `--root PATH` or `STACKSDAPP_ROOT`.

```
my-app/
├── stacksdapp.toml              # project marker + defaults.network
├── AGENTS.md                    # this project's agent pointer
├── contracts/
│   ├── Clarinet.toml            # registry, clarity_version, epoch per contract
│   ├── contracts/*.clar         # EDIT HERE
│   ├── settings/*.toml          # deployer mnemonics — never commit real seeds
│   ├── tests/*.test.ts          # Vitest + initSimnet
│   └── .cache/                  # ABI cache (gitignored)
└── frontend/
    ├── src/generated/           # AUTO-GENERATED — never hand-edit
    └── .env.local               # NEXT_PUBLIC_NETWORK
```

Details: [project-layout.md](project-layout.md)

## Command decision tree

| User goal | Command |
|-----------|---------|
| New project | `stacksdapp new NAME` |
| Adopt Clarinet repo | `stacksdapp init` |
| Add contract | `stacksdapp add NAME [--template blank\|sip010\|sip009] [--clarity-version 4\|5\|6]` |
| Type-check | `stacksdapp check` |
| Regenerate TS | `stacksdapp generate [--watch]` |
| Test | `stacksdapp test` |
| Local full stack | `stacksdapp dev [--auto-deploy]` (Docker) |
| Frontend only | `stacksdapp dev --network testnet\|mainnet` |
| Deploy | `stacksdapp deploy --network devnet\|testnet\|mainnet [--contract X] [--yes] [--dry-run]` |
| Reset generated/devnet | `stacksdapp clean [--force]` |
| Health check | `stacksdapp doctor [--strict]` |
| Refresh deps + skill | `stacksdapp upgrade` |

Full flags and exit codes: [cli-reference.md](cli-reference.md)

## Default workflow — testnet first

Recommend testnet for deploy verification (no Docker, reliable):

```bash
stacksdapp doctor
# Set mnemonic in contracts/settings/Testnet.toml (placeholder uses <YOUR...>)
stacksdapp check && stacksdapp generate && stacksdapp test
stacksdapp deploy --network testnet --yes
stacksdapp dev --network testnet
```

For CI/automation always pass `--yes` on deploy. Use `--dry-run` to preview fees.

After **any** `.clar` or `Clarinet.toml` change: `check` → `generate` → `test` → `deploy`.

## Clarity versions

| Version | Epoch | Notes |
|---------|-------|-------|
| **6** (default) | `4.0` | New projects; Clarinet **3.23+** for devnet |
| **5** | `3.4` | Legacy; set **both** version and epoch |
| **4** | `3.0` | Older contracts |

`stacksdapp add --clarity-version 5` sets epoch automatically. Manual downgrades must update both fields in `Clarinet.toml`.

Details: [clarity-versions.md](clarity-versions.md)

## Agent rules

### Do

- Run from project root (or rely on walk-up / `--root`)
- Run `stacksdapp doctor` when prerequisites are unclear
- Edit only `contracts/contracts/*.clar` and app components under `frontend/src/`
- Regenerate after contract changes: `stacksdapp generate`
- Use `--yes` for non-interactive deploys
- Prefer **testnet** for deploy CI and first-time verification
- Warn before committing mnemonics; devnet seeds in template are **public burners**

### Do not

- Hand-edit `frontend/src/generated/*`
- Commit real testnet/mainnet mnemonics
- Run `clarinet devnet start` alongside `stacksdapp dev` (port conflicts)
- Reuse devnet template mnemonics on testnet/mainnet
- Assume devnet is production-like for long-running deploy tests
- Hand-write SIP tokens with `(impl-trait 'sip-010-trait)` — use `stacksdapp add --template sip010|sip009` (full trait path required; see [sip-standards.md](sip-standards.md))
- Call `BigInt(hook.data)` on read-only uint results — unwrap cvToJSON shapes first ([frontend.md](frontend.md))
- Fire read-only hooks (`get-balance`, etc.) without checking deploy + network + wallet address — causes `Failed to fetch` ([frontend.md](frontend.md))
- Run `stacksdapp dev` (devnet) when contracts were deployed to testnet — set `NEXT_PUBLIC_NETWORK=testnet` instead

## Devnet (local Docker)

```bash
stacksdapp dev                  # devnet + frontend + watcher
stacksdapp dev --auto-deploy    # deploy once chain is ready (recommended)
```

**Caveats:** Docker required. Long idle devnet sessions can stall around Stacks block ~71 (PoX). Prefer `--auto-deploy` or deploy soon after boot. For reliable deploy verification use testnet.

If stuck booting: `stacksdapp clean --force`, stop leftover devnet containers, retry.

Details: [workflows.md](workflows.md) · [troubleshooting.md](troubleshooting.md)

## Deploy behavior

- **Testnet/mainnet:** Direct broadcast via `@stacks/transactions`; auto-versioning on redeploy (`counter` → `counter-v2`) unless `--no-auto-version`
- **Devnet:** Epoch burn gating for C5/C6; waits for core node `:20443`
- **`--dry-run`:** Plan + fee estimate only
- **`--wait-confirm`:** Poll until on-chain (testnet/mainnet; slower)

## Code generation

`stacksdapp generate` writes:

- `frontend/src/generated/contracts.ts` — async contract call functions
- `frontend/src/generated/hooks.ts` — React hooks
- `frontend/src/generated/DebugContracts.tsx` — debug UI forms
- `frontend/src/generated/deployments.json` — addresses after deploy

ABI cache in `contracts/.cache/` speeds repeat generates.

## Frontend (hooks & wallet)

Generated bindings → React hooks → your components:

- **`contracts.ts`** — async call functions (wallet on testnet/mainnet, burners on devnet)
- **`hooks.ts`** — `useCounter_Increment()` etc. with `{ call, data, loading, txid, txStatus, … }`
- **`deployments.json`** — contract addresses (requires deploy first)

Wallet (`WalletConnect` + Jotai) is for **testnet/mainnet**. Devnet writes use public burner keys — no wallet popup.

Full guide: [frontend.md](frontend.md)

## Global CLI flags

```
-v, --verbosity    More diagnostic output (repeatable)
-q, --quiet        Suppress non-errors
--json             Machine-readable output (implies quiet logs)
--color auto|always|never
--root PATH        Project root (env: STACKSDAPP_ROOT)
```

## Exit codes (for scripts)

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Project not found |
| 3 | Prerequisite / doctor failure |
| 4 | User aborted |
| 5 | Validation error |
| 6 | Clarity check failed |
| 7 | Tests failed |
| 8 | Deploy failed |
| 10 | Generate failed |

## Examples

**Add token and deploy to testnet:**

```bash
stacksdapp add my-token --template sip010
# edit contracts/contracts/my-token.clar
stacksdapp check && stacksdapp generate && stacksdapp test
stacksdapp deploy --network testnet --yes
```

**Adopt existing Clarinet project:**

```bash
cd existing-clarinet-repo
stacksdapp init
stacksdapp deploy --network testnet --yes
```

**Frontend against mainnet (no local chain):**

```bash
stacksdapp dev --network mainnet
```

## Additional resources

- [frontend.md](frontend.md) — hooks, wallet, devnet signing, custom UI
- [clarity-language.md](clarity-language.md) — Clarity cheat sheet + Stacks learning links
- [sip-standards.md](sip-standards.md) — SIP-010/009 specs + scaffold templates
- [cli-reference.md](cli-reference.md) — all commands, flags, config files
- [workflows.md](workflows.md) — new, adopt, upgrade, CI patterns
- [clarity-versions.md](clarity-versions.md) — C4/C5/C6 migration
- [troubleshooting.md](troubleshooting.md) — doctor, Docker, deploy errors
- [project-layout.md](project-layout.md) — directories and env vars
