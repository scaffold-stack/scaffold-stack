# Stacks official docs — index & fetch workflow

Scaffold Stacks skill files cover **CLI, project layout, frontend hooks, SIP templates, and troubleshooting**. For everything else on Stacks, use the **official docs index** — same catalog as [docs.stacks.co/llms.txt](https://docs.stacks.co/llms.txt).

## Agent workflow (required when docs are not in this skill)

1. **Check local skill first** — [SKILL.md](SKILL.md), [frontend.md](frontend.md), [clarity-language.md](clarity-language.md), [sip-standards.md](sip-standards.md), [stacks-js.md](stacks-js.md), [stacks-clarinet-docs.md](stacks-clarinet-docs.md)
2. **If not covered** — fetch the index: `https://docs.stacks.co/llms.txt`
3. **Open the matching `.md` URL** from the index (append `.md` to doc paths)
4. **Or ask GitBook directly:**

```http
GET https://docs.stacks.co/<path-to-page>.md?ask=<specific question>&goal=<what you are building>
```

Example:

```http
GET https://docs.stacks.co/stacks.js/read-only-calls.md?ask=How do I decode fetchCallReadOnlyFunction results?&goal=Fix frontend balance display in a scaffold project
```

**Never invent Clarity syntax, SIP trait addresses, or Stacks.js APIs** — fetch or use `?ask=`.

## Two doc indexes (do not confuse)

| Index | URL | Use for |
|-------|-----|---------|
| **Scaffold Stacks** | https://scaffoldstacks.mintlify.app/llms.txt | `stacksdapp` CLI, this template, deploy workflows |
| **Stacks (Hiro)** | https://docs.stacks.co/llms.txt | Clarity, Clarinet, Stacks.js, sBTC, APIs, PoX |

## Section map (docs.stacks.co/llms.txt)

Curated entry points per section. For the full link list, fetch `llms.txt`.

### Learn — concepts & network

| Topic | Doc |
|-------|-----|
| What is Stacks / Bitcoin L2 | [Stacks 101](https://docs.stacks.co/learn/stacks-101/what-is-stacks.md) |
| Mainnet vs testnet | [Mainnet and Testnets](https://docs.stacks.co/learn/network-fundamentals/mainnet-and-testnets.md) |
| Wallets & accounts | [Wallets & Accounts](https://docs.stacks.co/learn/network-fundamentals/wallets-and-accounts.md) |
| How transactions work | [How Transactions Work](https://docs.stacks.co/learn/transactions/how-transactions-work.md) |
| Post-conditions (concept) | [Post Conditions](https://docs.stacks.co/learn/transactions/post-conditions.md) |
| Clarity overview | [Clarity](https://docs.stacks.co/learn/clarity.md) |
| SIPs list | [SIPs](https://docs.stacks.co/learn/network-fundamentals/sips.md) |
| Nakamoto / block production | [What was the Nakamoto Upgrade?](https://docs.stacks.co/learn/block-production/what-was-the-nakamoto-upgrade.md) |
| sBTC overview | [sBTC](https://docs.stacks.co/learn/sbtc.md) |
| Bridging / USDCx | [Bridging](https://docs.stacks.co/learn/bridging.md) |

### Build — dApp development (beyond scaffold CLI)

| Topic | Doc | Local skill |
|-------|-----|-------------|
| Developer quickstart | [Developer Quickstart](https://docs.stacks.co/get-started/developer-quickstart.md) | [workflows.md](workflows.md) |
| Clarity crash course | [Clarity Crash Course](https://docs.stacks.co/get-started/clarity-crash-course.md) | [clarity-language.md](clarity-language.md) |
| Fungible tokens | [Fungible Tokens](https://docs.stacks.co/get-started/create-a-token/fungible-tokens.md) | [sip-standards.md](sip-standards.md) |
| NFTs | [Non-Fungible Tokens](https://docs.stacks.co/get-started/create-a-token/non-fungible-tokens.md) | [sip-standards.md](sip-standards.md) |
| Semi-fungible tokens | [Semi-Fungible Tokens](https://docs.stacks.co/get-started/create-a-token/semi-fungible-tokens.md) | — |
| Frontend / auth / txs | [Build a Frontend](https://docs.stacks.co/get-started/build-a-frontend.md) | [frontend.md](frontend.md) |
| Wallet authentication | [Authentication](https://docs.stacks.co/get-started/build-a-frontend/authentication.md) | [stacks-js.md](stacks-js.md) |
| Sending transactions (frontend) | [Sending Transactions](https://docs.stacks.co/get-started/build-a-frontend/sending-transactions.md) | [stacks-js.md](stacks-js.md) |
| Use cases (DeFi, gaming, etc.) | [Use Cases](https://docs.stacks.co/get-started/use-cases.md) | fetch on demand |
| Post-conditions (frontend) | [Post-Conditions](https://docs.stacks.co/get-started/build-a-frontend/post-conditions.md) | [stacks-js.md](stacks-js.md) |
| Path to production | [Path to Production](https://docs.stacks.co/get-started/path-to-production.md) | [troubleshooting.md](troubleshooting.md) |
| Clarinet overview | [Clarinet Overview](https://docs.stacks.co/clarinet/overview.md) | [stacks-clarinet-docs.md](stacks-clarinet-docs.md) |
| Clarinet testing | [Unit Testing](https://docs.stacks.co/clarinet/testing-with-clarinet-sdk.md) | [stacks-clarinet-docs.md](stacks-clarinet-docs.md) |
| Clarinet deploy | [Contract Deployment](https://docs.stacks.co/clarinet/contract-deployment.md) | [cli-reference.md](cli-reference.md) |
| Rendezvous fuzzing | [Rendezvous Overview](https://docs.stacks.co/rendezvous/overview.md) | [stacks-clarinet-docs.md](stacks-clarinet-docs.md) |
| Stacks.js overview | [Stacks.js Overview](https://docs.stacks.co/stacks.js/overview.md) | [stacks-js.md](stacks-js.md) |
| Stacks Connect | [Connect Wallet](https://docs.stacks.co/stacks-connect/connect-wallet.md) | [frontend.md](frontend.md) |
| Post-conditions guide | [Post-Conditions Overview](https://docs.stacks.co/post-conditions/overview.md) | [stacks-js.md](stacks-js.md) |
| sBTC builder guide | [sBTC Builder Quickstart](https://docs.stacks.co/more-guides/sbtc/sbtc-builder-quickstart.md) | fetch on demand |
| Price oracles (Pyth/DIA) | [Price Oracles](https://docs.stacks.co/more-guides/price-oracles.md) | fetch on demand |
| Verify BTC txs in Clarity | [Verify Bitcoin Transactions in Clarity](https://docs.stacks.co/more-guides/verify-bitcoin-transactions-clarity.md) | fetch on demand |
| USDCx bridging | [Bridging USDCx](https://docs.stacks.co/more-guides/bridging-usdcx.md) | fetch on demand |
| c32 address encoding | [c32check](https://docs.stacks.co/more-guides/c32check.md) | fetch on demand |
| Embedded wallets (Turnkey) | [Signing with Turnkey](https://docs.stacks.co/more-guides/onboarding/signing-with-turnkey.md) | fetch on demand |
| Devtools catalog | [Stacks Devtools Catalog](https://docs.stacks.co/stacks-devtools-catalog.md) | — |

### Operate — nodes, signers (usually not scaffold dApp work)

| Topic | Doc |
|-------|-----|
| Run a node | [Run a Node](https://docs.stacks.co/operate/run-a-node.md) |
| Run a signer | [Run a Signer](https://docs.stacks.co/operate/run-a-signer.md) |
| PoX-5 upgrade | [PoX-5 Upgrade Guide](https://docs.stacks.co/operate/run-a-signer/pox-5-upgrade-guide.md) |
| Staking STX | [Staking STX](https://docs.stacks.co/operate/staking-stx.md) |

Scaffold **devnet** uses Clarinet Docker — not a production node. Prefer **testnet** for deploy verification.

### Reference — APIs & language spec

| Topic | Doc |
|-------|-----|
| Clarity functions | [Functions](https://docs.stacks.co/reference/clarity/functions.md) |
| Clarity types | [Types](https://docs.stacks.co/reference/clarity/types.md) |
| Clarity keywords | [Keywords](https://docs.stacks.co/reference/clarity/keywords.md) |
| Stacks Blockchain API | [Stacks Blockchain API](https://docs.stacks.co/reference/api/stacks-blockchain-api.md) |
| Node RPC | [Stacks Node RPC](https://docs.stacks.co/reference/api/stacks-node-rpc.md) |
| `@stacks/transactions` | [Reference](https://docs.stacks.co/reference/stacks.js/stacks-transactions.md) |
| `@stacks/connect` | [Reference](https://docs.stacks.co/reference/stacks.js/stacks-connect.md) |
| `cvToValue` / `cvToJSON` | [cvToValue](https://docs.stacks.co/reference/stacks.js/stacks-transactions/utilities/cvtovalue.md) |
| Clarinet CLI ref | [CLI Reference](https://docs.stacks.co/reference/clarinet/cli-reference.md) |
| Clarinet SDK ref | [SDK Reference](https://docs.stacks.co/reference/clarinet-js-sdk/sdk-reference.md) |

### Tutorials & Cookbook

| Topic | Doc |
|-------|-----|
| Tutorials hub | [Welcome to Tutorials](https://docs.stacks.co/tutorials/readme.md) |
| Full-stack primer | [Getting Started with Stacks](https://docs.stacks.co/tutorials/bitcoin-primer/getting-started-with-stacks.md) |
| Cookbook hub | [Welcome to the Cookbook](https://docs.stacks.co/cookbook/readme.md) |
| SIP-010 transfer (JS) | [Transfer a SIP10 token](https://docs.stacks.co/cookbook/stacks.js/token-transfers/transfer-a-sip10-token.md) |
| Example Clarity contracts | [Example Contracts](https://docs.stacks.co/cookbook/clarity/example-contracts.md) |
| Post-condition recipes | [Build an ft pc](https://docs.stacks.co/cookbook/stacks.js/cryptography-and-security/build-an-ft-pc.md) |

## When to fetch vs use local skill

| Question type | Start here |
|---------------|------------|
| `stacksdapp` command, exit code, deploy | [SKILL.md](SKILL.md) · [cli-reference.md](cli-reference.md) |
| Generated hooks, wallet, `Failed to fetch` | [frontend.md](frontend.md) |
| SIP-010/009 templates, `impl-trait` | [sip-standards.md](sip-standards.md) |
| Clarity syntax cheat sheet | [clarity-language.md](clarity-language.md) |
| Stacks.js / Connect / post-conditions code | [stacks-js.md](stacks-js.md) |
| Clarinet features (not stacksdapp CLI) | [stacks-clarinet-docs.md](stacks-clarinet-docs.md) |
| sBTC, oracles, node ops, anything else | **Fetch** [llms.txt](https://docs.stacks.co/llms.txt) → open page → or `?ask=` |
