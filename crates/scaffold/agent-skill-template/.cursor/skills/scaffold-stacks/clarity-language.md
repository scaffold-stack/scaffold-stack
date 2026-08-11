# Clarity language — learning path & scaffold cheat sheet

Scaffold Stacks handles deploy, bindings, and frontend — **you still write Clarity** in `contracts/contracts/*.clar`. For language depth, use official docs; this file routes agents there and lists syntax you'll use daily in scaffold projects.

## When to use which resource

| Need | Resource |
|------|----------|
| First-time Clarity | [Clarity Crash Course](https://docs.stacks.co/get-started/clarity-crash-course.md) |
| Structured book | [Clarity Book](https://book.clarity-lang.org/) |
| Interactive course | [Clarity Universe](https://clarity-lang.org/universe) |
| Browser REPL | [Clarity Playground](https://play.stackslabs.com/) |
| Function/type reference | [Clarity Reference](https://docs.stacks.co/reference/clarity/functions.md) |
| Full docs index | [docs.stacks.co/llms.txt](https://docs.stacks.co/llms.txt) — see [stacks-docs-index.md](stacks-docs-index.md) for section map + fetch workflow |
| Language spec (SIP-002) | [SIP-002 Smart Contract Language](https://github.com/stacksgov/sips/blob/main/sips/sip-002/sip-002-smart-contract-language.md) |
| Ask docs a question | `GET https://docs.stacks.co/<page>.md?ask=<question>&goal=<goal>` |

**Agent rule:** For Clarity language questions beyond this cheat sheet, read [stacks-docs-index.md](stacks-docs-index.md) then fetch Stacks docs — do not invent syntax.

## Scaffold project workflow (Clarity side)

```bash
# edit contracts/contracts/my-contract.clar
stacksdapp check          # Clarinet type-check + check_checker
stacksdapp test           # Vitest + initSimnet in contracts/tests/
stacksdapp generate       # refresh frontend hooks
stacksdapp deploy --network testnet --yes
```

Contract tests live in `contracts/tests/*.test.ts` — no Docker. See [Testing with Clarinet SDK](https://docs.stacks.co/clarinet/testing-with-clarinet-sdk.md).

## Syntax essentials (scaffold projects)

Clarity is LISP-like: expressions in parentheses.

### Data variables

```clarity
(define-data-var counter uint u0)
(var-get counter)
(var-set counter (+ (var-get counter) u1))
```

### Functions

| Form | Who can call | Mutates state |
|------|--------------|---------------|
| `define-read-only` | Anyone | No |
| `define-public` | Anyone | Yes (via ok path) |
| `define-private` | Same contract only | Yes |

```clarity
(define-read-only (get-count)
  (ok (var-get counter)))

(define-public (increment)
  (begin
    (var-set counter (+ (var-get counter) u1))
    (ok (var-get counter))))
```

### Responses (critical)

Public functions return `(response T uint)` — success `(ok value)` or error `(err uint)`:

```clarity
(define-constant ERR_UNAUTHORIZED (err u401))

(define-public (admin-only)
  (begin
    (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_UNAUTHORIZED)
    (ok true)))
```

- `asserts!` — aborts with err if condition false
- `try!` — unwrap ok or propagate err

### Built-in principals & context

| Name | Meaning |
|------|---------|
| `tx-sender` | Principal that signed the transaction |
| `contract-caller` | Immediate caller (another contract if applicable) |
| `as-contract` | Run as current contract |

### Fungible tokens (SIP-010 building blocks)

```clarity
(define-fungible-token my-token)
(ft-mint? my-token amount recipient)
(ft-transfer? my-token amount sender recipient)
(ft-get-balance my-token who)
(ft-get-supply my-token)
```

### Non-fungible tokens (SIP-009 building blocks)

```clarity
(define-non-fungible-token my-nft uint)
(nft-mint? my-nft token-id recipient)
(nft-transfer? my-nft token-id sender recipient)
(nft-get-owner? my-nft token-id)
```

### Traits (standards)

```clarity
;; Always use the FULL deployed trait path on mainnet — see sip-standards.md
;; WRONG: (impl-trait 'sip-010-trait)  → VM Error: use of undeclared trait
;; Testnet: omit impl-trait (no standard trait deployed on testnet chain)
;; Mainnet:
(impl-trait 'SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard.sip-010-trait)
(use-trait token-trait 'SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard.sip-010-trait)

(define-public (call-token (token <token-trait>))
  (contract-call? token transfer u100 tx-sender recipient none))
```

Scaffold SIP templates ship with correct `impl-trait` and `[[project.requirements]]` — prefer `stacksdapp add --template sip010|sip009` over hand-written tokens. See [sip-standards.md](sip-standards.md).

### Maps, lists, optional

```clarity
(define-map balances principal uint)
(map-set balances who amount)
(map-get? balances who)

(define-public (example (maybe-id (optional uint)))
  (match maybe-id id (ok id) ERR_NONE))
```

## Testing in scaffold (TypeScript + simnet)

```typescript
import { describe, expect, it } from "vitest";
import { Cl } from "@stacks/transactions";

const deployer = simnet.getAccounts().get("deployer")!;

it("increments", () => {
  const { result } = simnet.callPublicFn("counter", "increment", [], deployer);
  expect(result).toBeOk(Cl.uint(1));
});
```

- `simnet.callPublicFn(contract, fn, args, sender)`
- `simnet.callReadOnlyFn(contract, fn, args, sender)`
- Use `Cl.uint`, `Cl.standardPrincipal`, etc. for args

More: [Clarinet testing guide](https://docs.stacks.co/clarinet/testing-with-clarinet-sdk.md) · [Rendezvous fuzz testing](https://docs.stacks.co/rendezvous/overview.md)

## Security reminders

- Contracts are **immutable** after deploy — test thoroughly first
- Use `asserts!` / `try!` — never ignore error paths
- Prefer explicit access control (`tx-sender`, `contract-owner`)
- Run [check_checker](https://docs.stacks.co/clarinet/check-checker.md) — enabled in scaffold `Clarinet.toml` by default

## Clarity versions in scaffold

New projects default to **Clarity 6** (`epoch = "4.0"`). See [clarity-versions.md](clarity-versions.md) for C5/C4 downgrades.

## Related skill files

- Token standards (SIP-010 / SIP-009): [sip-standards.md](sip-standards.md)
- Frontend hooks after deploy: [frontend.md](frontend.md)
- CLI commands: [cli-reference.md](cli-reference.md)
