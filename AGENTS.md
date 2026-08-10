# Scaffold Stacks — AI agent guide (CLI repo)

This repository builds the `stacksdapp` CLI. When helping users **build dApps**, read the bundled skill template — the same files installed into every scaffolded project.

## Skill source (edit here)

```
crates/scaffold/agent-skill-template/
├── AGENTS.md
└── .cursor/skills/scaffold-stacks/
    ├── SKILL.md
    ├── frontend.md
    ├── clarity-language.md
    ├── sip-standards.md
    ├── cli-reference.md
    ├── project-layout.md
    ├── clarity-versions.md
    ├── workflows.md
    └── troubleshooting.md
```

Changes here are copied into projects by `stacksdapp new`, `stacksdapp init`, and `stacksdapp upgrade`.

After editing, sync the CLI repo Cursor copy:

```bash
cp crates/scaffold/agent-skill-template/.cursor/skills/scaffold-stacks/*.md .cursor/skills/scaffold-stacks/
```

## Working on this repo

```bash
cargo build -p stacksdapp
cargo test --all
bash scripts/ci-smoke.sh
```

## Working on a scaffolded dApp

Read `.cursor/skills/scaffold-stacks/SKILL.md` (or the template above). For React/hooks/wallet work, see `frontend.md`.

Docs: https://scaffoldstacks.mintlify.app/ · Index: https://scaffoldstacks.mintlify.app/llms.txt
