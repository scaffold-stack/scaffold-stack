# Workflows

Step-by-step patterns for common Scaffold Stacks tasks.

## New project → testnet deploy

Recommended first path (no Docker):

```bash
stacksdapp new my-app && cd my-app
stacksdapp doctor
```

1. Fund testnet STX: https://explorer.hiro.so/sandbox/faucet?chain=testnet
2. Edit `contracts/settings/Testnet.toml` with deployer mnemonic (local only; never commit)
3. Edit `contracts/contracts/counter.clar` as needed
4. `stacksdapp check && stacksdapp generate && stacksdapp test`
5. `stacksdapp deploy --network testnet --yes`
6. `stacksdapp dev --network testnet` → http://localhost:3000

Connect Leather/Xverse on testnet. Debug UI calls contract functions.

## Full-stack: custom frontend + reusable hooks

End-to-end path when the user wants their own UI (not just the debug panel):

1. **Contracts** — edit `contracts/contracts/*.clar` or `stacksdapp add …`
2. **Validate** — `stacksdapp check && stacksdapp generate && stacksdapp test`
3. **Deploy** — `stacksdapp deploy --network testnet --yes` (populates `deployments.json`)
4. **Custom UI** — create `frontend/src/components/Feature.tsx` with `"use client"`; import hooks from `@/generated/hooks`
5. **Page** — add component to `app/page.tsx` or a new App Router route
6. **Run** — `stacksdapp dev --network testnet`; connect wallet for public calls

Hook naming, `Cl.*` args, wallet vs devnet signing, post-conditions: [frontend.md](frontend.md)

Keep `<DebugContracts />` during development to compare behavior with your custom components.

## Contract iteration loop

```bash
# edit contracts/contracts/*.clar
stacksdapp check
stacksdapp generate        # or stacksdapp dev (includes watcher)
stacksdapp test
stacksdapp deploy --network testnet --yes
```

With `stacksdapp dev --network testnet`, run `generate` manually or use `generate --watch` in a second terminal.

## Add SIP-010 token

```bash
stacksdapp add my-token --template sip010
```

Customize `contracts/contracts/my-token.clar`, then check → generate → test → deploy. Trait functions, hooks, and spec links: [sip-standards.md](sip-standards.md).

## Add SIP-009 NFT

```bash
stacksdapp add my-nft --template sip009
```

Same workflow — see [sip-standards.md](sip-standards.md) for trait table and hook examples.

## Adopt existing Clarinet project

From repo with `Clarinet.toml` (standard or nested layout):

```bash
stacksdapp init
```

Init will:
- Normalize layout to `contracts/Clarinet.toml` + `contracts/contracts/*.clar`
- Add frontend template (if missing)
- Install npm deps, generate bindings, git hooks, agent skill
- Write `stacksdapp.toml` if missing

Then:

```bash
stacksdapp deploy --network testnet --yes
stacksdapp dev --network testnet
```

Guide: https://scaffoldstacks.mintlify.app/adopt-existing.md

## Local devnet

Requires Docker Desktop.

```bash
stacksdapp doctor
stacksdapp dev --auto-deploy
```

- Boots Clarinet devnet + Next.js + file watcher
- `--auto-deploy` deploys contracts once `:20443` is healthy (recommended)
- Devnet mnemonics in `settings/Devnet.toml` are public — devnet only

**Deploy separately:**

```bash
stacksdapp dev                    # terminal 1
stacksdapp deploy --network devnet --yes   # terminal 2, soon after boot
```

Guide: https://scaffoldstacks.mintlify.app/local-devnet.md

## Mainnet deploy

1. Audit contracts and test on testnet first
2. Set `contracts/settings/Mainnet.toml` mnemonic (never commit)
3. `stacksdapp deploy --network mainnet --dry-run` — review plan and fees
4. `stacksdapp deploy --network mainnet --yes --wait-confirm`
5. `stacksdapp dev --network mainnet`

Guide: https://scaffoldstacks.mintlify.app/mainnet.md

## Upgrade project

```bash
stacksdapp upgrade
```

Refreshes npm lockfiles, regenerates bindings, updates git hooks and agent skill. Safe to run after updating `stacksdapp` CLI.

Guide: https://scaffoldstacks.mintlify.app/upgrade.md

## CI pipeline

```bash
stacksdapp doctor --strict
stacksdapp check
stacksdapp generate
stacksdapp test
stacksdapp deploy --network testnet --yes
```

Use `--json` for machine-readable output. Rely on exit codes (see cli-reference.md).

Do not run devnet in CI unless Docker is available and stall risk is acceptable.

## Clean slate

```bash
stacksdapp clean --force
```

Removes `.cache/`, devnet state, generated bindings. Does not delete `.clar` sources.

After clean: `stacksdapp generate` to restore bindings.

## Redeploy / naming collisions

By default, redeploying auto-versions (`counter` → `counter-v2`). To fail instead:

```bash
stacksdapp deploy --network testnet --no-auto-version --yes
```

## Frontend-only against remote network

```bash
stacksdapp dev --network testnet
stacksdapp dev --network mainnet
```

## Subdirectory commands

```bash
cd frontend
stacksdapp check    # walks up to project root

stacksdapp --root /path/to/my-app deploy --network testnet --yes
```
