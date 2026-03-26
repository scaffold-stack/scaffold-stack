use anyhow::{anyhow, Result};
use include_dir::{include_dir, Dir};
use std::path::Path;
use tokio::process::Command;
use which::which;

static FRONTEND_TEMPLATE: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../frontend-template");

pub async fn new_project(name: &str, git_init: bool) -> Result<()> {
    println!("⚡ scaffold-stacks — creating {name}");

    ensure_prerequisites().await?;

    let root = Path::new(name);
    if root.exists() {
        return Err(anyhow!("Target directory '{}' already exists", name));
    }
    tokio::fs::create_dir_all(root).await?;

    // Copy frontend template into <name>/frontend
    let frontend_dir = root.join("frontend");
    tokio::fs::create_dir_all(&frontend_dir).await?;
    FRONTEND_TEMPLATE
        .extract(&frontend_dir)
        .map_err(|e| anyhow!("Failed to copy frontend template: {e}"))?;

    // Contracts layout
    let contracts_root = root.join("contracts");
    tokio::fs::create_dir_all(contracts_root.join("contracts")).await?;
    tokio::fs::create_dir_all(contracts_root.join("settings")).await?;
    tokio::fs::create_dir_all(contracts_root.join("tests")).await?;

    // contracts/package.json
    let contracts_package = r#"{
  "name": "contracts",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "vitest run"
  },
  "devDependencies": {
    "@stacks/clarinet-sdk": "^3",
    "@stacks/transactions": "^6",
    "typescript": "^5",
    "vitest": "^1"
  }
}
"#;
    tokio::fs::write(contracts_root.join("package.json"), contracts_package).await?;

    // contracts/vitest.config.ts
    let vitest_config = r#"import { defineConfig } from 'vitest/config';
export default defineConfig({
  test: { environment: 'node' },
});
"#;
    tokio::fs::write(contracts_root.join("vitest.config.ts"), vitest_config).await?;

    // contracts/tsconfig.json
    let contracts_tsconfig = r#"{
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
    tokio::fs::write(contracts_root.join("tsconfig.json"), contracts_tsconfig).await?;

    // Clarinet.toml
    let clarinet_toml = format!(
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
    );
    tokio::fs::write(contracts_root.join("Clarinet.toml"), clarinet_toml).await?;

    // --- FIX: settings files must have [network] + funded accounts ---
    // These are the standard Clarinet devnet mnemonics (same as `clarinet new` generates).
    // Settings files — match exactly what `clarinet new` generates.
    // KEY FIXES vs previous versions:
    //   1. Devnet.toml includes sbtc_balance and [devnet] section with stacking orders.
    //   2. Testnet/Mainnet use placeholder mnemonic text (not empty string "").
    //      Clarinet rejects "" as invalid bip39 but accepts placeholder text fine.
    //   3. Testnet/Mainnet include stacks_node_rpc_address.
    //   4. Simnet.toml is intentionally NOT created — clarinet 3.x rejects it.
    let devnet_toml = r#"[network]
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
"#;

    let testnet_toml = r#"[network]
name = "testnet"
stacks_node_rpc_address = "https://api.testnet.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE TESTNET MNEMONIC HERE>"
"#;

    let mainnet_toml = r#"[network]
name = "mainnet"
stacks_node_rpc_address = "https://api.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE MAINNET MNEMONIC HERE>"
"#;

    // Only write the 3 files clarinet new creates — Simnet.toml must NOT exist.
    // Clarinet 3.x parses every *.toml in settings/ and rejects Simnet.toml.
    tokio::fs::write(contracts_root.join("settings/Devnet.toml"), devnet_toml).await?;
    tokio::fs::write(contracts_root.join("settings/Testnet.toml"), testnet_toml).await?;
    tokio::fs::write(contracts_root.join("settings/Mainnet.toml"), mainnet_toml).await?;

    // counter.clar
    let counter_clar = r#";; counter.clar sample contract scaffolded by scaffold-stacks

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
"#;
    tokio::fs::write(contracts_root.join("contracts/counter.clar"), counter_clar).await?;

    // counter.test.ts
    let counter_test = r#"import { describe, expect, it } from 'vitest';
import { initSimnet } from '@stacks/clarinet-sdk';
import { Cl } from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const address1 = accounts.get('wallet_1')!;

describe('counter', () => {
  it('increments', () => {
    const { result } = simnet.callPublicFn('counter', 'increment', [], address1);
    expect(result).toBeOk(Cl.uint(1));
  });
  it('get-count returns current value', () => {
    const { result } = simnet.callReadOnlyFn('counter', 'get-count', [], address1);
    expect(result).toBeOk(Cl.uint(1));
  });
  it('decrement', () => {
    const { result } = simnet.callPublicFn('counter', 'decrement', [], address1);
    expect(result).toBeOk(Cl.uint(0));
  });
});
"#;
    tokio::fs::write(contracts_root.join("tests/counter.test.ts"), counter_test).await?;

    // root package.json
    let root_package = format!(
        "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{\n    \"dev\": \"stacksdapp dev\",\n    \"generate\": \"stacksdapp generate\",\n    \"deploy\": \"stacksdapp deploy\",\n    \"test\": \"stacksdapp test\",\n    \"check\": \"stacksdapp check\"\n  }}\n}}\n"
    );
    tokio::fs::write(root.join("package.json"), root_package).await?;

    // .gitignore files — prevent node_modules, .env, generated files from being committed
    let root_gitignore = r#"# Rust
target/

# Node
node_modules/

# Environment — never commit real keys
.env
.env.local
.env.*.local

# Clarinet devnet state
contracts/.cache/
contracts/.devnet/
contracts/settings/Simnet.toml
contracts/settings

# Next.js build
frontend/.next/
frontend/out/


# OS
.DS_Store
*.pem
"#;
    tokio::fs::write(root.join(".gitignore"), root_gitignore).await?;

    let frontend_gitignore = r#"# dependencies
node_modules/

# environment variables — never commit real keys
.env
.env.local
.env.*.local

# Next.js
.next/
out/

# misc
.DS_Store
*.tsbuildinfo
next-env.d.ts
"#;
    tokio::fs::write(frontend_dir.join(".gitignore"), frontend_gitignore).await?;

    let contracts_gitignore = r#"# dependencies
node_modules/

# Clarinet cache and devnet state
.cache/
.devnet/
settings/Simnet.toml

# environment variables
.env
.env.local
.env.*.local

# OS
.DS_Store
"#;
    tokio::fs::write(contracts_root.join(".gitignore"), contracts_gitignore).await?;

    // .env.local — copy from example so the dev can switch networks immediately
    // We write devnet as the default so `stacksdapp dev` works out of the box.
    let env_local = r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex
"#;
    tokio::fs::write(frontend_dir.join(".env.local"), env_local).await?;
    // Also write the example file so devs know what's available
    tokio::fs::write(frontend_dir.join(".env.local.example"), r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex

# Optional node URL override:
# NEXT_PUBLIC_STACKS_NODE_URL=https://api.testnet.hiro.so
"#).await?;

    // npm install in frontend
    Command::new("npm")
        .arg("install")
        .current_dir(&frontend_dir)
        .spawn()?
        .wait()
        .await?;

    // npm install in contracts (for vitest + clarinet-sdk tests)
    Command::new("npm")
        .arg("install")
        .current_dir(&contracts_root)
        .spawn()?
        .wait()
        .await?;

    if git_init {
        Command::new("git")
            .arg("init")
            .current_dir(root)
            .spawn()?
            .wait()
            .await?;
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(root)
            .spawn()?
            .wait()
            .await?;
        Command::new("git")
            .args(["commit", "-m", "scaffold-stacks init"])
            .current_dir(root)
            .spawn()?
            .wait()
            .await?;
    }

    println!(
        "\n✅ Project '{name}' created successfully!\n\
         \n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         \n\
         👉 Recommended — deploy to testnet (no Docker needed):\n\
         \n\
         \t1. cd {name}\n\
         \t2. Get testnet STX from the faucet:\n\
         \t   https://explorer.hiro.so/sandbox/faucet?chain=testnet\n\
         \t3. Add your mnemonic to contracts/settings/Testnet.toml:\n\
         \t   [accounts.deployer]\n\
         \t   mnemonic = \"your 24 words here\"\n\
         \t4. stacksdapp deploy --network testnet\n\
         \t5. stacksdapp dev --network testnet\n\
         \n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         \n\
         🐳 Alternative — run locally with devnet (Docker required):\n\
         \n\
         \t1. cd {name}\n\
         \t2. Start Docker Desktop\n\
         \t3. stacksdapp dev               ← starts local chain + frontend\n\
         \t4. stacksdapp deploy --network devnet   ← in a second terminal\n\
         \n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         \n\
         📖 Docs: https://github.com/scaffold-stack/scaffold-stack\n"
    );
    Ok(())
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

    let contents = match template {
        "blank" | _ => format!(
            ";; {name}.clar\n\n(define-read-only (get-info)\n  (ok \"{name} contract\"))\n"
        ),
    };
    tokio::fs::write(&path, contents).await?;

    // Append [contracts.<name>] to Clarinet.toml
    let clarinet_toml_path = Path::new("contracts/Clarinet.toml");
    let mut existing = tokio::fs::read_to_string(clarinet_toml_path).await?;
    existing.push_str(&format!(
        "\n[contracts.{name}]\npath = \"contracts/{name}.clar\"\nclarity_version = 4\nepoch = \"latest\"\n\n[repl.costs_version]\nversion = 2\n"
    ));
    tokio::fs::write(clarinet_toml_path, existing).await?;

    // Regenerate bindings
    codegen::generate_all().await?;

    println!("[added] contracts/contracts/{name}.clar — bindings regenerated");
    Ok(())
}

async fn ensure_prerequisites() -> Result<()> {
    if which("node").is_err() {
        return Err(anyhow!(
            "Node.js >=20 is required. Install from nodejs.org"
        ));
    }
    if which("clarinet").is_err() {
        return Err(anyhow!(
            "clarinet is required. Install: brew install clarinet  OR  cargo install clarinet"
        ));
    }
    Ok(())
}