# Troubleshooting

Common issues and agent actions for Scaffold Stacks projects.

## Prerequisites

Always start with:

```bash
stacksdapp doctor
stacksdapp doctor --strict   # CI: fail on warnings
```

| Check fails | Fix |
|-------------|-----|
| Rust missing/old | Install via rustup.rs |
| Node missing | Install Node 20+ |
| Clarinet missing/old | `brew install clarinet` — need **3.23+** for C6 devnet |
| Docker missing | Install Docker Desktop for devnet only |
| Git hooks | `git config core.hooksPath .githooks` or `npm run setup-hooks` |

## Project not found (exit 2)

```
No scaffold-stacks project found...
```

- `cd` to directory containing `stacksdapp.toml` or `contracts/Clarinet.toml`
- Or: `stacksdapp --root /path/to/project <command>`
- For raw Clarinet repos: run `stacksdapp init` first

## Deploy failures (exit 8)

### Testnet / mainnet

| Symptom | Action |
|---------|--------|
| Placeholder mnemonic | Replace `<YOUR PRIVATE...>` in Testnet.toml |
| Insufficient STX | Fund via testnet faucet |
| Interactive prompt hangs | Add `--yes` |
| Tx in mempool, not confirmed | Normal without `--wait-confirm`; check explorer |
| Wrong network | Match `--network` to settings and wallet |

### Devnet

| Symptom | Action |
|---------|--------|
| Deploy never confirms | Chain may be stalled; use `--auto-deploy` or testnet |
| Port in use | Stop parallel `clarinet devnet start`; `stacksdapp clean --force` |
| Epoch burn wait | Expected for C5/C6; wait or use testnet |

## Devnet boot stuck

```
waiting for bitcoin-node / stacks-node...
```

1. Ensure Docker is running
2. Stop conflicting containers: `docker ps --filter name=devnet -q | xargs docker stop`
3. `stacksdapp clean --force`
4. Retry `stacksdapp dev --auto-deploy`
5. Do not run manual `clarinet devnet start` alongside `stacksdapp dev`

## Devnet stall after ~block 71

Known issue with long-running local devnet (PoX / signer timeout).

**Workarounds:**
- Deploy early: `stacksdapp dev --auto-deploy`
- Use testnet for deploy verification
- Full reset: `stacksdapp clean --force`, restart Docker, retry

Testnet/mainnet deploy is unaffected.

## Clarity version / epoch mismatch

- C5 requires `epoch = "3.4"` (not `"4.0"`)
- C6 requires `epoch = "4.0"`
- Regenerate plans: `cd contracts && clarinet deployments generate --devnet`

Or: `stacksdapp add name --clarity-version 5`

## Bindings out of sync

```bash
stacksdapp generate
```

Never patch `frontend/src/generated/*` manually.

## Check failed (exit 6)

Run `stacksdapp check` and fix Clarinet checker errors in `.clar` files.

### `use of undeclared trait <sip-010-trait>` (or `<nft-trait>`)

Explorer / deploy abort with this VM error when `impl-trait` uses a **short trait name** instead of a full on-chain contract path.

| Bad (causes error) | Good |
|--------------------|------|
| `(impl-trait 'sip-010-trait)` | **Testnet:** omit `impl-trait` — use `stacksdapp add --template sip010` |
| `(impl-trait 'SP3....sip-010-trait)` on testnet | **Mainnet only:** full `SP3FBR2…sip-010-trait` path (see [sip-standards.md](sip-standards.md)) |

**Agent actions:**

1. Run `stacksdapp add NAME --template sip010` (or `sip009`) — do not rewrite token from blog snippets
2. Open [sip-standards.md](sip-standards.md) for the network trait table
3. Match `[[project.requirements]]` in `Clarinet.toml` to the same network
4. `stacksdapp check` → `stacksdapp test` → redeploy

## Frontend: `Cannot convert [object Object] to a BigInt`

Common in custom UI when formatting SIP-010 balances or other uint read-only results.

```
formatTokenAmount(raw) → BigInt(String(raw))
Cannot convert [object Object] to a BigInt
```

**Cause:** `raw` is hook `data` from a read-only call. Scaffold uses `cvToValue`, which represents `(ok uint)` as `{ type: "uint", value: "1500000" }` — not a native `bigint`.

**Fix:**

1. Do not `BigInt(hook.data)` directly.
2. Unwrap with a helper — full example in [frontend.md](frontend.md) (`clarityUintToBigInt` + `formatTokenAmount`).
3. Remember SIP-010 amounts are **base units**; divide by `10**decimals` for human display.

**Debug:** `console.log(JSON.stringify(data))` on the hook result to see the shape before writing formatters.

## Frontend: `Failed to fetch` on read-only calls

```
TypeError: Failed to fetch
src/generated/contracts.ts … fetchCallReadOnlyFunction({ … functionName: 'get-balance'
```

**Cause:** Browser could not reach the Stacks node — not a contract revert. Most often **network env mismatch** or **devnet node down**.

| Failed request host | Fix |
|---------------------|-----|
| `localhost:3999` | Run `stacksdapp dev` (Docker), **or** switch to testnet: `NEXT_PUBLIC_NETWORK=testnet` + restart Next.js |
| `api.testnet.hiro.so` | Check connectivity; add `NEXT_PUBLIC_HIRO_API_KEY`; disable ad-blocker |
| Contract deployed but still fails | Verify `deployments.json` has the contract (e.g. `airdrop-token-v2`) and `ST…` prefix matches testnet |

**Agent checklist:**

1. `cat frontend/src/generated/deployments.json` — contract entry exists
2. `grep NEXT_PUBLIC_NETWORK frontend/.env.local` — matches deploy network (`testnet` for `ST…` contracts)
3. After testnet deploy: `stacksdapp dev --network testnet` (not default devnet)
4. In custom UI: guard read-only `call()` until wallet address is valid; show `hook.error` — full pattern in [frontend.md](frontend.md)
5. Restart Next.js after any `.env.local` change

## Tests failed (exit 7)

Run `stacksdapp test`. Contract tests use simnet — no Docker.

## Generate failed (exit 10)

- Validate `contracts/Clarinet.toml`
- Run `npm install` in `contracts/` and `frontend/`
- `stacksdapp clean --force` then `stacksdapp generate`

## Init conflicts

Merge duplicate `settings/`, `tests/`, or `deployments/` dirs before rerunning `stacksdapp init`.

## Git hook blocked commit

Unstage real mnemonics from Testnet/Mainnet settings. Emergency bypass: `SCAFFOLD_ALLOW_COMMITTED_MNEMONIC=1 git commit`

## Mnemonic security

- Devnet template mnemonics are public — devnet only
- Pre-commit hook blocks likely seed phrases in Testnet/Mainnet.toml

## Getting help

- Docs: https://scaffoldstacks.mintlify.app/
- Telegram: https://telegram.me/+CBp6wSIiXNhmMjZk
- GitHub: https://github.com/scaffold-stack/scaffold-stack
