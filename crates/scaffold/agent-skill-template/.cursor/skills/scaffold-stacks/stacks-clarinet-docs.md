# Clarinet — official docs map (beyond stacksdapp CLI)

Scaffold wraps Clarinet via **`stacksdapp check`**, **`test`**, **`deploy`**, and **`dev`**. This file maps **Hiro Clarinet documentation** for when agents need Clarinet features not covered in [cli-reference.md](cli-reference.md).

**Index:** [stacks-docs-index.md](stacks-docs-index.md) · [Clarinet overview](https://docs.stacks.co/clarinet/overview.md)

## stacksdapp vs raw Clarinet

| Goal | Prefer | Clarinet doc (if needed) |
|------|--------|--------------------------|
| Type-check | `stacksdapp check` | [Validation and Analysis](https://docs.stacks.co/clarinet/validation-and-analysis.md) |
| Unit tests | `stacksdapp test` | [Unit Testing](https://docs.stacks.co/clarinet/testing-with-clarinet-sdk.md) |
| Local chain | `stacksdapp dev` | [Local Blockchain Development](https://docs.stacks.co/clarinet/local-blockchain-development.md) |
| Deploy testnet/mainnet | `stacksdapp deploy --yes` | [Contract Deployment](https://docs.stacks.co/clarinet/contract-deployment.md) |
| New project layout | `stacksdapp new` | [Project Structure](https://docs.stacks.co/clarinet/project-structure.md) |

**Agent rule:** Do not tell users to run raw `clarinet devnet start` alongside `stacksdapp dev` — port conflicts. See [troubleshooting.md](troubleshooting.md).

## Version requirements (scaffold defaults)

| Tool | Version | Why |
|------|---------|-----|
| **Clarinet** | **3.23+** | Clarity 6 devnet / epoch 4.0 snapshot (burn ≥ 163) |
| `@stacks/clarinet-sdk` | **3.23.1** | Matches Clarinet in `contracts/package.json` |
| Default contract | Clarity **6**, epoch **4.0** | See [clarity-versions.md](clarity-versions.md) |

`stacksdapp doctor` warns if Clarinet is below 3.23.

## Testing (Vitest + simnet)

| Topic | Doc |
|-------|-----|
| Testing guide | [Unit Testing](https://docs.stacks.co/clarinet/testing-with-clarinet-sdk.md) |
| SDK reference | [SDK Reference](https://docs.stacks.co/reference/clarinet-js-sdk/sdk-reference.md) |
| Simnet-only code | [Simnet-only code](https://docs.stacks.co/clarinet/simnet-only-code.md) |
| Mainnet simulation | [Mainnet Execution Simulation](https://docs.stacks.co/clarinet/mainnet-execution-simulation.md) |

Scaffold test layout:

```
contracts/tests/*.test.ts    # Vitest + simnet (generated for add --template)
contracts/vitest.config.ts   # clarinet environment
```

Example pattern (from generated SIP templates):

```typescript
import { Cl } from '@stacks/transactions';

const accounts = simnet.getAccounts();
const deployer = accounts.get('deployer')!;

const { result } = simnet.callPublicFn(
  'my-token',
  'mint',
  [Cl.uint(100), Cl.standardPrincipal(deployer)],
  deployer,
);
```

## Validation & analysis

| Topic | Doc |
|-------|-----|
| check_checker | [Validation and Analysis](https://docs.stacks.co/clarinet/validation-and-analysis.md) |
| Formatter | [Clarity Formatter](https://docs.stacks.co/clarinet/clarity-formatter.md) |
| Contract interaction | [Contract Interaction](https://docs.stacks.co/clarinet/contract-interaction.md) |

Scaffold enables check_checker in `Clarinet.toml` by default.

## Deployment

| Topic | Doc |
|-------|-----|
| Deployment guide | [Contract Deployment](https://docs.stacks.co/clarinet/contract-deployment.md) |
| Clarinet CLI deploy commands | [CLI Reference](https://docs.stacks.co/reference/clarinet/cli-reference.md) |

Scaffold **`stacksdapp deploy`** generates plans, handles testnet broadcast, devnet epoch gating, and auto-versioning — prefer it over manual `clarinet deployments apply` in scaffold projects.

## Devnet & integrations

| Topic | Doc |
|-------|-----|
| Local devnet | [Local Blockchain Development](https://docs.stacks.co/clarinet/local-blockchain-development.md) |
| Stacks.js integration | [Stacks.js Integration](https://docs.stacks.co/clarinet/integrations/stacks.js.md) |
| sBTC integration | [sBTC Integration](https://docs.stacks.co/clarinet/integrations/sbtc.md) |
| VSCode extension | [Clarity VSCode Extension](https://docs.stacks.co/clarinet/integrations/clarity-vscode-extension.md) |
| FAQ | [Clarinet FAQ](https://docs.stacks.co/clarinet/faq.md) |

## Rendezvous (fuzz testing)

Optional advanced testing — not run by default in scaffold.

| Topic | Doc |
|-------|-----|
| Overview | [Rendezvous Overview](https://docs.stacks.co/rendezvous/overview.md) |
| Quickstart | [Rendezvous Quickstart](https://docs.stacks.co/rendezvous/quickstart.md) |
| CLI reference | [Rendezvous Reference](https://docs.stacks.co/reference/rendezvous/reference.md) |

Add to mature projects after `stacksdapp test` passes.

## Clarity language reference (official)

| Topic | Doc |
|-------|-----|
| All functions | [Functions](https://docs.stacks.co/reference/clarity/functions.md) |
| Types | [Types](https://docs.stacks.co/reference/clarity/types.md) |
| Keywords | [Keywords](https://docs.stacks.co/reference/clarity/keywords.md) |

Shorter cheat sheet: [clarity-language.md](clarity-language.md)

## Agent rules

- Clarinet **3.23+** for Clarity 6 devnet — see [troubleshooting.md](troubleshooting.md) if devnet stalls
- C5 contracts need epoch **3.4**, not 4.0 — [clarity-versions.md](clarity-versions.md)
- For doc gaps, fetch [llms.txt](https://docs.stacks.co/llms.txt) → Clarinet section
