# SIP-010 & SIP-009 — standards + scaffold templates

Scaffold Stacks ships **starter contracts** for fungible tokens (SIP-010) and NFTs (SIP-009). This file maps the specs to `stacksdapp add` templates, Clarity functions, and generated frontend hooks.

## Official specifications

| Standard | Spec | Stacks docs |
|----------|------|-------------|
| **SIP-010** (fungible token) | [GitHub SIP-010](https://github.com/stacksgov/sips/blob/main/sips/sip-010/sip-010-fungible-token-standard.md) | [Create a fungible token](https://docs.stacks.co/get-started/create-a-token/fungible-tokens.md) |
| **SIP-009** (NFT) | [GitHub SIP-009](https://github.com/stacksgov/sips/blob/main/sips/sip-009/sip-009-nft-standard.md) | [Create an NFT](https://docs.stacks.co/get-started/create-a-token/non-fungible-tokens.md) |
| Clarity Book (FT) | [SIP010 chapter](https://book.clarity-lang.org/ch10-03-sip010-ft-standard.html) | — |
| Clarity Book (NFT) | [SIP009 chapter](https://book.clarity-lang.org/ch10-01-sip009-nft-standard.html) | — |

Metadata extensions: [SIP-016](https://github.com/stacksgov/sips/blob/main/sips/sip-016/sip-016-metadata-schema.md) · [SIP-019 metadata updates](https://github.com/stacksgov/sips/blob/main/sips/sip-019/sip-019-token-metadata-update-notifications.md)

## Scaffold commands (prefer over hand-written tokens)

```bash
stacksdapp add my-token --template sip010
stacksdapp add my-nft --template sip009
stacksdapp add legacy-token --template sip010 --clarity-version 5   # if needed
```

**Do not** hand-write a token from blog snippets. Use the template — it includes SIP-compatible functions, tests, requirements, and hooks.

Each command creates:

- `contracts/contracts/<name>.clar` with all required SIP **functions** (see below)
- `contracts/tests/<name>.test.ts` (if no test file exists)
- `[contracts.<name>]` entry in `Clarinet.toml` with correct epoch
- `[[project.requirements]]` for the mainnet trait (Clarinet simnet type-checking)
- Regenerated hooks in `frontend/src/generated/hooks.ts`

## `impl-trait` — testnet vs mainnet (critical)

### Common mistake → explorer error

```
VM Error: use of undeclared trait <sip-010-trait>
```

**Cause:** `(impl-trait 'sip-010-trait)` or `(impl-trait '.sip-010-trait)` — a short name with no `(define-trait …)` in the same contract and no full `SP…` contract path.

**Fix:**

1. Prefer `stacksdapp add NAME --template sip010` (or `sip009`) instead of writing from scratch.
2. Follow the network rules below — **never** paste a bare trait name.

**Never:**

```clarity
(impl-trait 'sip-010-trait)           ;; WRONG — undeclared trait (most common AI mistake)
(define-trait sip-010-trait ...)      ;; WRONG shortcut — use deployed mainnet trait for production
(impl-trait 'SP3....sip-010-trait)    ;; WRONG on testnet — mainnet trait contract is not on testnet chain
```

### Network rules

| Network | `impl-trait` in token contract | Notes |
|---------|-------------------------------|-------|
| **Testnet** | **Omit** `(impl-trait …)` | No standard SIP-010/SIP-009 trait contract is deployed on testnet today. Scaffold templates ship **without** `impl-trait` so testnet deploy succeeds. Functions (`get-name`, `transfer`, …) still match SIP-010. |
| **Mainnet** | **Required** for wallet listing | Add before deploy: `(impl-trait 'SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard.sip-010-trait)` |
| **Simnet / `stacksdapp test`** | Omit (template default) | Clarinet requirement pulls mainnet trait source for type-checking only |

### Mainnet trait addresses

| Standard | `impl-trait` (mainnet only) | `[[project.requirements]]` (scaffold default) |
|----------|----------------------------|-----------------------------------------------|
| SIP-010 | `'SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard.sip-010-trait` | `SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard` |
| SIP-009 | `'SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait` | `SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait` |

Scaffold templates include a **comment** with the exact mainnet `impl-trait` line to uncomment before mainnet deploy.

## SIP-010 trait (required interface)

Seven functions wallets and apps expect:

| Function | Purpose |
|----------|---------|
| `transfer(amount, sender, recipient, memo)` | Move tokens |
| `get-name` | Token name |
| `get-symbol` | Ticker |
| `get-decimals` | Display decimals |
| `get-balance(who)` | Balance of principal |
| `get-total-supply` | Circulating supply |
| `get-token-uri` | Optional metadata URI |

Trait definition: [SIP-010 trait section](https://github.com/stacksgov/sips/blob/main/sips/sip-010/sip-010-fungible-token-standard.md)

### Scaffold SIP-010 template

The template implements all SIP-010 functions plus:

| Function | Access | Notes |
|----------|--------|-------|
| `mint(amount, recipient)` | public | Owner-only; initial supply |
| `set-token-uri(value)` | public | Owner-only; emits SIP-019-style print |
| `define-fungible-token` | — | Asset name = contract name |

Default: **6 decimals**, owner = deployer at deploy time.

### SIP-010 test (generated)

```typescript
simnet.callPublicFn("my-token", "mint", [Cl.uint(100), Cl.standardPrincipal(deployer)], deployer);
```

### SIP-010 frontend hooks (after generate + deploy)

```tsx
import { useMyToken_GetBalance, useMyToken_Transfer, useMyToken_Mint } from '@/generated/hooks';
import { Cl } from '@stacks/transactions';

// read:  useMyToken_GetBalance().call([Cl.principal('ST…')])
// write: useMyToken_Transfer().call([Cl.uint(100), Cl.principal(sender), Cl.principal(recipient), Cl.none()])
```

Hook names follow `use{ContractPascal}_{FunctionPascal}` — see [frontend.md](frontend.md).

---

## SIP-009 trait (required interface)

Four functions for NFT interoperability:

| Function | Purpose |
|----------|---------|
| `get-last-token-id` | Highest minted id |
| `get-token-uri(token-id)` | Metadata URI for id |
| `get-owner(token-id)` | Current owner |
| `transfer(token-id, sender, recipient)` | Change owner |

Spec: [SIP-009 trait section](https://github.com/stacksgov/sips/blob/main/sips/sip-009/sip-009-nft-standard.md)

### Scaffold SIP-009 template

Implements all SIP-009 functions plus:

| Function | Access | Notes |
|----------|--------|-------|
| `mint(recipient)` | public | Owner-only; auto-increments id |
| `set-base-uri(value)` | public | Owner-only; metadata template with `{id}` |
| `define-non-fungible-token` | — | Token ids are `uint` |
| `COLLECTION_LIMIT` | constant | Default cap u1000 |

### SIP-009 test (generated)

```typescript
simnet.callPublicFn("my-nft", "mint", [Cl.standardPrincipal(deployer)], deployer);
// expect Cl.uint(1) — first token id
```

### SIP-009 frontend hooks

```tsx
import { useMyNft_GetOwner, useMyNft_Mint, useMyNft_Transfer } from '@/generated/hooks';

// useMyNft_GetOwner().call([Cl.uint(1)])
// useMyNft_Transfer().call([Cl.uint(1), Cl.principal(sender), Cl.principal(recipient)])
```

---

## End-to-end: token/NFT dApp

```bash
stacksdapp add my-token --template sip010
stacksdapp check && stacksdapp generate && stacksdapp test
stacksdapp deploy --network testnet --contract my-token --yes   # no impl-trait needed
# For mainnet: uncomment impl-trait line in .clar first, then deploy
stacksdapp dev --network testnet
```

Same flow for `--template sip009`.

## Customizing templates

| Goal | Edit in `.clar` |
|------|-----------------|
| Change decimals / collection limit | constants at top |
| Restrict mint | `asserts!` on `tx-sender` |
| Royalties / marketplace | add functions; keep trait fns compatible |
| Metadata | `set-token-uri` / `set-base-uri`; consider SIP-016 off-chain JSON |
| Mainnet wallet listing | Uncomment/add mainnet `impl-trait` line before mainnet deploy |

After edits: `check` → `generate` → `test` → `deploy`.

## Agent rules

- **Always** use `stacksdapp add --template sip010|sip009` for new tokens
- **Never** add `(impl-trait 'sip-010-trait)` — use full mainnet path or omit for testnet
- Read the **SIP spec** for interface contracts; read the **scaffold template** for what's already implemented
- Do not break trait function signatures if wallet compatibility matters
- Use generated hooks for frontend — do not reimplement transfer in raw `@stacks/transactions` unless necessary
- Clarity language help: [clarity-language.md](clarity-language.md)
