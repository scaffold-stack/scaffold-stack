# Scaffold Stacks — AI agent guide

This project was scaffolded with [Scaffold Stacks](https://scaffoldstacks.mintlify.app/). Use the bundled agent skill for all Stacks dApp work.

## Skill location

**Cursor:** `.cursor/skills/scaffold-stacks/SKILL.md` (auto-discovered; no setup required)

**Other agents (Claude Code, Codex, etc.):** Read `.cursor/skills/scaffold-stacks/SKILL.md` first, then reference files in that directory as needed.

## Quick commands

Run from project root (or any subdirectory — CLI walks up to `stacksdapp.toml` or `contracts/Clarinet.toml`):

```bash
stacksdapp doctor          # verify Rust, Node, Clarinet, Docker
stacksdapp check           # type-check Clarity
stacksdapp generate        # regenerate TypeScript bindings
stacksdapp test            # contract + frontend tests
stacksdapp deploy --network testnet --yes
stacksdapp dev --network testnet
```

## Default workflow

1. Edit `contracts/contracts/*.clar`
2. `stacksdapp check && stacksdapp generate && stacksdapp test`
3. Deploy to **testnet** first (no Docker): `stacksdapp deploy --network testnet --yes`
4. `stacksdapp dev --network testnet`

## Documentation

- Guides: https://scaffoldstacks.mintlify.app/
- Docs index for agents: https://scaffoldstacks.mintlify.app/llms.txt
- Clarity language: `.cursor/skills/scaffold-stacks/clarity-language.md`
- SIP-010 / SIP-009: `.cursor/skills/scaffold-stacks/sip-standards.md`

## Frontend

Generated hooks live in `frontend/src/generated/hooks.ts`. See `.cursor/skills/scaffold-stacks/frontend.md` for wallet vs devnet signing, hook usage, and custom UI patterns.

## Never edit by hand

- `frontend/src/generated/*` — run `stacksdapp generate`
- Do not commit real mnemonics in `contracts/settings/Testnet.toml` or `Mainnet.toml`
