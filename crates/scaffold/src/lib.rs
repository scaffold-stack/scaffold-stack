use anyhow::{anyhow, Result};
use include_dir::{include_dir, Dir};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use which::which;

static FRONTEND_TEMPLATE: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../frontend-template");

pub async fn new_project(name: &str, git_init: bool) -> Result<()> {
    println!();
    println!("   \x1b[1;33mscaffold-stacks\x1b[0m  \x1b[2mv0.1.0\x1b[0m");
    println!("  \x1b[2m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!("  \x1b[1mCreating\x1b[0m  \x1b[1;36m{name}\x1b[0m");
    println!();

    ensure_prerequisites().await?;

    let root = Path::new(name);
    if root.exists() {
        return Err(anyhow!("  \x1b[31m✗\x1b[0m Directory '{name}' already exists"));
    }

    let style = ProgressStyle::with_template(
        "  {spinner:.yellow} {wide_msg:.dim}  \x1b[2m[{elapsed}]\x1b[0m"
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

    pb.println(format!("  \x1b[32m✔\x1b[0m  \x1b[1mScaffolded\x1b[0m   {name}/"));

    // ── Step 2: Install dependencies (parallel) ───────────────────────────────
    pb.set_message("Installing dependencies...");

    let fe_dir = frontend_dir.clone();
    let ct_dir = contracts_root.clone();

    let frontend_install = tokio::spawn(async move {
        Command::new("npm")
            .arg("install")
            .current_dir(&fe_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
    });

    let contracts_install = tokio::spawn(async move {
        Command::new("npm")
            .arg("install")
            .current_dir(&ct_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
    });

    let (fe, ct) = tokio::join!(frontend_install, contracts_install);

    match fe.unwrap() {
        Ok(s) if s.success() => {}
        _ => return Err(anyhow!("npm install failed in frontend/")),
    }
    match ct.unwrap() {
        Ok(s) if s.success() => {}
        _ => return Err(anyhow!("npm install failed in contracts/")),
    }

    pb.println("  \x1b[32m✔\x1b[0m  \x1b[1mInstalled\x1b[0m    node_modules");

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

        pb.println("  \x1b[32m✔\x1b[0m  \x1b[1mInitialised\x1b[0m  git (main)");
    }

    pb.finish_and_clear();

    // ── Success output ────────────────────────────────────────────────────────
    println!("  \x1b[1;32m✔ Done!\x1b[0m  Project \x1b[1;36m{name}\x1b[0m is ready.");
    println!("  \x1b[2m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!();
    println!("  \x1b[1;33m Recommended\x1b[0m  Deploy to testnet \x1b[2m(no Docker needed)\x1b[0m");
    println!();
    println!("     \x1b[1;36m1\x1b[0m  cd {name}");
    println!("     \x1b[1;36m2\x1b[0m  Get testnet STX \x1b[2m→\x1b[0m  https://explorer.hiro.so/sandbox/faucet?chain=testnet");
    println!("     \x1b[1;36m3\x1b[0m  Add mnemonic to \x1b[1mcontracts/settings/Testnet.toml\x1b[0m");
    println!("        \x1b[2m[accounts.deployer]\x1b[0m");
    println!("        \x1b[2mmnemonic = \"your 24 words here\"\x1b[0m");
    println!("     \x1b[1;36m4\x1b[0m  \x1b[1mstacksdapp deploy --network testnet\x1b[0m");
    println!("     \x1b[1;36m5\x1b[0m  \x1b[1mstacksdapp dev --network testnet\x1b[0m");
    println!();
    println!("  \x1b[2m───────────────────────────────────────\x1b[0m");
    println!();
    println!("  \x1b[1;34m Alternative\x1b[0m  Local devnet \x1b[2m(Docker required)\x1b[0m");
    println!();
    println!("     \x1b[1;36m1\x1b[0m  cd {name}  \x1b[2m+\x1b[0m  Start Docker Desktop");
    println!("     \x1b[1;36m2\x1b[0m  \x1b[1mstacksdapp dev\x1b[0m                               \x1b[2m← starts local chain + frontend\x1b[0m");
    println!("     \x1b[1;36m3\x1b[0m  \x1b[1mstacksdapp deploy --network devnet\x1b[0m           \x1b[2m← second terminal\x1b[0m");
    println!();
    println!("  \x1b[2m───────────────────────────────────────\x1b[0m");
    println!("  \x1b[2m  https://github.com/scaffold-stack/scaffold-stack\x1b[0m");
    println!();

    Ok(())
}

async fn write_project_files(
    name: &str,
    root: &Path,
    frontend_dir: &Path,
    contracts_root: &Path,
) -> Result<()> {
    tokio::fs::write(contracts_root.join("package.json"), r#"{
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
"#).await?;

    tokio::fs::write(contracts_root.join("vitest.config.ts"), r#"import { defineConfig } from 'vitest/config';
export default defineConfig({
  test: { environment: 'node' },
});
"#).await?;

    tokio::fs::write(contracts_root.join("tsconfig.json"), r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true
  },
  "include": ["tests/**/*.ts"]
}
"#).await?;

    tokio::fs::write(contracts_root.join("Clarinet.toml"), format!(r#"[project]
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
"#)).await?;

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

    tokio::fs::write(contracts_root.join("settings/Testnet.toml"), r#"[network]
name = "testnet"
stacks_node_rpc_address = "https://api.testnet.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE TESTNET MNEMONIC HERE>"
"#).await?;

    tokio::fs::write(contracts_root.join("settings/Mainnet.toml"), r#"[network]
name = "mainnet"
stacks_node_rpc_address = "https://api.hiro.so"
deployment_fee_rate = 10

[accounts.deployer]
mnemonic = "<YOUR PRIVATE MAINNET MNEMONIC HERE>"
"#).await?;

    tokio::fs::write(contracts_root.join("contracts/counter.clar"), r#";; counter.clar scaffolded by scaffold-stacks

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
"#).await?;

    tokio::fs::write(contracts_root.join("tests/counter.test.ts"), r#"import { describe, expect, it } from 'vitest';
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
"#).await?;

    tokio::fs::write(root.join("package.json"), format!(
        "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{\n    \"dev\": \"stacksdapp dev\",\n    \"generate\": \"stacksdapp generate\",\n    \"deploy\": \"stacksdapp deploy\",\n    \"test\": \"stacksdapp test\",\n    \"check\": \"stacksdapp check\"\n  }}\n}}\n"
    )).await?;

    tokio::fs::write(root.join(".gitignore"), r#"# Rust
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
"#).await?;

    tokio::fs::write(frontend_dir.join(".gitignore"), r#"node_modules/
.env
.env.local
.env.*.local
.next/
out/
.DS_Store
*.tsbuildinfo
next-env.d.ts
"#).await?;

    tokio::fs::write(contracts_root.join(".gitignore"), r#"node_modules/
.cache/
.devnet/
settings/Simnet.toml
.env
.env.local
.env.*.local
.DS_Store
"#).await?;

    tokio::fs::write(frontend_dir.join(".env.local"), r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex
"#).await?;

    tokio::fs::write(frontend_dir.join(".env.local.example"), r#"# Network: devnet | testnet | mainnet
NEXT_PUBLIC_NETWORK=devnet

# Required for testnet/mainnet deploy:
# DEPLOYER_PRIVATE_KEY=your_private_key_hex

# Optional node URL override:
# NEXT_PUBLIC_STACKS_NODE_URL=https://api.testnet.hiro.so
"#).await?;

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

    // ── Contract source ───────────────────────────────────────────────────────
    let (contract_source, test_source) = match template {

        "sip010" => (
            // SIP-010 Fungible Token
            // Trait: SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard
            format!(r#";; {name}.clar SIP-010 Fungible Token
(impl-trait 'SP3FBR2AGK5H9QBDH3EEN6DF8EK8JY7RX8QJ5SVTE.sip-010-trait-ft-standard.sip-010-trait)

;; Define the FT with no maximum supply
(define-fungible-token {name})

;; Error constants
(define-constant ERR_OWNER_ONLY (err u100))
(define-constant ERR_NOT_TOKEN_OWNER (err u101))

;; Contract constants
(define-constant CONTRACT_OWNER tx-sender)
(define-constant TOKEN_NAME "{name}")
(define-constant TOKEN_SYMBOL "{name}")
(define-constant TOKEN_DECIMALS u6)

(define-data-var token-uri (string-utf8 256) u"https://example.com/token-metadata.json")

;; SIP-010: Get token balance of a principal
(define-read-only (get-balance (who principal))
  (ok (ft-get-balance {name} who)))

;; SIP-010: Get total supply
(define-read-only (get-total-supply)
  (ok (ft-get-supply {name})))

;; SIP-010: Get human-readable token name
(define-read-only (get-name)
  (ok TOKEN_NAME))

;; SIP-010: Get ticker symbol
(define-read-only (get-symbol)
  (ok TOKEN_SYMBOL))

;; SIP-010: Get number of decimals
(define-read-only (get-decimals)
  (ok TOKEN_DECIMALS))

;; SIP-010: Get token metadata URI
(define-read-only (get-token-uri)
  (ok (some (var-get token-uri))))

;; Update token URI emits SIP-019 metadata update notification
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

;; Mint new tokens only contract owner
(define-public (mint (amount uint) (recipient principal))
  (begin
    (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
    (ft-mint? {name} amount recipient)))

;; SIP-010: Transfer tokens
;; Sender must be tx-sender or contract-caller to prevent unauthorised transfers
(define-public (transfer
  (amount uint)
  (sender principal)
  (recipient principal)
  (memo (optional (buff 34))))
  (begin
    ;; #[filter(amount, recipient)]
    (asserts! (or (is-eq tx-sender sender) (is-eq contract-caller sender)) ERR_NOT_TOKEN_OWNER)
    (try! (ft-transfer? {name} amount sender recipient))
    (match memo to-print (print to-print) 0x)
    (ok true)))
"#),
            // SIP-010 test
            format!(r#"import {{ describe, expect, it }} from 'vitest';
import {{ initSimnet }} from '@stacks/clarinet-sdk';
import {{ Cl, ClarityType }} from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const deployer = accounts.get('deployer')!;
const address1 = accounts.get('wallet_1')!;
const address2 = accounts.get('wallet_2')!;

describe('{name} (SIP-010)', () => {{
  it('mints tokens to a recipient', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'mint', [
      Cl.uint(1_000_000),
      Cl.principal(address1),
    ], deployer);
    expect(result).toBeOk(Cl.bool(true));
  }});

  it('only owner can mint', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'mint', [
      Cl.uint(1_000_000),
      Cl.principal(address1),
    ], address1);
    expect(result).toBeErr(Cl.uint(100));
  }});

  it('get-balance returns correct amount after mint', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.uint(1_000_000), Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-balance', [
      Cl.principal(address1),
    ], address1);
    expect(result).toBeOk(Cl.uint(1_000_000));
  }});

  it('get-total-supply returns total minted', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.uint(500_000), Cl.principal(address1)], deployer);
    simnet.callPublicFn('{name}', 'mint', [Cl.uint(500_000), Cl.principal(address2)], deployer);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-total-supply', [], address1);
    expect(result).toBeOk(Cl.uint(1_000_000));
  }});

  it('transfers tokens between principals', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.uint(1_000_000), Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callPublicFn('{name}', 'transfer', [
      Cl.uint(250_000),
      Cl.principal(address1),
      Cl.principal(address2),
      Cl.none(),
    ], address1);
    expect(result).toBeOk(Cl.bool(true));
  }});

  it('prevents transfer from non-owner', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.uint(1_000_000), Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callPublicFn('{name}', 'transfer', [
      Cl.uint(250_000),
      Cl.principal(address1),
      Cl.principal(address2),
      Cl.none(),
    ], address2);
    expect(result).toBeErr(Cl.uint(101));
  }});

  it('get-name returns token name', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-name', [], address1);
    expect(result).toBeOk(Cl.stringAscii('{name}'));
  }});

  it('get-symbol returns token symbol', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-symbol', [], address1);
    expect(result).toBeOk(Cl.stringAscii('{name}'));
  }});

  it('get-decimals returns 6', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-decimals', [], address1);
    expect(result).toBeOk(Cl.uint(6));
  }});

  it('get-token-uri returns some uri', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-token-uri', [], address1);
    expect(result).toHaveClarityType(ClarityType.ResponseOk);
  }});

  it('only owner can set-token-uri', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'set-token-uri', [
      Cl.stringUtf8('https://new-uri.com/metadata.json'),
    ], address1);
    expect(result).toBeErr(Cl.uint(100));
  }});
}});
"#),
        ),

        "sip009" => (
            // SIP-009 NFT
            // Trait: SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait.nft-trait
            format!(r#";; {name}.clar SIP-009 Non-Fungible Token
;; Implements the SIP-009 community-standard Non-Fungible Token trait.
(impl-trait 'SP2PABAF9FTAJYNFZH93XENAJ8FVY99RRM50D2JG9.nft-trait.nft-trait)

;; Define the NFT
(define-non-fungible-token {name} uint)

;; Keep track of the last minted token ID
(define-data-var last-token-id uint u0)

;; Error constants
(define-constant ERR_OWNER_ONLY (err u100))
(define-constant ERR_NOT_TOKEN_OWNER (err u101))
(define-constant ERR_SOLD_OUT (err u300))

;; Contract constants
(define-constant CONTRACT_OWNER tx-sender)
(define-constant COLLECTION_LIMIT u1000)

(define-data-var base-uri (string-ascii 256) "https://example.com/nft/{{id}}")

;; SIP-009: Get the last minted token ID
(define-read-only (get-last-token-id)
  (ok (var-get last-token-id)))

;; SIP-009: Get token metadata URI
(define-read-only (get-token-uri (token-id uint))
  (ok (some (var-get base-uri))))

;; SIP-009: Get the owner of a given token
(define-read-only (get-owner (token-id uint))
  (ok (nft-get-owner? {name} token-id)))

;; Update base URI — emits SIP-019 metadata update notification
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

;; SIP-009: Transfer NFT to another owner
(define-public (transfer (token-id uint) (sender principal) (recipient principal))
  (begin
    ;; #[filter(sender)]
    (asserts! (is-eq tx-sender sender) ERR_NOT_TOKEN_OWNER)
    (nft-transfer? {name} token-id sender recipient)))

;; Mint a new NFT only contract owner, up to COLLECTION_LIMIT
(define-public (mint (recipient principal))
  (let ((token-id (+ (var-get last-token-id) u1)))
    (asserts! (< (var-get last-token-id) COLLECTION_LIMIT) ERR_SOLD_OUT)
    (asserts! (is-eq tx-sender CONTRACT_OWNER) ERR_OWNER_ONLY)
    (try! (nft-mint? {name} token-id recipient))
    (var-set last-token-id token-id)
    (ok token-id)))
"#),
            // SIP-009 test
            format!(r#"import {{ describe, expect, it }} from 'vitest';
import {{ initSimnet }} from '@stacks/clarinet-sdk';
import {{ Cl, ClarityType }} from '@stacks/transactions';

const simnet = await initSimnet();
const accounts = simnet.getAccounts();
const deployer = accounts.get('deployer')!;
const address1 = accounts.get('wallet_1')!;
const address2 = accounts.get('wallet_2')!;

describe('{name} (SIP-009)', () => {{
  it('mints an NFT and returns token id 1', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'mint', [
      Cl.principal(address1),
    ], deployer);
    expect(result).toBeOk(Cl.uint(1));
  }});

  it('only owner can mint', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'mint', [
      Cl.principal(address1),
    ], address1);
    expect(result).toBeErr(Cl.uint(100));
  }});

  it('get-last-token-id increments after mint', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-last-token-id', [], address1);
    expect(result).toBeOk(Cl.uint(2));
  }});

  it('get-owner returns correct owner after mint', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-owner', [
      Cl.uint(1),
    ], address1);
    expect(result).toBeOk(Cl.some(Cl.principal(address1)));
  }});

  it('get-owner returns none for unminted token', () => {{
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-owner', [
      Cl.uint(999),
    ], address1);
    expect(result).toBeOk(Cl.none());
  }});

  it('transfers NFT to new owner', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callPublicFn('{name}', 'transfer', [
      Cl.uint(1),
      Cl.principal(address1),
      Cl.principal(address2),
    ], address1);
    expect(result).toBeOk(Cl.bool(true));
  }});

  it('prevents transfer from non-owner', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callPublicFn('{name}', 'transfer', [
      Cl.uint(1),
      Cl.principal(address1),
      Cl.principal(address2),
    ], address2);
    expect(result).toBeErr(Cl.uint(101));
  }});

  it('get-owner updates after transfer', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    simnet.callPublicFn('{name}', 'transfer', [
      Cl.uint(1),
      Cl.principal(address1),
      Cl.principal(address2),
    ], address1);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-owner', [Cl.uint(1)], address1);
    expect(result).toBeOk(Cl.some(Cl.principal(address2)));
  }});

  it('get-token-uri returns some uri', () => {{
    simnet.callPublicFn('{name}', 'mint', [Cl.principal(address1)], deployer);
    const {{ result }} = simnet.callReadOnlyFn('{name}', 'get-token-uri', [Cl.uint(1)], address1);
    expect(result).toHaveClarityType(ClarityType.ResponseOk);
  }});

  it('only owner can set-base-uri', () => {{
    const {{ result }} = simnet.callPublicFn('{name}', 'set-base-uri', [
      Cl.stringAscii('https://new-api.com/nft/{{id}}'),
    ], address1);
    expect(result).toBeErr(Cl.uint(100));
  }});
}});
"#),
        ),

        // blank template
        _ => (
            format!(";; {name}.clar\n\n(define-read-only (get-info)\n  (ok \"{name} contract\"))\n"),
            format!(r#"import {{ describe, expect, it }} from 'vitest';
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
"#),
        ),
    };

    tokio::fs::write(&path, contract_source).await?;

    // Write test file
    let test_path = Path::new("contracts/tests").join(format!("{name}.test.ts"));
    if !test_path.exists() {
        tokio::fs::write(&test_path, test_source).await?;
    }

    // Update Clarinet.toml
    let clarinet_toml_path = Path::new("contracts/Clarinet.toml");
    let mut existing = tokio::fs::read_to_string(clarinet_toml_path).await?;
    existing.push_str(&format!(
        "\n[contracts.{name}]\npath = \"contracts/{name}.clar\"\nclarity_version = 4\nepoch = \"latest\"\n"
    ));
    tokio::fs::write(clarinet_toml_path, existing).await?;

    codegen::generate_all().await?;

    println!(
        "  \x1b[32m✔\x1b[0m  \x1b[1mAdded\x1b[0m  contracts/contracts/{name}.clar"
    );
    println!(
        "  \x1b[32m✔\x1b[0m  \x1b[1mAdded\x1b[0m  contracts/tests/{name}.test.ts  \x1b[2m(bindings regenerated)\x1b[0m"
    );
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
            "\x1b[31m✗\x1b[0m clarinet is required.\n  Install: brew install clarinet  OR  cargo install clarinet"
        ));
    }
    Ok(())
}