# Project layout

Scaffold Stacks monorepo structure after `stacksdapp new` or `stacksdapp init`.

## Root

```
.
├── stacksdapp.toml          # [project] name, [defaults] network = "devnet"
├── AGENTS.md                # Pointer for non-Cursor agents
├── package.json             # npm scripts wrapping stacksdapp commands
├── .gitignore
├── .githooks/
│   └── pre-commit           # Mnemonic guard for Testnet/Mainnet settings
└── .cursor/
    └── skills/
        └── scaffold-stacks/ # Agent skill (auto-installed)
```

## contracts/

```
contracts/
├── Clarinet.toml            # [contracts.*] path, clarity_version, epoch
├── contracts/
│   └── *.clar               # Clarity source — primary edit surface
├── settings/
│   ├── Devnet.toml          # Public burner mnemonics (devnet only)
│   ├── Testnet.toml         # User's testnet deployer
│   └── Mainnet.toml         # User's mainnet deployer
├── tests/
│   └── *.test.ts            # Vitest + clarinet-sdk initSimnet
├── deployments/             # Clarinet deployment plans (generated/updated on deploy)
├── package.json             # @stacks/clarinet-sdk, vitest
├── vitest.config.ts
├── tsconfig.json
├── .cache/                  # ABI export cache (gitignored)
└── .devnet/                 # Local devnet state (gitignored)
```

### Clarinet.toml

Each contract entry includes:

```toml
[contracts.counter]
path = "contracts/counter.clar"
clarity_version = 6
epoch = "4.0"
```

Project-level `clarity_version` in `[project]` may also appear; per-contract settings take precedence for deploy.

## frontend/

```
frontend/
├── src/
│   ├── app/                 # Next.js app router pages
│   ├── components/          # UI components (editable)
│   │   └── debug/           # Debug panel wrapper
│   ├── generated/           # AUTO-GENERATED — run stacksdapp generate
│   │   ├── contracts.ts
│   │   ├── hooks.ts
│   │   ├── DebugContracts.tsx
│   │   └── deployments.json # Written on deploy
│   ├── lib/devnet.ts        # devnet burner signing
│   ├── store/wallet.ts      # Jotai wallet state
│   └── scaffold.config.ts   # network / node URL config
├── scripts/
│   ├── export-abi.mjs       # Used by codegen pipeline
│   └── build-tx.mjs
├── .env.local               # NEXT_PUBLIC_NETWORK=devnet|testnet|mainnet
└── package.json
```

## Edit matrix

| Path | Edit? | Action on change |
|------|-------|------------------|
| `contracts/contracts/*.clar` | Yes | `check` → `generate` → `test` |
| `contracts/Clarinet.toml` | Yes | May need redeploy plan regen |
| `contracts/tests/*.test.ts` | Yes | `stacksdapp test` |
| `frontend/src/components/**` | Yes | Normal frontend dev |
| `frontend/src/generated/**` | **No** | `stacksdapp generate` |
| `contracts/settings/Testnet.toml` | Yes (local) | Never commit real mnemonics |
| `contracts/.cache/**` | No | Deleted by `stacksdapp clean` |

## Environment variables

### frontend/.env.local

```bash
NEXT_PUBLIC_NETWORK=devnet          # devnet | testnet | mainnet
# NEXT_PUBLIC_STACKS_NODE_URL=...    # optional override
# NEXT_PUBLIC_HIRO_API_KEY=...       # optional Hiro API key
```

`stacksdapp dev` and deploy update network-related env as needed.

Frontend hooks, wallet, and signing: [frontend.md](frontend.md)

### Agent / CI

```bash
STACKSDAPP_ROOT=/path/to/project    # Same as --root
SCAFFOLD_ALLOW_COMMITTED_MNEMONIC=1 # Emergency only — bypass pre-commit hook
```

## Root discovery

CLI walks up from CWD looking for:

1. `stacksdapp.toml`, or
2. `contracts/Clarinet.toml`

Standard Clarinet repos (root `Clarinet.toml`) are normalized on `init`/`upgrade` to nested layout.

## Git hooks

After clone, enable mnemonic guard:

```bash
git config core.hooksPath .githooks
# or
npm run setup-hooks
```

`stacksdapp doctor --strict` warns if hooks are not configured.
