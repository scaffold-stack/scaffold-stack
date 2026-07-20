mod cli_style;

use anyhow::{anyhow, Result};
use cli_style::{
    note_line, print_creating_line, print_new_project_banner, print_next_steps,
    print_success_block, step_done_string,
};
use colored::Colorize;
use include_dir::{include_dir, Dir};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use which::which;

static FRONTEND_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/frontend-template");

const CONTRACTS_PACKAGE_LOCK: &str = include_str!("../contracts-template/package-lock.json");

const DEFAULT_CONTRACTS_PACKAGE_JSON: &str = r#"{
  "name": "contracts",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "vitest run",
    "test:report": "vitest run -- --coverage --costs"
  },
  "devDependencies": {
    "@stacks/clarinet-sdk": "3.21.0",
    "@stacks/transactions": "7.4.0",
    "@types/node": "^24",
    "typescript": "^5",
    "vitest": "^4.1.8",
    "vitest-environment-clarinet": "^3.0.0"
  }
}
"#;

const DEFAULT_VITEST_CONFIG: &str = r#"import { defineConfig } from "vitest/config";
import {
  vitestSetupFilePath,
  getClarinetVitestsArgv,
} from "@stacks/clarinet-sdk/vitest";

export default defineConfig({
  test: {
    environment: "clarinet",
    pool: "forks",
    isolate: false,
    maxWorkers: 1,
    setupFiles: [vitestSetupFilePath],
    environmentOptions: {
      clarinet: {
        ...getClarinetVitestsArgv(),
      },
    },
  },
});
"#;

const DEFAULT_CONTRACTS_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ESNext"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "strict": true,
    "noImplicitAny": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": [
    "node_modules/@stacks/clarinet-sdk/vitest-helpers/src",
    "tests"
  ]
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

const DEVNET_MNEMONIC_WARNING: &str =
    "# WARNING: These are public test mnemonics. NEVER use them on testnet or mainnet.\n\n";

const DEFAULT_DEVNET_SETTINGS_BODY: &str = r#"[network]
name = "devnet"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw"
balance = 100_000_000_000_000
sbtc_balance = 1_000_000_000
derivation = "m/44'/5757'/0'/0/0"

[devnet]
# Clarinet 3.2+ snapshot fast-boot: keep this section free of [[devnet.pox_stacking_orders]]
# and avoid custom images / early-epoch overrides (those force slow genesis mining).
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = false
# 15s keeps Nakamoto + signer able to keep up; 1s races burn height and stalls tips.
bitcoin_controller_block_time = 15_000
"#;

const DEFAULT_FULL_DEVNET_SETTINGS_BODY: &str = r#"[network]
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
# Clarinet 3.2+ snapshot fast-boot: no PoX stacking orders (those force slow genesis).
# Explorers off by default — enable locally if you need http://localhost:8000 / :8001.
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = false
# 15s keeps Nakamoto + signer able to keep up; 1s races burn height and stalls tips.
bitcoin_controller_block_time = 15_000
"#;

fn devnet_settings_with_warning(body: &str) -> String {
    format!("{DEVNET_MNEMONIC_WARNING}{body}")
}

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
    validate_project_name(name)?;

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

    let result: Result<()> = async {
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

            ensure_success(
                Command::new("git")
                    .args(["init", "-b", "main"])
                    .current_dir(root)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await?,
                "git init",
            )?;

            try_set_git_hooks_path(root).await?;

            ensure_success(
                Command::new("git")
                    .args(["add", "-A"])
                    .current_dir(root)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await?,
                "git add -A",
            )?;

            ensure_success(
                Command::new("git")
                    .args(["commit", "-m", "scaffold-stacks init"])
                    .current_dir(root)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await?,
                "git commit",
            )?;

            pb.println(step_done_string("Initialized", "git (main)"));
            pb.println(note_line(
                "pre-commit hook: blocks likely mnemonics in contracts/settings/Testnet/Mainnet.toml",
            ));
            pb.println(note_line(
                "after clone: npm run setup-hooks or git config core.hooksPath .githooks",
            ));
        }

        pb.finish_and_clear();

        // ── Success output ────────────────────────────────────────────────────────
        print_success_block(name);
        print_next_steps(name);

        Ok(())
    }
    .await;

    if let Err(err) = result {
        if root.exists() {
            let _ = tokio::fs::remove_dir_all(root).await;
        }
        return Err(err);
    }

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

    let mut rollback_moves: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    let nested_sources = clar_root.join("contracts");
    tokio::fs::create_dir_all(&nested_sources).await?;

    let result: Result<()> = async {
        let mut entries = tokio::fs::read_dir(&clar_root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let ft = entry.file_type().await?;
            if ft.is_file() && path.extension().is_some_and(|e| e == "clar") {
                let dest = nested_sources.join(entry.file_name());
                tokio::fs::rename(&path, &dest).await?;
                rollback_moves.push((dest.clone(), path.clone()));
            }
        }

        tokio::fs::rename(&root_clarinet, &nested_clarinet).await?;
        rollback_moves.push((nested_clarinet.clone(), root_clarinet.clone()));

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
            rollback_moves.push((dest_settings.clone(), root_settings.clone()));
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
            rollback_moves.push((dest_tests.clone(), root_tests.clone()));
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
            rollback_moves.push((dest_deployments.clone(), root_deployments.clone()));
        }

        for fname in ["package.json", "vitest.config.ts", "tsconfig.json"] {
            let src = root.join(fname);
            let dst = clar_root.join(fname);
            if src.exists() && !dst.exists() {
                tokio::fs::rename(&src, &dst).await?;
                rollback_moves.push((dst.clone(), src.clone()));
            }
        }

        Ok(())
    }
    .await;

    if let Err(err) = result {
        for (from, to) in rollback_moves.iter().rev() {
            if from.exists() {
                let _ = tokio::fs::rename(from, to).await;
            }
        }
        return Err(err);
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

    let mut rollback = InitRollback {
        frontend_dir: frontend_dir.clone(),
        ..InitRollback::default()
    };

    let result: Result<()> = async {
        tokio::fs::create_dir_all(contracts_root.join("contracts")).await?;
        tokio::fs::create_dir_all(contracts_root.join("settings")).await?;
        tokio::fs::create_dir_all(contracts_root.join("tests")).await?;

        if !frontend_dir.exists() {
            tokio::fs::create_dir_all(&frontend_dir).await?;
            FRONTEND_TEMPLATE
                .extract(&frontend_dir)
                .map_err(|e| anyhow!("Failed to copy frontend template: {e}"))?;
            rollback.frontend_created = true;
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

        ensure_contract_support_files(
            &contracts_root,
            &frontend_dir,
            Some(&mut rollback.created_files),
        )
        .await?;

        if ensure_stacksdapp_toml(root).await? {
            rollback
                .created_files
                .push(root.join(stacksdapp_shell::CONFIG_FILE));
        }

        write_git_hooks_tracked(root, &mut rollback).await?;
        rollback.generated_touched = true;
        run_generate_after_setup().await?;

        let _ = try_set_git_hooks_path(root).await;

        println!("[init] ✔ Existing Clarinet project initialized for scaffold-stacks.");
        Ok(())
    }
    .await;

    if let Err(err) = result {
        rollback.apply().await;
        return Err(err);
    }

    Ok(())
}

#[derive(Default)]
struct InitRollback {
    frontend_created: bool,
    frontend_dir: PathBuf,
    created_files: Vec<PathBuf>,
    generated_touched: bool,
    pre_commit_backup: Option<(PathBuf, Option<Vec<u8>>)>,
}

impl InitRollback {
    async fn apply(self) {
        if self.generated_touched && !self.frontend_created {
            let _ = tokio::fs::remove_dir_all(self.frontend_dir.join("src/generated")).await;
        }
        for path in self.created_files.into_iter().rev() {
            let _ = tokio::fs::remove_file(&path).await;
        }
        if let Some((path, backup)) = self.pre_commit_backup {
            match backup {
                Some(bytes) => {
                    let _ = tokio::fs::write(&path, bytes).await;
                }
                None => {
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }
        if self.frontend_created {
            let _ = tokio::fs::remove_dir_all(&self.frontend_dir).await;
        }
    }
}

async fn write_git_hooks_tracked(root: &Path, rollback: &mut InitRollback) -> Result<()> {
    let hook_path = root.join(".githooks/pre-commit");
    let backup = if hook_path.exists() {
        Some(tokio::fs::read(&hook_path).await?)
    } else {
        None
    };
    rollback.pre_commit_backup = Some((hook_path, backup));
    write_git_hooks(root).await
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
    let mut created_config = None;

    let result: Result<()> = async {
        if ensure_stacksdapp_toml(Path::new(".")).await? {
            created_config = Some(PathBuf::from(stacksdapp_shell::CONFIG_FILE));
        }
        ensure_contract_support_files(Path::new("contracts"), Path::new("frontend"), None).await?;
        run_npm_install(Path::new("frontend"), "frontend", "[upgrade]").await?;
        run_npm_install(Path::new("contracts"), "contracts", "[upgrade]").await?;
        stacksdapp_codegen::generate_all().await?;
        write_git_hooks(Path::new(".")).await?;
        let _ = try_set_git_hooks_path(Path::new(".")).await;
        println!("[upgrade] ✔ Upgrade complete.");
        Ok(())
    }
    .await;

    if let Err(err) = result {
        if let Some(path) = created_config {
            let _ = tokio::fs::remove_file(path).await;
        }
        return Err(err);
    }

    Ok(())
}

async fn ensure_stacksdapp_toml(root: &Path) -> Result<bool> {
    let path = root.join(stacksdapp_shell::CONFIG_FILE);
    if path.exists() {
        return Ok(false);
    }
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && *s != ".")
        .unwrap_or("stacksdapp-project");
    tokio::fs::write(&path, stacksdapp_shell::default_config_toml(name)).await?;
    println!(
        "[scaffold] Wrote {} (project root marker for subdirectory commands)",
        stacksdapp_shell::CONFIG_FILE
    );
    Ok(true)
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
        contracts_root.join("package-lock.json"),
        CONTRACTS_PACKAGE_LOCK,
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

[repl.analysis]
passes = ["check_checker"]
check_checker = {{ trusted_sender = false, trusted_caller = false, callee_filter = false }}

[contracts.counter]
path = "contracts/counter.clar"
clarity_version = 5
epoch = "latest"
"#
        ),
    )
    .await?;

    tokio::fs::write(
        contracts_root.join("settings/Devnet.toml"),
        devnet_settings_with_warning(DEFAULT_FULL_DEVNET_SETTINGS_BODY),
    )
    .await?;

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
        r#"import { describe, expect, it } from "vitest";
import { Cl } from "@stacks/transactions";

const accounts = simnet.getAccounts();
const address1 = accounts.get("wallet_1")!;

describe("counter", () => {
  it("increments", () => {
    const { result } = simnet.callPublicFn("counter", "increment", [], address1);
    expect(result).toBeOk(Cl.uint(1));
  });
  it("get-count returns current value", () => {
    simnet.callPublicFn("counter", "increment", [], address1);
    const { result } = simnet.callReadOnlyFn("counter", "get-count", [], address1);
    expect(result).toBeOk(Cl.uint(1));
  });
  it("decrement", () => {
    simnet.callPublicFn("counter", "increment", [], address1);
    const { result } = simnet.callPublicFn("counter", "decrement", [], address1);
    expect(result).toBeOk(Cl.uint(0));
  });
});
"#,
    )
    .await?;

    tokio::fs::write(
        root.join("package.json"),
        format!(
        "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{\n    \"dev\": \"stacksdapp dev\",\n    \"generate\": \"stacksdapp generate\",\n    \"deploy\": \"stacksdapp deploy\",\n    \"test\": \"stacksdapp test\",\n    \"check\": \"stacksdapp check\",\n    \"setup-hooks\": \"git config core.hooksPath .githooks\"\n  }}\n}}\n"
    ),
    )
    .await?;

    tokio::fs::write(
        root.join(stacksdapp_shell::CONFIG_FILE),
        stacksdapp_shell::default_config_toml(name),
    )
    .await?;

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

async fn ensure_contract_support_files(
    contracts_root: &Path,
    frontend_dir: &Path,
    mut created: Option<&mut Vec<PathBuf>>,
) -> Result<()> {
    write_if_missing(
        &contracts_root.join("package.json"),
        DEFAULT_CONTRACTS_PACKAGE_JSON,
        &mut created,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("package-lock.json"),
        CONTRACTS_PACKAGE_LOCK,
        &mut created,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("vitest.config.ts"),
        DEFAULT_VITEST_CONFIG,
        &mut created,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("tsconfig.json"),
        DEFAULT_CONTRACTS_TSCONFIG,
        &mut created,
    )
    .await?;
    let devnet_settings = devnet_settings_with_warning(DEFAULT_DEVNET_SETTINGS_BODY);
    write_if_missing(
        &contracts_root.join("settings/Devnet.toml"),
        &devnet_settings,
        &mut created,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("settings/Testnet.toml"),
        DEFAULT_TESTNET_SETTINGS,
        &mut created,
    )
    .await?;
    write_if_missing(
        &contracts_root.join("settings/Mainnet.toml"),
        DEFAULT_MAINNET_SETTINGS,
        &mut created,
    )
    .await?;
    write_if_missing(
        &frontend_dir.join(".env.local"),
        DEFAULT_FRONTEND_ENV_LOCAL,
        &mut created,
    )
    .await?;
    write_if_missing(
        &frontend_dir.join(".env.local.example"),
        DEFAULT_FRONTEND_ENV_LOCAL_EXAMPLE,
        &mut created,
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

async fn write_if_missing(
    path: &Path,
    contents: &str,
    created: &mut Option<&mut Vec<PathBuf>>,
) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, contents).await?;
    if let Some(list) = created.as_deref_mut() {
        list.push(path.to_path_buf());
    }
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

fn ensure_success(status: std::process::ExitStatus, command: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{command} failed with status {status}"))
    }
}

pub async fn add_contract(name: &str, template: &str) -> Result<()> {
    validate_contract_name(name)?;
    validate_contract_template(template)?;

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

    stacksdapp_shell::print_banner("Adding Contract ✨");
    stacksdapp_shell::kv("Contract", name);
    if template != "blank" {
        stacksdapp_shell::kv("Template", template);
    }
    println!();

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
            format!(
                r#"import {{ describe, expect, it }} from "vitest";
import {{ Cl }} from "@stacks/transactions";

const accounts = simnet.getAccounts();
const deployer = accounts.get("deployer")!;

describe("{name} FT", () => {{
  it("mints tokens", () => {{
    const {{ result }} = simnet.callPublicFn(
      "{name}",
      "mint",
      [Cl.uint(100), Cl.standardPrincipal(deployer)],
      deployer
    );
    expect(result).toBeOk(Cl.bool(true));
  }});
}});
"#
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
            format!(
                r#"import {{ describe, expect, it }} from "vitest";
import {{ Cl }} from "@stacks/transactions";

const accounts = simnet.getAccounts();
const deployer = accounts.get("deployer")!;

describe("{name} NFT", () => {{
  it("mints a token", () => {{
    const {{ result }} = simnet.callPublicFn(
      "{name}",
      "mint",
      [Cl.standardPrincipal(deployer)],
      deployer
    );
    expect(result).toBeOk(Cl.uint(1));
  }});
}});
"#
            ),
            Some("SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait"),
        ),

        "blank" => (
            format!(
                ";; {name}.clar\n\n(define-read-only (get-info)\n  (ok \"{name} contract\"))\n"
            ),
            format!(
                r#"import {{ describe, expect, it }} from "vitest";
import {{ Cl }} from "@stacks/transactions";

const accounts = simnet.getAccounts();
const address1 = accounts.get("wallet_1")!;

describe("{name}", () => {{
  it("returns contract info", () => {{
    const {{ result }} = simnet.callReadOnlyFn("{name}", "get-info", [], address1);
    expect(result).toBeOk(Cl.stringAscii("{name} contract"));
  }});
}});
"#
            ),
            None,
        ),
        _ => unreachable!("template validated above"),
    };

    let step = stacksdapp_shell::begin_step("Created contract");
    let clarinet_toml_path = Path::new("contracts/Clarinet.toml");
    let clarinet_backup = tokio::fs::read_to_string(clarinet_toml_path).await.ok();
    let test_path = Path::new("contracts/tests").join(format!("{name}.test.ts"));
    let test_existed = test_path.exists();

    if let Err(e) = tokio::fs::write(&path, &contract_source).await {
        step.fail();
        return Err(e.into());
    }
    if !test_existed {
        if let Err(e) = tokio::fs::write(&test_path, &test_source).await {
            let _ = tokio::fs::remove_file(&path).await;
            step.fail();
            return Err(e.into());
        }
    }
    step.finish();

    let step = stacksdapp_shell::begin_step("Updated project configuration");
    let mut existing = match tokio::fs::read_to_string(clarinet_toml_path).await {
        Ok(s) => s,
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            if !test_existed {
                let _ = tokio::fs::remove_file(&test_path).await;
            }
            step.fail();
            return Err(e.into());
        }
    };

    existing = existing.replace("requirements = []", "");

    if let Some(req_id) = contract_id {
        let req_block = format!("\n[[project.requirements]]\ncontract_id = \"{}\"\n", req_id);
        if !existing.contains(&format!("contract_id = \"{}\"", req_id)) {
            existing.push_str(&req_block);
        }
    }

    existing.push_str(&format!(
        "\n[contracts.{name}]\npath = \"contracts/{name}.clar\"\nclarity_version = 5\nepoch = \"latest\"\n"
    ));

    if let Err(e) = tokio::fs::write(clarinet_toml_path, existing).await {
        let _ = tokio::fs::remove_file(&path).await;
        if !test_existed {
            let _ = tokio::fs::remove_file(&test_path).await;
        }
        step.fail();
        return Err(e.into());
    }
    step.finish();

    let step = stacksdapp_shell::begin_step("Generated TypeScript bindings");
    if let Err(e) = stacksdapp_codegen::generate_all_quiet().await {
        let _ = tokio::fs::remove_file(&path).await;
        if !test_existed {
            let _ = tokio::fs::remove_file(&test_path).await;
        }
        if let Some(backup) = clarinet_backup {
            let _ = tokio::fs::write(clarinet_toml_path, backup).await;
        }
        step.fail();
        return Err(e);
    }
    step.finish();

    if !stacksdapp_shell::is_quiet() {
        println!();
        stacksdapp_shell::rule();
        println!();
        println!("{}", "Created".bold().white());
        println!();
        println!(
            "{}",
            format!("contracts/contracts/{name}.clar").truecolor(52, 211, 153)
        );
        println!();
        println!("{}", "Generated".bold().white());
        println!();
        for f in ["contracts.ts", "hooks.ts", "DebugContracts.tsx"] {
            println!(
                "{} {}",
                "✓".truecolor(52, 211, 153),
                f.truecolor(156, 163, 175)
            );
        }
        println!();
        stacksdapp_shell::rule();
        println!();
        println!("{}", "Next".bold().white());
        println!();
        println!(
            "{}",
            "stacksdapp deploy --network testnet"
                .truecolor(52, 211, 153)
                .bold()
        );
        println!();
        println!("{}", "Done.".truecolor(156, 163, 175));
        println!();
    }

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
    let use_ci = dir.join("package-lock.json").exists();
    let subcommand = if use_ci { "ci" } else { "install" };
    let mut child = Command::new("npm")
        .arg(subcommand)
        .args([
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

/// Project directory / package name: single relative segment, no path traversal.
fn validate_project_name(name: &str) -> Result<()> {
    validate_safe_path_segment(name, "Project name")?;
    if name.len() > 64 {
        return Err(anyhow!(
            "Project name must be at most 64 characters (got {})",
            name.len()
        ));
    }
    if !is_valid_identifier_name(name) {
        return Err(anyhow!(
            "Invalid project name '{name}'. Use a letter followed by letters, digits, hyphens, or underscores (e.g. my-dapp)."
        ));
    }
    Ok(())
}

/// Clarinet / Clarity contract name: safe file stem + valid on-chain contract id charset.
fn validate_contract_name(name: &str) -> Result<()> {
    validate_safe_path_segment(name, "Contract name")?;
    // Stacks contract names are limited to 40 characters on-chain.
    if name.len() > 40 {
        return Err(anyhow!(
            "Contract name must be at most 40 characters (got {})",
            name.len()
        ));
    }
    if !is_valid_identifier_name(name) {
        return Err(anyhow!(
            "Invalid contract name '{name}'. Use a letter followed by letters, digits, hyphens, or underscores (e.g. my-token)."
        ));
    }
    Ok(())
}

fn validate_contract_template(template: &str) -> Result<()> {
    match template {
        "blank" | "sip010" | "sip009" => Ok(()),
        other => Err(anyhow!(
            "Invalid contract template '{other}'. Expected one of: blank | sip010 | sip009"
        )),
    }
}

fn validate_safe_path_segment(name: &str, label: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("{label} cannot be empty"));
    }
    if name != name.trim() {
        return Err(anyhow!(
            "{label} cannot have leading or trailing whitespace"
        ));
    }
    if name.contains(['/', '\\', '\0']) {
        return Err(anyhow!(
            "{label} cannot contain path separators or null bytes (got '{name}')"
        ));
    }
    if name == "." || name == ".." {
        return Err(anyhow!("{label} cannot be '.' or '..'"));
    }

    let path = Path::new(name);
    if path.is_absolute() {
        return Err(anyhow!("{label} cannot be an absolute path (got '{name}')"));
    }
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(os)), None) => {
            let segment = os
                .to_str()
                .ok_or_else(|| anyhow!("{label} contains invalid UTF-8"))?;
            if segment != name {
                return Err(anyhow!(
                    "{label} must be a single directory name without path components (got '{name}')"
                ));
            }
        }
        _ => {
            return Err(anyhow!(
                "{label} must be a single directory name without path components (got '{name}')"
            ));
        }
    }
    Ok(())
}

fn is_valid_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_accepts_simple_names() {
        assert!(validate_project_name("my-dapp").is_ok());
        assert!(validate_project_name("MyApp_1").is_ok());
    }

    #[test]
    fn project_name_rejects_traversal_and_paths() {
        assert!(validate_project_name("../evil").is_err());
        assert!(validate_project_name("foo/bar").is_err());
        assert!(validate_project_name("foo\\bar").is_err());
        assert!(validate_project_name("..").is_err());
        assert!(validate_project_name("/tmp/x").is_err());
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name(" bad").is_err());
    }

    #[test]
    fn project_name_rejects_invalid_charset() {
        assert!(validate_project_name("1dapp").is_err());
        assert!(validate_project_name("my dapp").is_err());
        assert!(validate_project_name("my.dapp").is_err());
    }

    #[test]
    fn contract_name_accepts_clarity_ids() {
        assert!(validate_contract_name("counter").is_ok());
        assert!(validate_contract_name("sip010-token").is_ok());
    }

    #[test]
    fn contract_name_rejects_traversal_and_overlong() {
        assert!(validate_contract_name("../x").is_err());
        assert!(validate_contract_name("a/b").is_err());
        assert!(validate_contract_name(&"a".repeat(41)).is_err());
        assert!(validate_contract_name("9bad").is_err());
    }

    #[test]
    fn contract_template_rejects_unknown_values() {
        assert!(validate_contract_template("blank").is_ok());
        assert!(validate_contract_template("sip010").is_ok());
        assert!(validate_contract_template("sip009").is_ok());
        assert!(validate_contract_template("sip10").is_err());
    }

    #[test]
    fn edge_case_names_reject_unicode_and_control_chars() {
        assert!(validate_project_name("café").is_err());
        assert!(validate_contract_name("tok\u{0000}en").is_err());
        assert!(validate_project_name("my\u{200b}dapp").is_err());
    }

    #[test]
    fn edge_case_names_reject_boundary_lengths() {
        assert!(validate_project_name(&"a".repeat(64)).is_ok());
        assert!(validate_project_name(&"a".repeat(65)).is_err());
        assert!(validate_contract_name(&"a".repeat(40)).is_ok());
        assert!(validate_contract_name(&"a".repeat(41)).is_err());
    }

    mod fuzz_validation {
        use super::{validate_contract_name, validate_project_name, validate_safe_path_segment};
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn fuzz_safe_path_segment_rejects_path_separators(s in r"[^/\\]*[/\\][^/\\]*") {
                prop_assume!(!s.is_empty());
                prop_assert!(validate_safe_path_segment(&s, "fuzz").is_err());
            }

            #[test]
            fn fuzz_safe_path_segment_rejects_null_bytes(s in r"\PC*") {
                if s.contains('\0') {
                    prop_assert!(validate_safe_path_segment(&s, "fuzz").is_err());
                }
            }

            #[test]
            fn fuzz_identifier_names_reject_leading_digits(s in r"[0-9][a-zA-Z0-9_-]*") {
                prop_assume!(!s.is_empty() && s.len() <= 40);
                prop_assert!(validate_contract_name(&s).is_err());
                prop_assume!(s.len() <= 64);
                prop_assert!(validate_project_name(&s).is_err());
            }
        }
    }
}

#[cfg(test)]
mod init_tests {
    use super::InitRollback;
    use std::path::PathBuf;

    #[tokio::test]
    async fn init_rollback_removes_created_frontend_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let frontend = tmp.path().join("frontend");
        let config = tmp.path().join("stacksdapp.toml");
        tokio::fs::create_dir_all(&frontend).await.unwrap();
        tokio::fs::write(&config, "marker = true\n").await.unwrap();
        tokio::fs::write(frontend.join("package.json"), "{}")
            .await
            .unwrap();

        let rollback = InitRollback {
            frontend_created: true,
            frontend_dir: frontend.clone(),
            created_files: vec![config.clone()],
            generated_touched: false,
            pre_commit_backup: None,
        };
        rollback.apply().await;

        assert!(!frontend.exists());
        assert!(!config.exists());
    }

    #[tokio::test]
    async fn init_rollback_restores_pre_commit_hook_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let hook = tmp.path().join(".githooks/pre-commit");
        tokio::fs::create_dir_all(hook.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&hook, "original\n").await.unwrap();

        let rollback = InitRollback {
            frontend_created: false,
            frontend_dir: PathBuf::from("frontend"),
            created_files: Vec::new(),
            generated_touched: false,
            pre_commit_backup: Some((hook.clone(), None)),
        };
        rollback.apply().await;

        assert!(!hook.exists());
    }
}
