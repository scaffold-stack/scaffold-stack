# Frontend — hooks, wallet, and infra

Next.js 15 + React 18 + Tailwind + Jotai + `@stacks/connect` v8 + `@stacks/transactions` v7.

After `stacksdapp generate` and `stacksdapp deploy`, the frontend calls deployed contracts via **reusable generated hooks**.

## Full-stack checklist (custom UI + hooks)

Use this end-to-end when building a dApp with your own frontend (not only the debug panel):

```
1. stacksdapp new my-app && cd my-app
2. Edit contracts/contracts/*.clar  (or stacksdapp add …)
3. stacksdapp check && stacksdapp generate && stacksdapp test
4. Set deployer mnemonic in contracts/settings/Testnet.toml
5. stacksdapp deploy --network testnet --yes   → writes deployments.json
6. Create frontend/src/components/YourFeature.tsx  ("use client", import hooks)
7. Add <YourFeature /> to frontend/src/app/page.tsx (or a new app/*/page.tsx)
8. stacksdapp dev --network testnet   # MUST match deploy network (not bare dev if deployed to testnet)
9. Connect wallet (testnet/mainnet) → call public functions from your component
```

Keep `<DebugContracts />` on the home page while building — it validates hooks work; ship custom UI alongside or replace it later.

## Architecture

```
contracts/*.clar
    ↓ stacksdapp generate
frontend/src/generated/
    contracts.ts      ← low-level call functions
    hooks.ts          ← React hooks wrapping contracts.ts
    DebugContracts.tsx← debug UI (uses hooks)
    deployments.json  ← on-chain addresses (written by deploy)
         ↓
frontend/src/components/   ← your custom UI (edit here)
frontend/src/app/          ← Next.js pages
frontend/src/lib/devnet.ts ← devnet burner signing (hand-edited)
frontend/src/scaffold.config.ts ← network config (hand-edited)
```

**Rule:** Never edit `generated/*`. Build custom UI in `components/` and import from `@/generated/hooks` or `@/generated/contracts`.

## Imports and paths

`frontend/tsconfig.json` maps `@/*` → `./src/*`:

```tsx
import { useCounter_Increment } from '@/generated/hooks';
import { counter_increment } from '@/generated/contracts';
import { scaffoldConfig } from '@/scaffold.config';
import { useAtomValue } from 'jotai';
import { addressAtom } from '@/store/wallet';
```

## Discovering hooks after generate

Each public/read-only function gets one hook in `frontend/src/generated/hooks.ts`:

| Contract `counter` | Function `increment` | Hook `useCounter_Increment` |
| Contract `my-token` | Function `transfer` | Hook `useMyToken_Transfer` |

Naming: `use` + `{ContractPascalCase}` + `_` + `{FunctionPascalCase}`.

After `stacksdapp add my-nft` → `stacksdapp generate` → new hooks appear automatically. Search `hooks.ts` or let TypeScript autocomplete imports.

## deployments.json

Contract calls resolve addresses from `frontend/src/generated/deployments.json`:

```json
{ "contracts": { "counter": { "contract_id": "ST....counter" } } }
```

- Written by `stacksdapp deploy`
- If missing, `contracts.ts` logs a warning and calls return `undefined` / `null`
- After redeploy with auto-versioning (`counter-v2`), regenerate is automatic via deploy; run `stacksdapp generate` if bindings drift

## scaffold.config.ts

Driven by `frontend/.env.local`:

```bash
NEXT_PUBLIC_NETWORK=devnet   # devnet | testnet | mainnet
# NEXT_PUBLIC_STACKS_NODE_URL=...   # optional override
# NEXT_PUBLIC_HIRO_API_KEY=...      # optional Hiro API key for read calls
```

Exports `scaffoldConfig`:

| Field | Purpose |
|-------|---------|
| `network` | Active network |
| `nodeUrl` | Hiro API (devnet: `http://localhost:3999`) |
| `targetNetwork` | Wallet request network (`testnet` when UI is devnet) |
| `isDevnet` / `isTestnet` / `isMainnet` | Branching helpers |
| `getReadOnlyNetwork()` | Network object for read-only RPC calls |

`stacksdapp dev --network testnet` updates `.env.local` automatically.

## Signing model (critical)

| Network | Public (write) calls | Read-only calls |
|---------|----------------------|-----------------|
| **devnet** | `lib/devnet.ts` — public burner keys, no wallet popup | `fetchCallReadOnlyFunction` via node |
| **testnet/mainnet** | `@stacks/connect` `request('stx_callContract')` — **wallet popup** | `fetchCallReadOnlyFunction` via Hiro |

**Do not** expect Leather/Xverse to sign devnet writes — devnet uses template burner mnemonics (same as `contracts/settings/Devnet.toml`). Wallet connect is for testnet/mainnet UX and display.

## contracts.ts (generated)

One async function per contract function:

```ts
// Naming: {contractCamel}_{functionCamel}
counter_increment(functionArgs, postConditions?)
counter_getCount(functionArgs, senderAddress?)
```

- **Public:** devnet → `callDevnetContract`; testnet/mainnet → wallet `request()`
- **Read-only:** always RPC via `fetchCallReadOnlyFunction` + `cvToValue`
- Import `Cl` from `@stacks/transactions` to build args (see table below)
- Read-only `data` is JSON from `cvToValue` (numbers, booleans, objects, `(ok …)` / `(err …)` shapes)

### Parsing hook `data` — avoid `BigInt([object Object])`

Read-only hooks (e.g. `useMyToken_GetBalance`, `useMyToken_GetTotalSupply`) return **`cvToValue` output**, not plain JavaScript numbers.

For Clarity `(response uint uint)` functions like SIP-010 `get-balance`, `data` is typically a **cvToJSON-shaped object**, not a `bigint`:

```ts
// hook.data after get-balance — NOT a bigint
{ type: "uint", value: "1500000" }
```

**Wrong** (throws `Cannot convert [object Object] to a BigInt`):

```tsx
function formatTokenAmount(raw: unknown, decimals = 6): string {
  const value = typeof raw === "bigint" ? raw : BigInt(String(raw)); // raw is { type, value }
  // ...
}
```

**Right** — unwrap cvToJSON shapes first, then format:

```tsx
/** Extract a Clarity uint/int from hook `data` (cvToValue / cvToJSON shapes). */
function clarityUintToBigInt(raw: unknown): bigint | null {
  if (raw === null || raw === undefined) return null;
  if (typeof raw === "bigint") return raw;
  if (typeof raw === "number" && Number.isFinite(raw)) return BigInt(Math.trunc(raw));
  if (typeof raw === "string" && raw !== "") return BigInt(raw);

  if (typeof raw === "object") {
    const obj = raw as { type?: string; value?: unknown };
    // (ok uint) / (ok int) from read-only calls
    if (obj.type === "uint" || obj.type === "int") {
      return BigInt(String(obj.value));
    }
    // (optional uint), nested some, etc.
    if ("value" in obj && obj.value != null) {
      return clarityUintToBigInt(obj.value);
    }
  }
  return null;
}

function formatTokenAmount(raw: unknown, decimals = 6): string {
  const value = clarityUintToBigInt(raw);
  if (value === null) return "—";
  const whole = value / BigInt(10 ** decimals);
  const fraction = value % BigInt(10 ** decimals);
  const fractionStr = fraction
    .toString()
    .padStart(decimals, "0")
    .replace(/0+$/, "");
  return fractionStr ? `${whole}.${fractionStr}` : String(whole);
}
```

Usage with generated hooks:

```tsx
"use client";
import { useEffect } from "react";
import { useMyToken_GetBalance } from "@/generated/hooks";
import { Cl } from "@stacks/transactions";
import { useAtomValue } from "jotai";
import { addressAtom } from "@/store/wallet";

export function BalanceDisplay() {
  const address = useAtomValue(addressAtom);
  const { call, data, loading } = useMyToken_GetBalance();

  useEffect(() => {
    if (address) void call([Cl.principal(address)]);
  }, [call, address]);

  if (loading) return <p>Loading…</p>;
  return <p>Balance: {formatTokenAmount(data)}</p>; // pass hook.data, not data.value blindly
}
```

**Agent rules for display helpers:**

| `data` shape | What to do |
|--------------|------------|
| `{ type: "uint", value: "123" }` | Use `BigInt(obj.value)` — SIP-010 amounts are **base units** (divide by `10**decimals` for display) |
| `{ type: "(optional …)", value: … }` | Recurse into `.value` |
| `{ type: "bool", value: true }` | Use `.value` directly — do not `BigInt()` |
| `string` / `bigint` | `BigInt(raw)` after null check |
| Unsure | `console.log(JSON.stringify(data))` once, then write extractor |

**Never** `BigInt(hook.data)` or `BigInt(String(hook.data))` when `data` comes from a read-only hook — always unwrap first.

### Read-only RPC — avoid `Failed to fetch`

Generated read-only functions (e.g. `airdropTokenV2_getBalance` in `contracts.ts`) call Hiro / local node via `fetchCallReadOnlyFunction`. Browser `TypeError: Failed to fetch` means the **HTTP request never succeeded** — not a Clarity revert.

Typical stack trace:

```
Failed to fetch
src/generated/contracts.ts … fetchCallReadOnlyFunction({ … functionName: 'get-balance'
```

#### Root causes (check in order)

| Cause | Symptom | Fix |
|-------|---------|-----|
| **Network env mismatch** | Deployed to testnet, app still on devnet (`localhost:3999`) | Set `NEXT_PUBLIC_NETWORK=testnet` in `frontend/.env.local`, or run `stacksdapp dev --network testnet` (updates env automatically). **Restart** Next.js after changing env. |
| **Devnet node down** | Request to `http://localhost:3999/...` fails | Run `stacksdapp dev` (Docker). Do not use `npm run dev` alone on devnet without the stacks node. |
| **Contract not deployed** | `deployments.json` missing entry; console warns `"airdrop-token-v2" not deployed` | `stacksdapp deploy --network testnet --contract airdrop-token-v2 --yes` |
| **Wrong chain for address** | `deployments.json` has `ST…` contract but app points at mainnet (or vice versa) | Match `NEXT_PUBLIC_NETWORK` to deploy network; ST* = testnet, SP* = mainnet |
| **Invalid / missing principal arg** | `get-balance` called with `undefined`, `""`, or before wallet connect | Guard: only call when address is a valid `ST…` / `SP…` principal |
| **Hiro rate limit / blocker** | Intermittent failures to `api.testnet.hiro.so` | Add `NEXT_PUBLIC_HIRO_API_KEY=…` in `.env.local`; disable ad-blocker for Hiro domains |
| **Calling from server** | Error during SSR / in non-client component | Read-only hooks only in `"use client"` components |

**Agent rule:** After `stacksdapp deploy --network testnet`, always run `stacksdapp dev --network testnet` (not bare `stacksdapp dev` which defaults to devnet).

#### Pre-flight before read-only hooks

```bash
# 1. Contract deployed on intended network
cat frontend/src/generated/deployments.json   # must contain "airdrop-token-v2": { "contract_id": "ST…" }

# 2. Frontend network matches deploy
grep NEXT_PUBLIC_NETWORK frontend/.env.local  # testnet if contract_id starts with ST

# 3. Node reachable (pick one)
curl -s -o /dev/null -w "%{http_code}" https://api.testnet.hiro.so/v2/info   # expect 200
curl -s -o /dev/null -w "%{http_code}" http://localhost:3999/v2/info        # devnet only, expect 200 when stacksdapp dev is up
```

#### Safe read-only hook pattern (guards + error UI)

Do **not** fire `get-balance` on mount unconditionally. Guard address, deployment, and network:

```tsx
"use client";
import { useEffect } from "react";
import { useAirdropTokenV2_GetBalance } from "@/generated/hooks";
import { Cl } from "@stacks/transactions";
import { useAtomValue } from "jotai";
import { addressAtom } from "@/store/wallet";
import { scaffoldConfig } from "@/scaffold.config";
import deployments from "@/generated/deployments.json";

const CONTRACT = "airdrop-token-v2";

function isValidPrincipal(addr: string | null | undefined): addr is string {
  return typeof addr === "string" && /^(ST|SP)[0-9A-HJ-NP-Z]{38,41}$/.test(addr);
}

export function TokenBalance() {
  const walletAddress = useAtomValue(addressAtom);
  const { call, data, loading, error } = useAirdropTokenV2_GetBalance();

  const contractId = deployments?.contracts?.[CONTRACT]?.contract_id as string | undefined;
  const isDeployed = Boolean(contractId);

  useEffect(() => {
    if (!isDeployed) return;
    if (!isValidPrincipal(walletAddress)) return;
    // Avoid unhandled rejection noise — hook sets `error`
    void call([Cl.principal(walletAddress)]).catch(() => {});
  }, [call, walletAddress, isDeployed]);

  if (!isDeployed) {
    return (
      <p>
        {CONTRACT} not deployed — run{" "}
        <code>stacksdapp deploy --network testnet --contract {CONTRACT} --yes</code>
      </p>
    );
  }

  if (scaffoldConfig.isDevnet) {
    return (
      <p>
        Devnet mode — ensure <code>stacksdapp dev</code> is running (node: {scaffoldConfig.nodeUrl})
      </p>
    );
  }

  if (!isValidPrincipal(walletAddress)) {
    return <p>Connect wallet to view balance</p>;
  }

  if (loading) return <p>Loading balance…</p>;

  if (error) {
    return (
      <p>
        Could not load balance ({error.message}). Check{" "}
        <code>NEXT_PUBLIC_NETWORK={scaffoldConfig.network}</code> matches deploy network and Hiro is
        reachable.
      </p>
    );
  }

  return <p>Balance: {formatTokenAmount(data)}</p>;
}
```

#### Debugging in browser DevTools

1. **Network tab** — find the failed request URL:
   - `localhost:3999` → devnet not running or wrong `NEXT_PUBLIC_NETWORK`
   - `api.testnet.hiro.so` → Hiro outage, rate limit, or blocker
2. **Console** — look for `[scaffold-stacks] "…" not deployed` before the fetch error
3. **Compare** `deployments.json` `contract_id` prefix with `scaffoldConfig.network`

#### Optional: Hiro API key (rate limits)

In `frontend/.env.local`:

```bash
NEXT_PUBLIC_HIRO_API_KEY=your_hiro_platform_api_key
```

`scaffold.config.ts` passes this to `getReadOnlyNetwork()` for read-only calls.

### Building `ClarityValue[]` arguments

| Clarity type | TypeScript |
|--------------|------------|
| `uint` | `Cl.uint(n)` or `Cl.uint(BigInt(n))` |
| `int` | `Cl.int(n)` |
| `bool` | `Cl.bool(true)` |
| `principal` | `Cl.principal('ST…')` |
| `(string-ascii …)` | `Cl.stringAscii('hello')` |
| `(string-utf8 …)` | `Cl.stringUtf8('hello')` |
| `(buff …)` | `Cl.bufferFromHex('0x…')` |
| `(tuple (field-a …) …)` | `Cl.tuple({ 'field-a': Cl.uint(1), … })` |
| `(list …)` | `Cl.list([Cl.uint(1), Cl.uint(2)])` |
| `(optional …)` | `Cl.none()` or `Cl.some(Cl.uint(1))` |

Check the generated debug UI or contract ABI for exact field names. For complex args, reference `DebugContracts.tsx` form handling.

### Post-conditions

Generated **hooks** call public functions with default empty post-conditions (`[]`). Wallet requests use `postConditionMode: 'allow'`.

For explicit post-conditions, call **`contracts.ts` directly**:

```ts
import { counter_increment } from '@/generated/contracts';
import { Pc, PostConditionMode } from '@stacks/transactions';

await counter_increment([Cl.uint(1)], [
  Pc.principal('ST…').willSendLte(1000).ustx(),
]);
```

Devnet path uses `PostConditionMode.Deny` in `lib/devnet.ts`.

## hooks.ts (generated)

One hook per public/read-only function:

```ts
// Naming: use{ContractPascal}_{FunctionPascal}
import { useCounter_Increment } from '@/generated/hooks';

function MyButton() {
  const { call, data, loading, error, txid, txStatus, txStatusError, explorerUrl } =
    useCounter_Increment();

  return (
    <button
      disabled={loading}
      onClick={() => call([])}  // ClarityValue[] — empty if no args
    >
      {loading ? '…' : 'Increment'}
    </button>
  );
}
```

### Hook return values

| Field | Meaning |
|-------|---------|
| `call(args)` | Invoke the contract function |
| `data` | Last result |
| `loading` | In-flight |
| `error` | Thrown error |
| `txid` | Public calls only — broadcast tx id |
| `txStatus` | `pending` \| `success` \| `abort_by_response` \| `error` |
| `txStatusError` | Revert message when aborted |
| `explorerUrl` | Hiro explorer link for `txid` |

Public hooks poll `${nodeUrl}/extended/v1/tx/{txid}` until success/abort (respects `NEXT_PUBLIC_HIRO_API_KEY`).

Read-only hooks resolve on `call()` — no txid polling.

### Composing multiple hooks

Use several hooks in one client component — each manages its own loading/tx state:

```tsx
"use client";
import { useCounter_GetCount, useCounter_Increment } from '@/generated/hooks';

export function CounterPanel() {
  const read = useCounter_GetCount();
  const write = useCounter_Increment();
  // call read.call([]) to refresh; write.call([]) to increment
}
```

## Wallet infra (hand-edited)

```
app/layout.tsx
  └── WalletProvider          # syncs @stacks/connect → Jotai
        ├── Header            # WalletConnect + NetworkBadge
        └── page content

store/wallet.ts
  addressAtom                 # persisted STX address (localStorage)
  isMountedAtom               # SSR guard

components/WalletConnect.tsx
  connect() / disconnect()    # @stacks/connect v8
```

- `WalletConnect` shows connect button or truncated address
- User must connect wallet on **testnet/mainnet** before public calls from custom UI
- Debug UI on devnet works without wallet (burner keys)

### Show connected address in custom UI

```tsx
"use client";
import { useAtomValue } from 'jotai';
import { addressAtom } from '@/store/wallet';

export function MyPanel() {
  const address = useAtomValue(addressAtom);
  if (!address) return <p>Connect wallet to continue</p>;
  return <p>Signed in as {address}</p>;
}
```

## Debug UI

```
components/debug/DebugContracts.tsx   → re-exports @/generated/DebugContracts
app/page.tsx                          → renders <DebugContracts />
```

Auto-generated debug panel: tabs per contract, forms per function, shows tx status. Use as reference for hook usage; customize by building your own components importing the same hooks.

## Devnet burners (`lib/devnet.ts`)

- Derives keys from public template mnemonics (`deployer`, `wallet_1`…)
- `ensureDefaultBurner()` — default sender `wallet_1`
- `getDevnetSenderAddress()` — used as read-only sender on devnet
- `callDevnetContract()` — broadcasts with `PostConditionMode.Deny`

**Never** reuse devnet burners on testnet/mainnet.

## Custom UI workflow

1. Edit / add contracts → `stacksdapp check && stacksdapp generate && stacksdapp test`
2. Deploy → `stacksdapp deploy --network testnet --yes`
3. Create component in `frontend/src/components/`:

```tsx
"use client";
import { useCounter_GetCount } from '@/generated/hooks';
import { useEffect } from 'react';

export function CounterDisplay() {
  const { call, data, loading } = useCounter_GetCount();

  useEffect(() => { void call([]); }, [call]);

  if (loading) return <p>Loading…</p>;
  return <p>Count: {JSON.stringify(data)}</p>;
}
```

4. Import into `app/page.tsx` or create `app/dashboard/page.tsx` (Next.js App Router)
5. Run `stacksdapp dev --network testnet` (or devnet with Docker)

### SIP-010 / SIP-009 in custom UI

After `stacksdapp add my-token --template sip010` and deploy:

```tsx
"use client";
import { useMyToken_GetBalance, useMyToken_Transfer } from '@/generated/hooks';
import { Cl } from '@stacks/transactions';

export function TokenPanel() {
  const balance = useMyToken_GetBalance();
  const transfer = useMyToken_Transfer();
  // balance.call([Cl.principal('ST…')])
  // transfer.call([Cl.uint(100), Cl.principal('ST…')])
}
```

Same hook pattern for SIP-009 NFT contracts — see [sip-standards.md](sip-standards.md) for trait functions and hook arg examples.

### Contract tests (backend)

When adding functions, extend `contracts/tests/*.test.ts` using `simnet` + `Cl` from `@stacks/transactions`. Run `stacksdapp test` before deploy — no Docker required.

## Live reload

- **`stacksdapp dev`** (devnet): file watcher regenerates bindings when `.clar` files change
- **`stacksdapp dev --network testnet`**: run `stacksdapp generate --watch` in a second terminal, or regenerate manually after edits
- **`stacksdapp generate --watch`**: standalone ABI watcher from project root

After regenerate, new hooks appear in `hooks.ts` — refresh imports in your components.

## Direct contract calls (no hook)

For non-React code or one-off scripts inside the app:

```ts
import { counter_increment } from '@/generated/contracts';
import { Cl } from '@stacks/transactions';

await counter_increment([]);
```

Prefer hooks in React components for loading/error/tx state.

## Frontend scripts

```bash
cd frontend
npm run dev          # or stacksdapp dev from project root
npm run build
npm run typecheck
npm test             # vitest (optional frontend tests)
```

## Common agent mistakes

| Mistake | Fix |
|---------|-----|
| Edit `generated/hooks.ts` | Run `stacksdapp generate` |
| Expect wallet popup on devnet | Devnet uses burners in `lib/devnet.ts` |
| Call contract before deploy | Deploy first; check `deployments.json` |
| Wrong network in wallet vs app | Match wallet network to `NEXT_PUBLIC_NETWORK` |
| Skip `"use client"` in hook components | Hooks require client components |
| Build args as plain JS | Use `Cl.*` from `@stacks/transactions` |
| `BigInt(hook.data)` on read-only uint | Unwrap cvToJSON shape first — see **Parsing hook data** above |
| Pass `data` straight to `formatTokenAmount` without unwrapping | SIP-010 `get-balance` returns `{ type: "uint", value: "…" }`, not `bigint` |
| Fire read-only hooks on mount without guards | Wait for valid wallet address + deployed contract; handle `hook.error` |
| `stacksdapp dev` after testnet deploy | Use `stacksdapp dev --network testnet` so `.env.local` matches `deployments.json` |
| `npm run dev` alone on devnet | Needs `stacksdapp dev` — local node at `localhost:3999` |
| Remove DebugContracts too early | Keep it while validating new hooks |
| Forget redeploy after new contract | `deploy` then verify `deployments.json` |

## Skill coverage map

| Full-stack need | Documented in |
|-----------------|---------------|
| CLI: new, add, check, generate, test, deploy | `SKILL.md`, `workflows.md`, `cli-reference.md` |
| Clarity versions / epochs | `clarity-versions.md` |
| Clarity language + learning links | `clarity-language.md` |
| SIP-010 / SIP-009 specs + templates | `sip-standards.md` |
| Generated hooks + custom components | `frontend.md` |
| Stacks.js / Connect / post-conditions | `stacks-js.md` |
| Clarinet (official docs) | `stacks-clarinet-docs.md` |
| Full Stacks docs catalog | `stacks-docs-index.md` → [llms.txt](https://docs.stacks.co/llms.txt) |
| Wallet vs devnet signing | this file |
| Project directories | `project-layout.md` |
| Errors / devnet stall | `troubleshooting.md` |
