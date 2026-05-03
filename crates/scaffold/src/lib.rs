mod cli_style;

use anyhow::{anyhow, Result};
use cli_style::{
    footer_repo_link, print_creating_line, print_new_project_banner, print_success_block,
    section_alternative, section_recommended, step_done_string,
};
use colored::Colorize;
use include_dir::{include_dir, Dir};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use which::which;

static FRONTEND_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/frontend-template");

const DEFAULT_CONTRACTS_PACKAGE_JSON: &str = r#"{
  "name": "contracts",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "vitest run"
  },
  "devDependencies": {
    "@stacks/clarinet-sdk": "^3",
    "@stacks/transactions": "7.4.0",
    "typescript": "^5",
    "vitest": "^1"
  }
}
"#;

const DEFAULT_VITEST_CONFIG: &str = r#"import { defineConfig } from 'vitest/config';
export default defineConfig({
  test: { environment: 'node' },
});
"#;

const DEFAULT_CONTRACTS_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true
  },
  "include": ["tests/**/*.ts"]
}
"#;

const DEFAULT_FRONTEND_ENV_LOCAL: &str = r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex
"#;

const DEFAULT_FRONTEND_ENV_LOCAL_EXAMPLE: &str = r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex

# Optional node URL override:
# NEXT_PUBLIC_STACKS_NODE_URL=https://api.testnet.hiro.so
"#;

const DEFAULT_DEVNET_SETTINGS: &str = r#"[network]
name = "devnet"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"
"#;

const DEFAULT_TESTNET_SETTINGS: &str = r#"[network]
name = "testnet"
stacks_node_rpc_address = "https://api.testnet.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE TESTNET MNEMONIC HERE>"
"#;

const DEFAULT_MAINNET_SETTINGS: &str = r#"[network]
name = "mainnet"
stacks_node_rpc_address = "https://api.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE MAINNET MNEMONIC HERE>"
"#;

/// Pre-commit hook: blocks staging Testnet/Mainnet settings that look like a real BIP39 phrase.
/// Override (emergency only): SCAFFOLD_ALLOW_COMMITTED_MNEMONIC=1 git commit ...
const GIT_HOOK_PRE_COMMIT: &str = r#"#!/bin/sh
# Installed by scaffold-stacks — block likely seed phrases in Testnet/Mainnet settings.
if [ -n "${SCAFFOLD_ALLOW_COMMITTED_MNEMONIC}" ]; then
  exit 0
fi

# Use a for-loop (not `| while read`) so `exit 1` aborts the hook reliably.
for f in $(git diff --cached --name-only --diff-filter=ACM); do
  case "$f" in
    */settings/Testnet.toml|*/settings/Mainnet.toml) ;;
    *) continue ;;
  esac

  content=$(git show ":$f" 2>/dev/null) || continue
  mnemonic=$(printf '%s\n' "$content" | sed -n 's/^[[:space:]]*mnemonic[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  [ -z "$mnemonic" ] && continue

  # Scaffold placeholders use angle brackets; real BIP39 phrases do not.
  mn_first=$(printf '%.1s' "$mnemonic")
  [ "$mn_first" = '<' ] && continue

  # Rough BIP39 heuristic: 12+ whitespace-separated tokens (typical seed length).
  words=$(printf '%s\n' "$mnemonic" | awk '{ print NF }')
  if [ "$words" -ge 12 ] 2>/dev/null; then
    echo "" >&2
    echo "================================================================" >&2
    echo "  scaffold-stacks git hook: possible seed phrase in commit" >&2
    echo "================================================================" >&2
    echo "  Staged file: $f" >&2
    echo "  [accounts.deployer].mnemonic has $words whitespace-separated tokens." >&2
    echo "  Committing real mnemonics to git is unsafe (history, forks, leaks)." >&2
    echo "" >&2
    echo "  Prefer env-based keys or a gitignored secrets file. To force this commit:" >&2
    echo "    SCAFFOLD_ALLOW_COMMITTED_MNEMONIC=1 git commit   # not recommended" >&2
    echo "================================================================" >&2
    echo "" >&2
    exit 1
  fi
done

exit 0
"#;

pub async fn new_project(name: &str, git_init: bool) -> Result<()> {
    print_new_project_banner();
    print_creating_line(name);

    ensure_prerequisites().await?;

    let root = Path::new(name);
    if root.exists() {
        return Err(anyhow!(
            "{} Directory '{}' already exists",
            "✗".red().bold(),
            name
        ));
    }

    let style = ProgressStyle::with_template(
        "  {spinner:.yellow} {wide_msg:.dim}  \x1b[2m[{elapsed}]\x1b[0m",
    )
    .unwrap()
    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

    let pb = ProgressBar::new_spinner();
    pb.set_style(style);
    pb.enable_steady_tick(Duration::from_millis(80));

    // ── Step 1: Scaffold files ────────────────────────────────────────────────
    pb.set_message("Scaffolding project structure...");

    tokio::fs::create_dir_all(root).await?;
    let frontend_dir = root.join("frontend");
    let contracts_root = root.join("contracts");
    tokio::fs::create_dir_all(&frontend_dir).await?;
    tokio::fs::create_dir_all(contracts_root.join("contracts")).await?;
    tokio::fs::create_dir_all(contracts_root.join("settings")).await?;
    tokio::fs::create_dir_all(contracts_root.join("tests")).await?;

    FRONTEND_TEMPLATE
        .extract(&frontend_dir)
        .map_err(|e| anyhow!("Failed to copy frontend template: {e}"))?;

    write_project_files(name, root, &frontend_dir, &contracts_root).await?;

    pb.println(step_done_string("Scaffolded", &format!("{name}/")));

    // ── Step 2: Install dependencies (parallel) ───────────────────────────────
    pb.set_message("Installing frontend dependencies...");

    let fe_dir = frontend_dir.clone();
    let ct_dir = contracts_root.clone();
    run_npm_install_with_feedback(&pb, &fe_dir, "frontend", "").await?;

    pb.set_message("Installing contract dependencies...");
    run_npm_install_with_feedback(&pb, &ct_dir, "contracts", "").await?;

    pb.println(step_done_string("Installed", "node_modules"));

    // ── Step 3: Git init ──────────────────────────────────────────────────────
    if git_init {
        pb.set_message("Initialising git repository...");

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?;

        try_set_git_hooks_path(root).await?;

        Command::new("git")
            .args(["add", "-A"])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?;

        Command::new("git")
            .args(["commit", "-m", "scaffold-stacks init"])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?;

        pb.println(step_done_string("Initialised", "git (main)"));
        pb.println(format!(
            "  {}  {}",
            "··".dimmed(),
            "pre-commit hook: blocks likely mnemonics in contracts/settings/Testnet|Mainnet.toml"
                .dimmed()
        ));
        pb.println(format!(
            "  {}  {}",
            "··".dimmed(),
            "after clone: npm run setup-hooks  or  git config core.hooksPath .githooks".dimmed()
        ));
    }

    pb.finish_and_clear();

    // ── Success output ────────────────────────────────────────────────────────
    print_success_block(name);
    section_recommended();
    println!(
        "    {}  {}",
        "1".cyan().bold(),
        format!("cd {}", name).dimmed()
    );
    println!(
        "    {}  {}",
        "2".cyan().bold(),
        format!(
            "{}  {}",
            "Get testnet STX".white(),
            "https://explorer.hiro.so/sandbox/faucet?chain=testnet".dimmed()
        )
    );
    println!(
        "    {}  {}",
        "3".cyan().bold(),
        format!(
            "{} {}",
            "Edit".white(),
            "contracts/settings/Testnet.toml".bold().white()
        )
    );
    println!(
        "          {}",
        "[accounts.deployer]".dimmed()
    );
    println!(
        "          {}",
        "mnemonic = \"…\"".dimmed()
    );
    println!(
        "    {}  {}",
        "4".cyan().bold(),
        "stacksdapp deploy --network testnet".bold().green()
    );
    println!(
        "    {}  {}",
        "5".cyan().bold(),
        "stacksdapp dev --network testnet".bold().green()
    );
    println!();
    section_alternative();
    println!(
        "    {}  {}  {}",
        "1".cyan().bold(),
        format!("cd {}", name).dimmed(),
        format!("{} {}", "·".dimmed(), "start Docker Desktop".dimmed())
    );
    println!(
        "    {}  {}  {}",
        "2".cyan().bold(),
        "stacksdapp dev".bold().green(),
        format!("{} {}", "←".dimmed(), "local chain + Next.js".dimmed())
    );
    println!(
        "    {}  {}  {}",
        "3".cyan().bold(),
        "stacksdapp deploy --network devnet".bold().green(),
        format!("{} {}", "←".dimmed(), "second terminal".dimmed())
    );
    println!();
    footer_repo_link();

    Ok(())
}

/// Standard Clarinet uses `Clarinet.toml` at the repo root next to `contracts/`,
/// `settings/`, and `tests/`. scaffold-stacks expects `contracts/Clarinet.toml`
/// with sources under `contracts/contracts/*.clar`. When only the standard
/// layout is present, move artifacts into the scaffold layout in-place.
async fn normalize_standard_clarinet_layout(root: &Path) -> Result<()> {
    let nested_clarinet = root.join("contracts").join("Clarinet.toml");
    if nested_clarinet.exists() {
        return Ok(());
    }

    let root_clarinet = root.join("Clarinet.toml");
    if !root_clarinet.exists() {
        return Ok(());
    }

    let clar_root = root.join("contracts");
    if !clar_root.is_dir() {
        return Err(anyhow!(
            "Found Clarinet.toml at the repo root but no contracts/ directory.\n\
             Create a Clarinet project first or run init from your Clarinet repo root."
        ));
    }

    let nested_sources = clar_root.join("contracts");
    tokio::fs::create_dir_all(&nested_sources).await?;

    let mut entries = tokio::fs::read_dir(&clar_root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let ft = entry.file_type().await?;
        if ft.is_file() && path.extension().is_some_and(|e| e == "clar") {
            let dest = nested_sources.join(entry.file_name());
            tokio::fs::rename(&path, &dest).await?;
        }
    }

    tokio::fs::rename(&root_clarinet, &nested_clarinet).await?;

    let root_settings = root.join("settings");
    let dest_settings = clar_root.join("settings");
    if root_settings.exists() && root_settings.is_dir() {
        if dest_settings.exists() {
            return Err(anyhow!(
                "[init] Both ./settings and ./contracts/settings exist.\n\
                 Remove or merge one directory, then rerun stacksdapp init."
            ));
        }
        tokio::fs::rename(&root_settings, &dest_settings).await?;
    }

    let root_tests = root.join("tests");
    let dest_tests = clar_root.join("tests");
    if root_tests.exists() && root_tests.is_dir() {
        if dest_tests.exists() {
            return Err(anyhow!(
                "[init] Both ./tests and ./contracts/tests exist.\n\
                 Remove or merge one directory, then rerun stacksdapp init."
            ));
        }
        tokio::fs::rename(&root_tests, &dest_tests).await?;
    }

    let root_deployments = root.join("deployments");
    let dest_deployments = clar_root.join("deployments");
    if root_deployments.exists() && root_deployments.is_dir() {
        if dest_deployments.exists() {
            return Err(anyhow!(
                "[init] Both ./deployments and ./contracts/deployments exist.\n\
                 Remove or merge one directory, then rerun stacksdapp init."
            ));
        }
        tokio::fs::rename(&root_deployments, &dest_deployments).await?;
    }

    for fname in ["package.json", "vitest.config.ts", "tsconfig.json"] {
        let src = root.join(fname);
        let dst = clar_root.join(fname);
        if src.exists() && !dst.exists() {
            tokio::fs::rename(&src, &dst).await?;
        }
    }

    println!(
        "[init] Detected standard Clarinet layout (Clarinet.toml at repo root).\n\
         Normalized to scaffold-stacks layout: contracts/Clarinet.toml and contracts/contracts/*.clar."
    );

    Ok(())
}

pub async fn init_project() -> Result<()> {
    ensure_prerequisites().await?;

    let root = Path::new(".");
    normalize_standard_clarinet_layout(root).await?;

    let contracts_root = root.join("contracts");
    let frontend_dir = root.join("frontend");
    let clarinet_toml = contracts_root.join("Clarinet.toml");

    if !clarinet_toml.exists() {
        return Err(anyhow!(
            "No Clarinet project detected.\n\
             Expected either:\n\
               • Clarinet.toml in the current directory (standard Clarinet), or\n\
               • contracts/Clarinet.toml (scaffold-stacks layout)."
        ));
    }

    tokio::fs::create_dir_all(contracts_root.join("contracts")).await?;
    tokio::fs::create_dir_all(contracts_root.join("settings")).await?;
    tokio::fs::create_dir_all(contracts_root.join("tests")).await?;

    if !frontend_dir.exists() {
        tokio::fs::create_dir_all(&frontend_dir).await?;
        FRONTEND_TEMPLATE
            .extract(&frontend_dir)
            .map_err(|e| anyhow!("Failed to copy frontend template: {e}"))?;
        println!("[init] Added frontend template in ./frontend");
    } else if !frontend_dir.join("scripts/export-abi.mjs").exists() {
        return Err(anyhow!(
            "frontend/ exists but is missing scripts/export-abi.mjs.\n\
             To avoid overwriting existing frontend files, init will not continue automatically.\n\
             Add that script or move/backup frontend/ and rerun `stacksdapp init`."
        ));
    } else {
        println!("[init] Existing frontend detected. Keeping files unchanged.");
    }

    ensure_contract_support_files(&contracts_root, &frontend_dir).await?;
    run_generate_after_setup().await?;

    write_git_hooks(Path::new(".")).await?;
    let _ = try_set_git_hooks_path(Path::new(".")).await;

    println!("[init] ✔ Existing Clarinet project initialized for scaffold-stacks.");
    Ok(())
}

pub async fn upgrade_project() -> Result<()> {
    ensure_prerequisites().await?;

    normalize_standard_clarinet_layout(Path::new(".")).await?;

    if !Path::new("contracts/Clarinet.toml").exists()
        || !Path::new("frontend/package.json").exists()
    {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from a project containing Clarinet.toml (repo root or contracts/) and frontend/package.json.\n\
             Run stacksdapp init first if you only have a Clarinet repo."
        ));
    }

    println!("[upgrade] Refreshing dependencies and regenerating bindings (non-destructive)...");
    run_npm_install(Path::new("frontend"), "frontend", "[upgrade]").await?;
    run_npm_install(Path::new("contracts"), "contracts", "[upgrade]").await?;
    stacksdapp_codegen::generate_all().await?;
    write_git_hooks(Path::new(".")).await?;
    let _ = try_set_git_hooks_path(Path::new(".")).await;
    println!("[upgrade] ✔ Upgrade complete.");
    Ok(())
}

async fn write_project_files(
    name: &str,
    root: &Path,
    frontend_dir: &Path,
    contracts_root: &Path,
) -> Result<()> {
    tokio::fs::write(
        contracts_root.join("package.json"),
        DEFAULT_CONTRACTS_PACKAGE_JSON,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("vitest.config.ts"),
        DEFAULT_VITEST_CONFIG,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("tsconfig.json"),
        DEFAULT_CONTRACTS_TSCONFIG,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("Clarinet.toml"),
        format!(
            r#"[project]
name = "{name}"
description = ""
authors = []
telemetry = false
cache_dir = "./.cache"
requirements = []

[contracts.counter]
path = "contracts/counter.clar"
clarity_version = 4
epoch = "latest"

[repl.costs_version]
version = 2
"#
        ),
    )
    .await?;

    tokio::fs::write(contracts_root.join("settings/Devnet.toml"), r#"[network]
name = "devnet"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_1]
mnemonic = "sell invite acquire kitten bamboo drastic jelly vivid peace spawn twice guilt pave pen trash pretty park cube fragile unaware remain midnight betray rebuild"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_2]
mnemonic = "hold excess usual excess ring elephant install account glad dry fragile donkey gaze humble truck breeze nation gasp vacuum limb head keep delay hospital"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_3]
mnemonic = "cycle puppy glare enroll cost improve round trend wrist mushroom scorpion tower claim oppose clever elephant dinosaur eight problem before frozen dune wagon high"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_4]
mnemonic = "board list obtain sugar hour worth raven scout denial thunder horse logic fury scorpion fold genuine phrase wealth news aim below celery when cabin"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_5]
mnemonic = "hurry aunt blame peanut heavy update captain human rice crime juice adult scale device promote vast project quiz unit note reform update climb purchase"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_6]
mnemonic = "area desk dutch sign gold cricket dawn toward giggle vibrant indoor bench warfare wagon number tiny universe sand talk dilemma pottery bone trap buddy"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_7]
mnemonic = "prevent gallery kind limb income control noise together echo rival record wedding sense uncover school version force bleak nuclear include danger skirt enact arrow"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.wallet_8]
mnemonic = "female adjust gallery certain visit token during great side clown fitness like hurt clip knife warm bench start reunion globe detail dream depend fortune"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[accounts.faucet]
mnemonic = "shadow private easily thought say logic fault paddle word top book during ignore notable orange flight clock image wealth health outside kitten belt reform"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[devnet]
disable_stacks_explorer = false
disable_stacks_api = false

[[devnet.pox_stacking_orders]]
start_at_cycle = 1
duration = 10
auto_extend = true
wallet = "wallet_1"
slots = 2
btc_address = "mr1iPkD9N3RJZZxXRk7xF9d36gffa6exNC"

[[devnet.pox_stacking_orders]]
start_at_cycle = 1
duration = 10
auto_extend = true
wallet = "wallet_2"
slots = 2
btc_address = "muYdXKmX9bByAueDe6KFfHd5Ff1gdN9ErG"

[[devnet.pox_stacking_orders]]
start_at_cycle = 1
duration = 10
auto_extend = true
wallet = "wallet_3"
slots = 2
btc_address = "mvZtbibDAAA3WLpY7zXXFqRa3T4XSknBX7"
"#).await?;

    tokio::fs::write(
        contracts_root.join("settings/Testnet.toml"),
        r#"[network]
name = "testnet"
stacks_node_rpc_address = "https://api.testnet.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE TESTNET MNEMONIC HERE>"
"#,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("settings/Mainnet.toml"),
        r#"[network]
name = "mainnet"
stacks_node_rpc_address = "https://api.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE MAINNET MNEMONIC HERE>"
"#,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("contracts/counter.clar"),
        r#";; counter.clar scaffolded by scaffold-stacks

(define-data-var counter uint u0)

(define-read-only (get-count)
  (ok (var-get counter)))

(define-public (increment)
  (begin
    (var-set counter (+ (var-get counter) u1))
    (ok (var-get counter))))

(define-public (decrement)
  (begin
    (asserts! (> (var-get counter) u0) (err u1))
    (var-set counter (- (var-get counter) u1))
    (ok (var-get counter))))

(define-public (reset)
  (begin
    (var-set counter u0)
    (ok u0)))
"#,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("tests/counter.test.ts"),
        r#"import { describe, expect, it } from 'vitest';
import { initSimnet } from '@stacks/clarinet-sdk';
import { Cl } from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const address1 = accounts.get('wallet_1')!;

describe('counter', () => {
  it('increments', () => {
    const { result } = simnet.callPublicFn('counter', 'increment', [], address1);
    expect(result.value.value).toBe(1n);
  });
  it('get-count returns current value', () => {
    const { result } = simnet.callReadOnlyFn('counter', 'get-count', [], address1);
    expect(result.value.value).toBe(1n);
  });
  it('decrement', () => {
    const { result } = simnet.callPublicFn('counter', 'decrement', [], address1);
    expect(result.value.value).toBe(0n);
  });
});
"#,
    )
    .await?;

    tokio::fs::write(root.join("package.json"), format!(
        "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{\n    \"dev\": \"stacksdapp dev\",\n    \"generate\": \"stacksdapp generate\",\n    \"deploy\": \"stacksdapp deploy\",\n    \"test\": \"stacksdapp test\",\n    \"check\": \"stacksdapp check\",\n    \"setup-hooks\": \"git config core.hooksPath .githooks\"\n  }}\n}}\n"
    )).await?;

    tokio::fs::write(
        root.join(".gitignore"),
        r#"# Rust
target/

# Node
node_modules/

# Environment — never commit real keys
.env
.env.local
.env.*.local

# Clarinet devnet / generated settings (keep Devnet.toml, Testnet.toml, Mainnet.toml tracked)
contracts/.cache/
contracts/.devnet/
contracts/settings/Simnet.toml
contracts/settings/Epoch*.toml

# Next.js build
frontend/.next/
frontend/out/

# OS
.DS_Store
*.pem
"#,
    )
    .await?;

    tokio::fs::write(
        frontend_dir.join(".gitignore"),
        r#"node_modules/
.env
.env.local
.env.*.local
.next/
out/
.DS_Store
*.tsbuildinfo
next-env.d.ts
"#,
    )
    .await?;

    tokio::fs::write(
        contracts_root.join(".gitignore"),
        r#"node_modules/
.cache/
.devnet/
settings/Simnet.toml
.env
.env.local
.env.*.local
.DS_Store
"#,
    )
    .await?;

    tokio::fs::write(frontend_dir.join(".env.local"), DEFAULT_FRONTEND_ENV_LOCAL).await?;

    tokio::fs::write(
        frontend_dir.join(".env.local.example"),
        DEFAULT_FRONTEND_ENV_LOCAL_EXAMPLE,
    )
    .await?;

    write_git_hooks(root).await?;

    Ok(())
}

async fn ensure_contract_support_files(contracts_root: &Path, frontend_dir: &Path) -> Result<()> {
    write_if_missing(
        &contracts_root.join("package.json"),
        DEFAULT_CONTRACTS_PACKAGE_JSON,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("vitest.config.ts"),
        DEFAULT_VITEST_CONFIG,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("tsconfig.json"),
        DEFAULT_CONTRACTS_TSCONFIG,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("settings/Devnet.toml"),
        DEFAULT_DEVNET_SETTINGS,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("settings/Testnet.toml"),
        DEFAULT_TESTNET_SETTINGS,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("settings/Mainnet.toml"),
        DEFAULT_MAINNET_SETTINGS,
    )
    .await?;
    write_if_missing(&frontend_dir.join(".env.local"), DEFAULT_FRONTEND_ENV_LOCAL).await?;
    write_if_missing(
        &frontend_dir.join(".env.local.example"),
        DEFAULT_FRONTEND_ENV_LOCAL_EXAMPLE,
    )
    .await?;
    Ok(())
}

async fn run_generate_after_setup() -> Result<()> {
    run_npm_install(Path::new("frontend"), "frontend", "[init]").await?;
    run_npm_install(Path::new("contracts"), "contracts", "[init]").await?;
    stacksdapp_codegen::generate_all().await?;
    Ok(())
}

async fn write_if_missing(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, contents).await?;
    Ok(())
}

async fn run_npm_install(dir: &Path, scope: &str, message_prefix: &str) -> Result<()> {
    if !dir.join("package.json").exists() {
        return Ok(());
    }

    let style = ProgressStyle::with_template(
        "  {spinner:.yellow} {wide_msg:.dim}  \x1b[2m[{elapsed}]\x1b[0m",
    )
    .unwrap()
    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

    let pb = ProgressBar::new_spinner();
    pb.set_style(style);
    pb.enable_steady_tick(Duration::from_millis(80));
    let head = npm_install_message_head(message_prefix);
    pb.set_message(format!("{head}Installing {scope} dependencies..."));

    run_npm_install_with_feedback(&pb, dir, scope, message_prefix).await?;

    pb.finish_and_clear();
    println!("  \x1b[32m✔\x1b[0m  {head}Finished installing {scope} dependencies.");
    Ok(())
}

fn npm_install_message_head(message_prefix: &str) -> String {
    if message_prefix.is_empty() {
        String::new()
    } else {
        format!("{message_prefix} ")
    }
}

pub async fn add_contract(name: &str, template: &str) -> Result<()> {
    let contracts_dir = Path::new("contracts/contracts");
    if !contracts_dir.exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
        ));
    }

    let path = contracts_dir.join(format!("{name}.clar"));
    if path.exists() {
        return Err(anyhow!("Contract '{}' already exists", name));
    }

    // ── Template Selection ───────────────────────────────────────────────────
    let (contract_source, test_source, contract_id) = match template {
        "sip010" => (
            format!(
                r#";; {name}.clar Fungible Token

(define-fungible-token {name})

;; Constants
(define-constant CONTRACT_OWNER tx-sender)
(define-constant ERR_OWNER_ONLY (err u100))
(define-constant ERR_NOT_TOKEN_OWNER (err u101))

(define-data-var token-uri (string-utf8 256) u"https://hiro.so")

;; SIP-010 Functions
(define-read-only (get-name) (ok "{name}"))
(define-read-only (get-symbol) (ok "{name}"))
(define-read-only (get-decimals) (ok u6))
(define-read-only (get-balance (who principal)) (ok (ft-get-balance {name} who)))
(define-read-only (get-total-supply) (ok (ft-get-supply {name})))
(define-read-only (get-token-uri) (ok (some (var-get token-uri))))

;; Public Functions
(define-public (set-token-uri (value (string-utf8 256)))
    (begin
        (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
        (var-set token-uri value)
        (ok (print {{
              notification: "token-metadata-update",
              payload: {{
                contract-id: current-contract,
                token-class: "ft"
              }}
            }}))))

(define-public (mint (amount uint) (recipient principal))
  (begin
    (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
    (ft-mint? {name} amount recipient)))

(define-public (transfer (amount uint) (sender principal) (recipient principal) (memo (optional (buff 34))))
  (begin
    (asserts! (or (is-eq tx-sender sender) (is-eq contract-caller sender)) ERR_NOT_TOKEN_OWNER)
    (try! (ft-transfer? {name} amount sender recipient))
    (match memo to-print (print to-print) 0x)
    (ok true)))
"#
            ),
            String::from(
                r#"import { describe, expect, it } from 'vitest';
import { initSimnet } from '@stacks/clarinet-sdk';
import { Cl } from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const deployer = accounts.get('deployer')!;
const wallet1 = accounts.get('wallet_1')!;

describe('token FT', () => {
  it('mints tokens', () => {
    const { result } = simnet.callPublicFn('token', 'mint', [Cl.uint(100), Cl.standardPrincipal(deployer)], deployer);
    expect(result.value.type).toBe('true');
  });
});
"#,
            ),
            Some("SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard"),
        ),

        "sip009" => (
            format!(
                r#";; {name}.clar Non-Fungible Token

(define-non-fungible-token {name} uint)

(define-data-var last-token-id uint u0)
(define-data-var base-uri (string-ascii 256) "https://api.example.com/metadata/{{id}}")

(define-constant CONTRACT_OWNER tx-sender)
(define-constant COLLECTION_LIMIT u1000)
(define-constant ERR_OWNER_ONLY (err u100))
(define-constant ERR_NOT_TOKEN_OWNER (err u101))
(define-constant ERR_SOLD_OUT (err u300))

(define-read-only (get-last-token-id) (ok (var-get last-token-id)))
(define-read-only (get-token-uri (token-id uint)) (ok (some (var-get base-uri))))
(define-read-only (get-owner (token-id uint)) (ok (nft-get-owner? {name} token-id)))

(define-public (set-base-uri (value (string-ascii 256)))
    (begin
        (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
        (var-set base-uri value)
        (ok (print {{
              notification: "token-metadata-update",
              payload: {{
                token-class: "nft",
                contract-id: current-contract,
              }}
            }}))))

(define-public (transfer (token-id uint) (sender principal) (recipient principal))
  (begin
    (asserts! (or (is-eq tx-sender sender) (is-eq contract-caller sender)) ERR_NOT_TOKEN_OWNER)
    (nft-transfer? {name} token-id sender recipient)))

(define-public (mint (recipient principal))
  (let ((token-id (+ (var-get last-token-id) u1)))
    (asserts! (< (var-get last-token-id) COLLECTION_LIMIT) ERR_SOLD_OUT)
    (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
    (try! (nft-mint? {name} token-id recipient))
    (var-set last-token-id token-id)
    (ok token-id)))
"#
            ),
            String::from(
                r#"import { describe, expect, it } from 'vitest';
import { initSimnet } from '@stacks/clarinet-sdk';
import { Cl } from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const deployer = accounts.get('deployer')!;

describe('nft NFT', () => {
  it('mints a token', () => {
    const { result } = simnet.callPublicFn('nft', 'mint', [Cl.standardPrincipal(deployer)], deployer);
    expect(result.value.value).toBe(1n);
  });
});
"#,
            ),
            Some("SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait"),
        ),

        _ => (
            format!(
                ";; {name}.clar\n\n(define-read-only (get-info)\n  (ok \"{name} contract\"))\n"
            ),
            format!(
                r#"import {{ describe, expect, it }} from 'vitest';
import {{ initSimnet }} from '@stacks/clarinet-sdk';
import {{ Cl }} from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const address1 = accounts.get('wallet_1')!;

describe('{name}', () => {{
  it('returns contract info', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-info', [], address1);
    expect(result).toBeOk(Cl.stringAscii('{name} contract'));
  }});
}});
"#
            ),
            None,
        ),
    };

    // 1. Write clarity contract and test files
    tokio::fs::write(&path, contract_source).await?;
    let test_path = Path::new("contracts/tests").join(format!("{name}.test.ts"));
    if !test_path.exists() {
        tokio::fs::write(&test_path, test_source).await?;
    }

    // 2. Update Clarinet.toml
    let clarinet_toml_path = Path::new("contracts/Clarinet.toml");
    let mut existing = tokio::fs::read_to_string(clarinet_toml_path).await?;

    existing = existing.replace("requirements = []", "");

    // Add remote requirement if specified
    if let Some(req_id) = contract_id {
        let req_block = format!("\n[[project.requirements]]\ncontract_id = \"{}\"\n", req_id);
        if !existing.contains(&format!("contract_id = \"{}\"", req_id)) {
            existing.push_str(&req_block);
        }
    }

    // Add the new contract definition
    existing.push_str(&format!(
        "\n[contracts.{name}]\npath = \"contracts/{name}.clar\"\nclarity_version = 4\nepoch = \"latest\"\n"
    ));

    tokio::fs::write(clarinet_toml_path, existing).await?;

    // Regenerate Bindings
    stacksdapp_codegen::generate_all().await?;

    println!("  \x1b[32m✔\x1b[0m  \x1b[1mAdded\x1b[0m  contracts/contracts/{name}.clar");
    Ok(())
}

async fn ensure_prerequisites() -> Result<()> {
    if which("node").is_err() {
        return Err(anyhow!(
            "\x1b[31m✗\x1b[0m Node.js >=20 is required. Install from https://nodejs.org"
        ));
    }
    if which("clarinet").is_err() {
        return Err(anyhow!(
            "\x1b[31m✗\x1b[0m clarinet is required.\n  Install (mac): brew install clarinet  OR for linux go to https://docs.stacks.co/get-started/developer-quickstart#source for guide"
        ));
    }
    Ok(())
}

async fn run_npm_install_with_feedback(
    pb: &ProgressBar,
    dir: &Path,
    scope: &str,
    message_prefix: &str,
) -> Result<()> {
    let mut child = Command::new("npm")
        .args([
            "install",
            "--no-audit",
            "--no-fund",
            "--prefer-offline",
            "--progress=false",
            "--loglevel=verbose",
        ])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture npm install logs for {scope}"))?;
    let mut lines = BufReader::new(stderr).lines();

    let head = npm_install_message_head(message_prefix);
    while let Some(line) = lines.next_line().await? {
        if let Some(dep) = parse_npm_dep_hint(&line) {
            pb.set_message(format!("{head}Installing {scope} dependencies... {dep}"));
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!("npm install failed in {scope}/"));
    }
    Ok(())
}

fn parse_npm_dep_hint(line: &str) -> Option<String> {
    // Example npm verbose line:
    // npm http fetch GET 200 https://registry.npmjs.org/react 123ms (cache hit)
    if let Some(url_start) = line.find("https://registry.npmjs.org/") {
        let url = &line[url_start + "https://registry.npmjs.org/".len()..];
        let pkg = url
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        if !pkg.is_empty() {
            return Some(pkg.to_string());
        }
    }

    // Fallback for lines mentioning node_modules package paths.
    if let Some(mod_start) = line.find("node_modules/") {
        let after = &line[mod_start + "node_modules/".len()..];
        let pkg = after
            .split([' ', '\t', '\n', '\r', '/', '\\'])
            .next()
            .unwrap_or("");
        if !pkg.is_empty() {
            return Some(pkg.to_string());
        }
    }

    None
}

/// Writes `.githooks/pre-commit` (mnemonic guard for Testnet/Mainnet settings).
async fn write_git_hooks(root: &Path) -> Result<()> {
    let hooks_dir = root.join(".githooks");
    tokio::fs::create_dir_all(&hooks_dir).await?;
    let hook_path = hooks_dir.join("pre-commit");
    tokio::fs::write(&hook_path, GIT_HOOK_PRE_COMMIT).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&hook_path).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&hook_path, perms).await?;
    }
    Ok(())
}

/// Points this repo at `.githooks` so the pre-commit hook runs (no-op if not a git work tree).
async fn try_set_git_hooks_path(root: &Path) -> Result<()> {
    if !root.join(".git").exists() {
        return Ok(());
    }
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(root)
        .status()
        .await?;
    if !status.success() {
        eprintln!(
            "[scaffold-stacks] Could not set core.hooksPath; run: git config core.hooksPath .githooks"
        );
    }
    Ok(())
}
