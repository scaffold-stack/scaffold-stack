# Clarity versions and epochs

Scaffold Stacks supports Clarity 4, 5, and 6. **New projects default to Clarity 6.**

## Version ↔ epoch mapping

| clarity_version | epoch | Stacks epoch | Use case |
|-----------------|-------|--------------|----------|
| 6 | `"4.0"` | 4.0 (burn 163+) | Default for new contracts |
| 5 | `"3.4"` | 3.4 | Legacy contracts, backward compat |
| 4 | `"3.0"` | 3.0 | Older contracts |

**Critical:** `clarity_version` and `epoch` must match. Clarinet 3.23+ treats ambiguous configs as epoch 4.0 — a C5 contract with `epoch = "4.0"` will misbehave on devnet deploy.

## Adding contracts

```bash
# Default C6 + epoch 4.0
stacksdapp add my-contract

# Legacy C5 + epoch 3.4 (automatic)
stacksdapp add legacy --clarity-version 5

# C4 + epoch 3.0
stacksdapp add old --clarity-version 4
```

The `add` command writes the correct epoch into `Clarinet.toml` for the new contract entry.

## Downgrading an existing project

If changing C6 → C5 manually:

1. Set `clarity_version = 5` **and** `epoch = "3.4"` on each affected `[contracts.*]` entry
2. Regenerate deployment plans:
   ```bash
   cd contracts
   clarinet deployments generate --devnet
   clarinet deployments generate --testnet
   ```
3. `stacksdapp check && stacksdapp test`
4. Redeploy

Never leave `clarity_version = 5` with `epoch = "4.0"`.

## Deploy implications

### Testnet / mainnet

- Direct broadcast via `@stacks/transactions`
- Works for C4, C5, C6 when node supports the epoch
- No Docker required

### Devnet

- **Clarity 6** requires **Clarinet 3.23+** and epoch 4.0 devnet snapshot
- C5/C6 deploys wait for epoch burn height before broadcasting
- Devnet `settings/Devnet.toml` includes PoX stacking orders required for epoch 4.0 snapshot — do not remove them

Run `stacksdapp doctor` to verify Clarinet version.

## Templates

| Template | Standard | Notes |
|----------|----------|-------|
| `blank` | — | Empty public/read-only stubs |
| `sip010` | SIP-010 | Fungible token — full guide: [sip-standards.md](sip-standards.md) |
| `sip009` | SIP-009 | NFT — full guide: [sip-standards.md](sip-standards.md) |

Templates inherit `--clarity-version` (default 6). Adds trait **requirements** to `Clarinet.toml` automatically.

## Testing

Contract tests use `initSimnet()` via Vitest — no Docker. Simnet uses Clarinet's in-memory chain; epoch behavior differs from devnet but type-checking and logic tests are reliable.

```bash
stacksdapp test    # runs contracts/tests/*.test.ts
```

## When to use which version

| Scenario | Recommendation |
|----------|----------------|
| New dApp | C6 (default) |
| Porting existing C5 contract | `--clarity-version 5` or manual 3.4 epoch |
| Audited C5 codebase | Keep C5; do not upgrade epoch without audit |
| Mainnet production | Match deployed epoch; test on testnet first |

## Codegen

`stacksdapp generate` reads ABIs regardless of Clarity version. After version changes, always regenerate bindings.
