# Stacks.js — Connect, transactions & post-conditions

Scaffold frontend uses **`@stacks/connect` v8**, **`@stacks/transactions` v7**, **`@stacks/network` v7**. Generated code lives in `frontend/src/generated/contracts.ts` and `hooks.ts`. This file maps official Stacks.js docs to scaffold patterns.

**Full reference index:** [stacks-docs-index.md](stacks-docs-index.md) · [Stacks.js overview](https://docs.stacks.co/stacks.js/overview.md)

## Packages in scaffold projects

| Package | Role in scaffold |
|---------|------------------|
| `@stacks/connect` | Wallet connect + `request('stx_callContract', …)` on testnet/mainnet |
| `@stacks/transactions` | `Cl.*` args, `fetchCallReadOnlyFunction`, `cvToValue`, post-conditions |
| `@stacks/network` | `createNetwork`, Hiro API key — see `scaffold.config.ts` |

Devnet **does not** use Connect for writes — see [frontend.md](frontend.md) (`lib/devnet.ts` burners).

## Stacks Connect (wallet)

| Task | Doc |
|------|-----|
| Connect wallet | [Connect Wallet](https://docs.stacks.co/stacks-connect/connect-wallet.md) |
| Authentication (SIWE-style) | [Authentication](https://docs.stacks.co/get-started/build-a-frontend/authentication.md) |
| Call contract via wallet | [Broadcast Transactions](https://docs.stacks.co/stacks-connect/broadcast-transactions.md) |
| Send transactions (guide) | [Sending Transactions](https://docs.stacks.co/get-started/build-a-frontend/sending-transactions.md) |
| `stx_callContract` | [stx_callContract](https://docs.stacks.co/reference/stacks.js/stacks-connect/methods/stx_callcontract.md) |
| Message signing | [Message Signing](https://docs.stacks.co/stacks-connect/message-signing.md) |
| Wallet support matrix | [Wallet Support](https://docs.stacks.co/stacks-connect/wallet-support.md) |
| v7 → v8 migration | [Migration Guide](https://docs.stacks.co/stacks-connect/migration-guide.md) |

Scaffold template: `frontend/src/components/WalletConnect.tsx`, `store/wallet.ts`.

Generated public calls (testnet/mainnet):

```typescript
import { request } from '@stacks/connect';

await request('stx_callContract', {
  contract: `${address}.${contractName}`,
  functionName: 'increment',
  functionArgs: [], // ClarityValue[]
  postConditions: [],
  postConditionMode: 'allow',
  network: scaffoldConfig.targetNetwork,
});
```

## Read-only calls

| Task | Doc |
|------|-----|
| Read-only calls guide | [Read Only Calls](https://docs.stacks.co/stacks.js/read-only-calls.md) |
| `fetchCallReadOnlyFunction` | [fetchCallReadOnlyFunction](https://docs.stacks.co/reference/stacks.js/stacks-transactions/network/fetchcallreadonlyfunction.md) |
| Decode results | [cvToValue](https://docs.stacks.co/reference/stacks.js/stacks-transactions/utilities/cvtovalue.md) · [cvToJSON](https://docs.stacks.co/reference/stacks.js/stacks-transactions/utilities/cvtojson.md) |

Scaffold generated read-only functions use `cvToValue` — hook `data` shapes are documented in [frontend.md](frontend.md) (do not `BigInt(hook.data)` blindly).

## Building Clarity values (`Cl.*`)

| Task | Doc |
|------|-----|
| Encoding & decoding | [Encoding & Decoding](https://docs.stacks.co/stacks.js/encoding-and-decoding.md) |
| Clarity values reference | [Clarity Values](https://docs.stacks.co/reference/stacks.js/stacks-transactions/clarity-values.md) |

Common in scaffold hooks:

```typescript
import { Cl } from '@stacks/transactions';

Cl.uint(100)
Cl.int(-1)
Cl.principal('ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM')
Cl.standardPrincipal(address)
Cl.contractPrincipal('SP…', 'contract-name')
Cl.none()
Cl.some(Cl.uint(1))
Cl.tuple({ 'field': Cl.uint(1) })
Cl.list([Cl.uint(1), Cl.uint(2)])
```

## Post-conditions

Post-conditions protect users from unexpected token/STX transfers during contract calls.

| Task | Doc |
|------|-----|
| Concept | [Post-Conditions Overview](https://docs.stacks.co/post-conditions/overview.md) |
| Implementation | [Implementation](https://docs.stacks.co/post-conditions/implementation.md) |
| Examples | [Examples](https://docs.stacks.co/post-conditions/examples.md) |
| Frontend guide | [Post-Conditions (frontend)](https://docs.stacks.co/get-started/build-a-frontend/post-conditions.md) |
| `Pc` builder API | [Post Conditions](https://docs.stacks.co/reference/stacks.js/stacks-transactions/post-conditions.md) |
| Cookbook: FT PC | [Build an ft pc](https://docs.stacks.co/cookbook/stacks.js/cryptography-and-security/build-an-ft-pc.md) |

Scaffold **hooks** default to `postConditionMode: 'allow'` with empty post-conditions. For explicit PCs, call **`contracts.ts` directly** — see [frontend.md](frontend.md).

```typescript
import { Pc, PostConditionMode } from '@stacks/transactions';
import { myToken_transfer } from '@/generated/contracts';

await myToken_transfer(
  [Cl.uint(100), Cl.principal(sender), Cl.principal(recipient), Cl.none()],
  [Pc.principal(sender).willSendLte(100).ft('ST….token', 'my-token')],
);
```

## Networks & Hiro API

| Task | Doc |
|------|-----|
| Networks | [Networks](https://docs.stacks.co/stacks.js/networks.md) |
| Network configuration | [Network Configuration](https://docs.stacks.co/stacks.js/network-configuration.md) |
| `@stacks/network` ref | [stacks-network](https://docs.stacks.co/reference/stacks.js/stacks-network.md) |

Scaffold: `frontend/src/scaffold.config.ts` — `getReadOnlyNetwork()`, optional `NEXT_PUBLIC_HIRO_API_KEY`.

Default node URLs:

| Network | URL |
|---------|-----|
| Testnet | `https://api.testnet.hiro.so` |
| Mainnet | `https://api.hiro.so` |
| Devnet | `http://localhost:3999` |

## Contract deploy & calls (low-level)

| Task | Doc |
|------|-----|
| Build transactions | [Build Transactions](https://docs.stacks.co/stacks.js/build-transactions.md) |
| Contract calls | [Contract Calls](https://docs.stacks.co/stacks.js/contract-calls.md) |
| Contract deployment | [Contract Deployment](https://docs.stacks.co/stacks.js/contract-deployment.md) |
| `broadcastTransaction` | [broadcastTransaction](https://docs.stacks.co/reference/stacks.js/stacks-transactions/network/broadcasttransaction.md) |

Prefer **`stacksdapp deploy`** for contracts; use Stacks.js deploy only for custom tooling.

## SIP-010 / SIP-009 via Connect

| Task | Doc |
|------|-----|
| Transfer SIP-010 FT | [stx_transferSip10Ft](https://docs.stacks.co/reference/stacks.js/stacks-connect/methods/stx_transfersip10ft.md) |
| Transfer SIP-009 NFT | [stx_transferSip9Nft](https://docs.stacks.co/reference/stacks.js/stacks-connect/methods/stx_transfersip9nft.md) |
| Cookbook: SIP-010 transfer | [Transfer a SIP10 token](https://docs.stacks.co/cookbook/stacks.js/token-transfers/transfer-a-sip10-token.md) |

Scaffold projects usually use **generated hooks** wrapping contract `transfer` — see [sip-standards.md](sip-standards.md).

## Agent rules

- Prefer generated `hooks.ts` / `contracts.ts` over reimplementing Stacks.js in app code
- Match `scaffoldConfig.network` / wallet network / `deployments.json` chain (ST vs SP)
- For API details not listed here, fetch [docs.stacks.co/llms.txt](https://docs.stacks.co/llms.txt) or use `?ask=` on the relevant `.md` page
- Clarity-side work: [clarity-language.md](clarity-language.md) · [sip-standards.md](sip-standards.md)
