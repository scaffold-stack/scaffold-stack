use anyhow::{anyhow, Result};
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network as BitcoinNetwork;
use bip39::Mnemonic;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::str::FromStr;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tempfile::NamedTempFile;

pub struct NetworkConfig {
    pub stacks_node: String,
}

pub fn network_config(network: &str) -> NetworkConfig {
    match network {
        "devnet"  => NetworkConfig { stacks_node: "http://localhost:3999".into() },
        "testnet" => NetworkConfig { stacks_node: "https://api.testnet.hiro.so".into() },
        "mainnet" => NetworkConfig { stacks_node: "https://api.hiro.so".into() },
        other     => panic!("Unknown network: {other}"),
    }
}

#[derive(Debug, Deserialize)]
struct ClarinetToml {
    contracts: Option<HashMap<String, ContractEntry>>,
}

#[derive(Debug, Deserialize)]
struct ContractEntry {
    path: String,
}

#[derive(Debug, Deserialize)]
struct DeploymentPlanFile {
    plan: DeploymentPlan,
}

#[derive(Debug, Deserialize)]
struct DeploymentPlan {
    batches: Vec<DeploymentBatch>,
}

#[derive(Debug, Deserialize)]
struct DeploymentBatch {
    transactions: Vec<DeploymentTransaction>,
}

#[derive(Debug, Deserialize, Clone)]
struct DeploymentTransaction {
    #[serde(rename = "transaction-type")]
    transaction_type: String,
    #[serde(rename = "contract-name")]
    contract_name: Option<String>,
    #[serde(rename = "expected-sender")]
    expected_sender: Option<String>,
    cost: Option<u64>,
    path: Option<String>,
    #[serde(rename = "clarity-version")]
    clarity_version: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct AccountResponse {
    nonce: u64,
}

#[derive(Debug, Deserialize)]
struct CoreInfoResponse {
    burn_block_height: u64,
    stacks_tip_height: u64,
}

#[derive(Serialize)]
struct DeploymentInfo {
    contract_id: String,
    tx_id: String,
    block_height: u64,
}

#[derive(Serialize)]
struct DeploymentFile {
    network: String,
    deployed_at: String,
    contracts: HashMap<String, DeploymentInfo>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn deploy(network: &str) -> Result<()> {
    if !Path::new("contracts/Clarinet.toml").exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
        ));
    }

    if network == "testnet" || network == "mainnet" {
        validate_settings_mnemonic(network)?;
    }

    let config = network_config(network);
    println!("🚀 Deploying to {} ({})", network, config.stacks_node);

    if network == "devnet" {
        wait_for_node(&config.stacks_node).await?;
    }

    deploy_via_clarinet(network).await
}

// ── Core deploy ───────────────────────────────────────────────────────────────

async fn deploy_via_clarinet(network: &str) -> Result<()> {
    // Always use --low-cost for fee estimation.
    // Testnet fee estimation is unreliable (low tx volume → extreme outliers).
    // Mainnet fees are set conservatively — increase to "--medium-cost" if
    // transactions are not confirming within a reasonable time.
    let fee_flag = "--low-cost";

    let contracts_dir = std::path::Path::new("contracts");
    let ordered = resolve_deployment_order(contracts_dir).await?;
    reorder_clarinet_toml(contracts_dir, &ordered).await?;

    // Step 1: resolve all conflicts BEFORE touching clarinet.
    // This runs until Clarinet.toml has no contracts that exist on-chain.
    if network == "testnet" || network == "mainnet" {
        println!("[deploy] Checking for contract name conflicts on {}...", network);
        auto_version_conflicting_contracts(network).await?;
    }

    // Step 2: generate + apply
    let clarinet_output = run_generate_and_apply(network, fee_flag).await?;

    // Step 3: if clarinet still reports ContractAlreadyExists (race condition
    // or something we missed), resolve again and retry once more.
    if clarinet_output.contains("ContractAlreadyExists") {
        println!("[deploy] Unexpected conflict after versioning — re-resolving and retrying...");
        auto_version_conflicting_contracts(network).await?;
        let clarinet_output2 = run_generate_and_apply(network, fee_flag).await?;
        return write_deployments_json_from_output(network, &clarinet_output2).await;
    }

    write_deployments_json_from_output(network, &clarinet_output).await
}

async fn reorder_clarinet_toml(
    contracts_dir: &std::path::Path,
    order: &[String],
) -> anyhow::Result<()> {
    let path = contracts_dir.join("Clarinet.toml");
    let raw = fs::read_to_string(&path).await?;

    // Split into the project header (everything before the first [contracts.])
    // and the individual contract blocks
    let first_contract = raw.find("\n[contracts.").unwrap_or(raw.len());
    let header = raw[..first_contract].to_string();

    // Extract each [contracts.<name>] block as a string
    let mut blocks: HashMap<String, String> = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_block = String::new();

    for line in raw[first_contract..].lines() {
        if let Some(name) = line.trim().strip_prefix("[contracts.").and_then(|s| s.strip_suffix(']')) {
            if let Some(prev) = current_name.take() {
                blocks.insert(prev, current_block.trim().to_string());
            }
            current_name = Some(name.to_string());
            current_block = format!("{line}\n");
        } else if current_name.is_some() {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }
    if let Some(prev) = current_name {
        blocks.insert(prev, current_block.trim().to_string());
    }

    // Reassemble in dependency order
    let mut output = header;
    for name in order {
        if let Some(block) = blocks.get(name) {
            output.push('\n');
            output.push_str(block);
            output.push('\n');
        }
    }

    fs::write(&path, output).await?;
    println!("[deploy] Clarinet.toml reordered to respect dependency graph.");
    Ok(())
}

/// Run `clarinet deployments generate` then `apply`, returning stdout.
async fn run_generate_and_apply(network: &str, fee_flag: &str) -> Result<String> {
    // Delete stale plan so clarinet never prompts "Overwrite? [Y/n]"
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    if Path::new(&plan_path).exists() {
        fs::remove_file(&plan_path).await?;
    }

    println!("[deploy] Generating deployment plan...");
    let gen = Command::new("clarinet")
        .args(["deployments", "generate", &format!("--{network}"), fee_flag])
        .current_dir("contracts")
        .status()
        .await
        .map_err(|_| anyhow!(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
        ))?;

    if !gen.success() {
        return Err(anyhow!(
            "Failed to generate deployment plan.\n\
             • Run `clarinet check` to validate your contracts.\n\
             • Ensure settings/{}.toml has a valid mnemonic.",
            capitalize(network)
        ));
    }

    check_plan_fee(network)?;

    if network == "devnet" {
        return run_apply_devnet_direct(network).await;
    }

    println!("[deploy] Applying deployment plan to {}...", network);
    let mut child = Command::new("clarinet")
        .args(["deployments", "apply", "--no-dashboard", &format!("--{network}")])
        .current_dir("contracts")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) 
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to open stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to open stdout"))?;
    

    let contracts_dir = std::path::Path::new("contracts");
    let expected_count = resolve_deployment_order(contracts_dir).await?.len();
    
    let mut confirmed_count = 0;
    let mut broadcast_count = 0;


    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut captured_stdout = String::new();

    
    while let Ok(Some(line)) = reader.next_line().await {
        println!("{}", line);
        captured_stdout.push_str(&line);
        captured_stdout.push('\n');

        if line.contains("REDEPLOYMENT REQUIRED") || line.contains("out of sync") {
            println!("[deploy] Error: Devnet is out of sync. You may need to restart Clarinet or increment contract version.");
            let _ = child.kill().await;
            return Err(anyhow!("Devnet redeployment required. Check your contract versions."));
        }

        // Handle interactive fee prompts
        if line.contains("Overwrite?") || line.contains("Confirm?") || line.contains("[Y/n]") {
            let _ = stdin.write_all(b"y\n").await;
            let _ = stdin.flush().await;
        }
        if line.contains("Broadcasted") && line.contains("ContractPublish(") {
            broadcast_count += 1;
            println!("[deploy] Broadcast progress: {}/{}", broadcast_count, expected_count);
        }

        if line.contains("Confirmed Publish") || line.contains("Published") {
            confirmed_count += 1;
            println!("[deploy] Confirmation progress: {}/{}", confirmed_count, expected_count);
        }

        if confirmed_count >= expected_count {
            println!("[deploy] All contracts confirmed. Finalizing JSON...");
            let _ = child.kill().await; // Clarinet can linger after local confirmations
            break;
        }

        if broadcast_count >= expected_count {
            println!("[deploy] All contracts broadcasted. Finalizing JSON...");
            let _ = child.kill().await; // Don't block on extra Clarinet output after broadcast
            break;
        }

    }
    Ok(captured_stdout)
}

async fn run_apply_devnet_direct(network: &str) -> Result<String> {
    println!("[deploy] Applying deployment plan to devnet...");
    let plan = read_deployment_plan(network).await?;
    let transactions = flatten_contract_publishes(&plan);
    if transactions.is_empty() {
        return Err(anyhow!("No contract publish transactions found in the devnet deployment plan."));
    }

    let settings_raw = fs::read_to_string("contracts/settings/Devnet.toml").await?;
    let mnemonic = parse_mnemonic(&settings_raw)
        .ok_or_else(|| anyhow!("No deployer mnemonic found in contracts/settings/Devnet.toml"))?;
    let derivation = parse_deployer_derivation(&settings_raw)
        .unwrap_or_else(|| "m/44'/5757'/0'/0/0".to_string());
    let sender_key = derive_private_key_from_mnemonic(&mnemonic, &derivation)?;

    let expected_sender = transactions
        .first()
        .and_then(|tx| tx.expected_sender.clone())
        .ok_or_else(|| anyhow!("No expected sender found in the devnet deployment plan."))?;
    let mut nonce = fetch_local_core_nonce(&expected_sender).await?;
    let script_path = write_devnet_broadcast_script()?;
    let mut captured_stdout = String::new();

    println!("[deploy] Broadcasting transactions to http://localhost:20443");

    for tx in transactions {
        let contract_name = tx.contract_name.clone()
            .ok_or_else(|| anyhow!("Missing contract name in deployment plan."))?;
        let contract_path = tx.path.clone()
            .ok_or_else(|| anyhow!("Missing contract path for {contract_name} in deployment plan."))?;
        let fee = tx.cost.unwrap_or(0);
        let args = serde_json::json!({
            "contractName": contract_name,
            "codePath": contract_path,
            "senderKey": sender_key,
            "fee": fee.to_string(),
            "nonce": nonce.to_string(),
            "clarityVersion": tx.clarity_version,
        });

        let output = Command::new("node")
            .arg(&script_path)
            .arg(args.to_string())
            .current_dir("contracts")
            .output()
            .await
            .map_err(|_| anyhow!("node is required to deploy directly to devnet"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow!(
                "Direct devnet deployment failed for {}.\nstdout:\n{}\nstderr:\n{}",
                tx.contract_name.as_deref().unwrap_or("unknown contract"),
                stdout.trim(),
                stderr.trim(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = serde_json::from_str(stdout.trim())
            .map_err(|e| anyhow!("Failed to parse devnet broadcast response: {e}\nRaw output: {}", stdout.trim()))?;
        let txid = result
            .get("txid")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("Devnet broadcast response did not include a txid: {}", stdout.trim()))?;

        println!("🟦  Publish {}.{}  Transaction broadcast {}", expected_sender, tx.contract_name.as_deref().unwrap_or(""), txid);
        captured_stdout.push_str(&format!(
            "Broadcasted ContractPublish(StandardPrincipalData({}), ContractName(\"{}\"), \"{}\")\n",
            expected_sender,
            tx.contract_name.as_deref().unwrap_or(""),
            txid,
        ));
        nonce += 1;
    }

    Ok(captured_stdout)
}

async fn read_deployment_plan(network: &str) -> Result<DeploymentPlanFile> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    let raw = fs::read_to_string(&plan_path).await
        .map_err(|e| anyhow!("Failed to read deployment plan at {plan_path}: {e}"))?;
    serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("Failed to parse deployment plan at {plan_path}: {e}"))
}

fn flatten_contract_publishes(plan: &DeploymentPlanFile) -> Vec<DeploymentTransaction> {
    plan.plan
        .batches
        .iter()
        .flat_map(|batch| batch.transactions.iter())
        .filter(|tx| tx.transaction_type == "contract-publish")
        .cloned()
        .collect()
}

fn write_devnet_broadcast_script() -> Result<std::path::PathBuf> {
    let mut file = NamedTempFile::new()?;
    let script = r#"
import fs from 'fs';
import { createRequire } from 'module';

const require = createRequire(`${process.cwd()}/package.json`);
const {
  makeContractDeploy,
  AnchorMode,
  PostConditionMode,
  broadcastRawTransaction,
} = require('@stacks/transactions');

const input = JSON.parse(process.argv[2]);
const codeBody = fs.readFileSync(input.codePath, 'utf8');

const transaction = await makeContractDeploy({
  contractName: input.contractName,
  codeBody,
  senderKey: input.senderKey,
  fee: BigInt(input.fee),
  nonce: BigInt(input.nonce),
  network: 'testnet',
  anchorMode: AnchorMode.OnChainOnly,
  postConditionMode: PostConditionMode.Allow,
  ...(typeof input.clarityVersion === 'number' ? { clarityVersion: input.clarityVersion } : {}),
});

const response = await broadcastRawTransaction(
  transaction.serialize(),
  'http://localhost:20443/v2/transactions',
);

console.log(JSON.stringify(response));
if (!response?.txid) {
  process.exit(1);
}
"#;
    use std::io::Write;
    file.write_all(script.as_bytes())?;
    let (_, path) = file.keep()?;
    Ok(path)
}

async fn fetch_local_core_nonce(address: &str) -> Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let url = format!("http://localhost:20443/v2/accounts/{address}?proof=0");
    let response = client.get(&url).send().await
        .map_err(|e| anyhow!("Failed to fetch local core account state from {url}: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Local core node returned {} for {}: {}",
            status,
            url,
            body
        ));
    }

    let account: AccountResponse = response.json().await?;
    Ok(account.nonce)
}

fn derive_private_key_from_mnemonic(mnemonic: &str, derivation: &str) -> Result<String> {
    let mnemonic = Mnemonic::parse_normalized(mnemonic)
        .map_err(|e| anyhow!("Invalid mnemonic in devnet settings: {e}"))?;
    let seed = mnemonic.to_seed_normalized("");
    let secp = Secp256k1::new();
    let root = Xpriv::new_master(BitcoinNetwork::Testnet, &seed)
        .map_err(|e| anyhow!("Failed to derive root key from mnemonic: {e}"))?;
    let path = DerivationPath::from_str(derivation)
        .map_err(|e| anyhow!("Invalid devnet derivation path {derivation}: {e}"))?;
    let child = root.derive_priv(&secp, &path)
        .map_err(|e| anyhow!("Failed to derive child key {derivation}: {e}"))?;
    Ok(format!("{}01", hex::encode(child.private_key.secret_bytes())))
}

pub async fn resolve_deployment_order(contracts_dir: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let clarinet_raw = fs::read_to_string(contracts_dir.join("Clarinet.toml")).await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow::anyhow!("Failed to parse Clarinet.toml: {e}"))?;

    let contract_map = clarinet.contracts.unwrap_or_default();
    let known: HashSet<String> = contract_map.keys().cloned().collect();

    // Build dependency map: name → [local deps]
    let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();

    for (name, entry) in &contract_map {
        let clar_path = contracts_dir.join(&entry.path);
        let source = fs::read_to_string(&clar_path).await.unwrap_or_default();
        let deps = parse_local_deps(&source, &known);

        if !deps.is_empty() {
            println!("[deploy] {name} depends on: {}", deps.join(", "));
        }

        dep_graph.insert(name.clone(), deps);
    }

    let order = topological_sort(&dep_graph)?;
    println!("[deploy] Deployment order: {}", order.join(" → "));

    Ok(order)
}

// ── Auto-versioning ───────────────────────────────────────────────────────────
fn check_plan_fee(network: &str) -> Result<()> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    let plan_raw = std::fs::read_to_string(&plan_path).unwrap_or_default();

    // Parse total cost from the YAML — look for "cost: <number>" lines and sum them
    let total_micro_stx: u64 = plan_raw
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("cost:") {
                trimmed.split_whitespace().nth(1)?.parse::<u64>().ok()
            } else {
                None
            }
        })
        .sum();
    if total_micro_stx > 0 {
        println!("[deploy] Estimated fee: {:.6} STX", total_micro_stx as f64 / 1_000_000.0);
    }

    Ok(())
}


async fn auto_version_conflicting_contracts(network: &str) -> Result<()> {
    let config = network_config(network);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let _ = Command::new("clarinet")
        .args(["deployments", "generate", &format!("--{}", network), "--low-cost"])
        .current_dir("contracts")
        .status()
        .await;

    let deployer = get_deployer_from_plan(network).await?;
    println!("[deploy] Using derived deployer address: {}", deployer);

    let base_dir = Path::new("contracts");
    let clarinet_path = base_dir.join("Clarinet.toml");
    let clarinet_raw = fs::read_to_string(&clarinet_path).await?;
    let mut clarinet_content = clarinet_raw.clone();
    
    let clarinet_struct: ClarinetToml = toml::from_str(&clarinet_raw)?;
    let contracts = clarinet_struct.contracts.unwrap_or_default();
    
    let mut any_changes = false;

    for (current_name, entry) in &contracts {
        let base_name = strip_version_suffix(current_name);
        
        // Find the next available name on the network
        let correct_name = find_next_free_name(&client, &config.stacks_node, &deployer, &base_name).await?;

        if current_name == &correct_name {
            continue;
        }

        println!("[deploy] Conflict detected: '{}' already exists on-chain. Renaming to '{}'", current_name, correct_name);

        let old_file_path = base_dir.join(&entry.path); 
        let new_rel_path = format!("contracts/{}.clar", correct_name);
        let new_file_path = base_dir.join(&new_rel_path);

        if old_file_path.exists() {
            fs::rename(&old_file_path, &new_file_path).await?;
            println!("[deploy] Renamed file: {} -> {}", entry.path, new_rel_path);
        }

        let old_header = format!("[contracts.{}]", current_name);
        let new_header = format!("[contracts.{}]", correct_name);
        clarinet_content = clarinet_content.replace(&old_header, &new_header);

        let old_path_line = format!("path = \"{}\"", entry.path);
        let new_path_line = format!("path = \"{}\"", new_rel_path);
        clarinet_content = clarinet_content.replace(&old_path_line, &new_path_line);

        let dot_old_name = format!(".{}", current_name);
        let dot_new_name = format!(".{}", correct_name);

        for (_, other_entry) in &contracts {
            let p = base_dir.join(&other_entry.path);
            
            let target_file = if p == old_file_path { &new_file_path } else { &p };

            if target_file.exists() {
                let source = fs::read_to_string(target_file).await?;
                if source.contains(&dot_old_name) {
                    let updated_source = source.replace(&dot_old_name, &dot_new_name);
                    fs::write(target_file, updated_source).await?;
                    println!("[deploy] Updated internal reference in {}", target_file.display());
                }
            }
        }

        any_changes = true;
    }

    if any_changes {
        fs::write(&clarinet_path, &clarinet_content).await?;
        
        // Remove all cached plans so Clarinet/SDK never hold onto stale
        // contract file paths after a version bump.
        for plan_name in [
            "default.devnet-plan.yaml",
            "default.simnet-plan.yaml",
            "default.testnet-plan.yaml",
            "default.mainnet-plan.yaml",
        ] {
            let plan_path = base_dir.join("deployments").join(plan_name);
            let _ = fs::remove_file(plan_path).await;
        }

        println!("[deploy] Clarinet.toml updated with new versions.");       
        // Regenerate bindings so the frontend/tests use the new names
        let _ = Command::new("stacksdapp").arg("generate").status().await;
    }

    Ok(())
}

/// Helper to parse the address Clarinet derived in the plan file
async fn get_deployer_from_plan(network: &str) -> Result<String> {
    let plan_path = format!("contracts/deployments/default.{}-plan.yaml", network);
    let content = fs::read_to_string(&plan_path).await
        .map_err(|_| anyhow!("Clarinet plan not found at {}. Is the path correct?", plan_path))?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("expected-sender:") {
            return Ok(trimmed.split(':').nth(1).unwrap_or("").trim().to_string());
        }
    }
    Err(anyhow!("Could not find 'expected-sender' in the deployment plan. Check your mnemonic in settings."))
}
/// Find the next free contract name starting from base_name (unversioned),
/// then base_name-v2, base_name-v3, etc.
async fn find_next_free_name(
    client: &reqwest::Client,
    node: &str,
    deployer: &str,
    base_name: &str,
) -> Result<String> {
    // Check unversioned first (e.g. "counter")
    let url = format!("{node}/v2/contracts/source/{deployer}/{base_name}");
    let base_free = !client.get(&url).send().await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    if base_free {
        return Ok(base_name.to_string());
    }

    // Find next free versioned name
    let mut version = 2u32;
    loop {
        let candidate = format!("{base_name}-v{version}");
        let url = format!("{node}/v2/contracts/interface/{deployer}/{candidate}");
        let taken = client.get(&url).send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if !taken {
            return Ok(candidate);
        }
        version += 1;
        if version > 99 {
            return Err(anyhow!(
                "Could not find a free version for '{base_name}' (tried up to v99).                  Consider using a fresh deployer address."
            ));
        }
    }
}


/// Strip trailing -vN suffix: "counter-v2" → "counter", "foo-v10" → "foo"
fn strip_version_suffix(name: &str) -> String {
    // Find last occurrence of -v followed by digits at end of string
    if let Some(idx) = name.rfind("-v") {
        let suffix = &name[idx + 2..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..idx].to_string();
        }
    }
    name.to_string()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validate_settings_mnemonic(network: &str) -> Result<()> {
    let path = format!("contracts/settings/{}.toml", capitalize(network));
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| anyhow!("Settings file not found: {path}"))?;
    let mnemonic = parse_mnemonic(&raw).unwrap_or_default();
    if mnemonic.is_empty() || mnemonic.contains('<') || mnemonic.contains('>') {
        return Err(anyhow!(
            "No valid mnemonic in {path}.\n\
             Add your deployer seed phrase:\n\n\
             [accounts.deployer]\n\
             mnemonic = \"your 24 words here\"\n\n\
             Get testnet STX: https://explorer.hiro.so/sandbox/faucet?chain=testnet"
        ));
    }
    Ok(())
}

fn parse_mnemonic(toml_raw: &str) -> Option<String> {
    let mut in_deployer = false;
    for line in toml_raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[accounts.deployer]" { in_deployer = true; continue; }
        if trimmed.starts_with('[') { in_deployer = false; }
        if in_deployer && trimmed.starts_with("mnemonic") {
            if let Some(val) = trimmed.splitn(2, '=').nth(1) {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn parse_deployer_derivation(toml_raw: &str) -> Option<String> {
    let mut in_deployer = false;
    for line in toml_raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[accounts.deployer]" { in_deployer = true; continue; }
        if trimmed.starts_with('[') { in_deployer = false; }
        if in_deployer && trimmed.starts_with("derivation") {
            if let Some(val) = trimmed.splitn(2, '=').nth(1) {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

async fn wait_for_node(url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    println!("[deploy] Waiting for Stacks node at {url}...");
    for attempt in 1..=60 {
        if client.get(&format!("{url}/v2/info")).send().await
            .map(|r| r.status().is_success()).unwrap_or(false)
        {
            println!("[deploy] ✔ Node is ready");
            return Ok(());
        }
        if attempt % 10 == 0 { println!("[deploy] Still waiting... ({attempt}s)"); }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(anyhow!(
        "Stacks node at {url} did not become ready after 60s.\n\
         Make sure `stacksdapp dev` is running and Docker is started."
    ))
}

async fn write_deployments_json_from_output(network: &str, output: &str) -> Result<()> {
    let mut txid_map: HashMap<String, String> = HashMap::new();
    let mut actual_deployer = None;
    for line in output.lines() {
        if line.contains("Broadcasted") {
            // Extract Deployer Address: Look for StandardPrincipalData(ADDRESS)
            if let Some(start) = line.find("StandardPrincipalData(") {
                let rest = &line[start + "StandardPrincipalData(".len()..];
                if let Some(end) = rest.find(')') {
                    actual_deployer = Some(rest[..end].to_string());
                }
            }

            // Extract Contract Name: Look for ContractName("NAME")
            let cn_marker = "ContractName(\"";
            if let Some(pos) = line.find(cn_marker) {
                let rest = &line[pos + cn_marker.len()..];
                if let Some(end) = rest.find('"') {
                    let contract_name = rest[..end].to_string();
                    
                    // Extract TXID: It's the 64-char hex string inside quotes at the end
                    // Format: ...), "TXID") Publish ...
                    let parts: Vec<&str> = line.split('"').collect();
                    for part in parts {
                        if part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()) {
                            txid_map.insert(contract_name.clone(), part.to_string());
                        }
                    }
                }
            }
        }
    }
    let settings_file = format!("contracts/settings/{}.toml", capitalize(network));
    let settings_raw = fs::read_to_string(&settings_file).await.unwrap_or_default();

    let deployer_address = actual_deployer
        .or_else(|| parse_deployer_address_from_settings(&settings_raw))
        .unwrap_or_else(|| "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM".to_string());

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow!("Failed to parse Clarinet.toml: {e}"))?;
    let contract_names: Vec<String> = clarinet
        .contracts
        .as_ref()
        .map(|contracts| contracts.keys().cloned().collect())
        .unwrap_or_default();

    if network == "devnet" {
        wait_for_devnet_contracts(&deployer_address, &contract_names).await?;
    }

    let mut contracts_map = HashMap::new();
    let timestamp = chrono::Utc::now().to_rfc3339();

    for name in contract_names {
        let contract_id = format!("{deployer_address}.{name}");
        let txid = txid_map.get(&name)
            .map(|t| format!("0x{t}"))
            .unwrap_or_default();
        println!("  ✔ {name} | txid {} | address {contract_id}",
            if txid.is_empty() { "(pending)" } else { &txid });
        contracts_map.insert(name.clone(), DeploymentInfo {
            contract_id, tx_id: txid, block_height: 0,
        });
    }

    let json = serde_json::to_string_pretty(&DeploymentFile {
        network: network.to_string(),
        deployed_at: timestamp,
        contracts: contracts_map,
    })?;

    let out_path = Path::new("frontend/src/generated/deployments.json");
    if let Some(p) = out_path.parent() { fs::create_dir_all(p).await?; }
    fs::write(out_path, &json).await?;
    println!("\n[deploy] Written to {}", out_path.display());
    Ok(())
}

async fn wait_for_devnet_contracts(deployer: &str, contract_names: &[String]) -> Result<()> {
    if contract_names.is_empty() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let node = "http://localhost:20443";
    let initial_info = fetch_local_core_info().await.ok();

    println!("[deploy] Verifying contract publish on local devnet core node...");
    for attempt in 1..=30 {
        let mut pending = Vec::new();

        for contract_name in contract_names {
            let url = format!(
                "{node}/v2/contracts/source/{deployer}/{contract_name}?proof=0"
            );
            let deployed = client
                .get(&url)
                .send()
                .await
                .map(|response| response.status().is_success())
                .unwrap_or(false);

            if !deployed {
                pending.push(contract_name.clone());
            }
        }

        if pending.is_empty() {
            println!("[deploy] ✔ Local devnet core node reports all contracts deployed");
            return Ok(());
        }

        if attempt == 1 || attempt % 5 == 0 {
            println!(
                "[deploy] Waiting for devnet core to expose: {}",
                pending.join(", ")
            );
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let nonce = fetch_local_core_nonce(deployer).await.unwrap_or_default();
    let stacks_api_healthy = probe_stacks_api_health().await.unwrap_or(false);
    let final_info = fetch_local_core_info().await.ok();
    let stall_hint = match (initial_info, final_info) {
        (Some(start), Some(end))
            if start.burn_block_height == end.burn_block_height
                && start.stacks_tip_height == end.stacks_tip_height =>
        {
            format!(
                "Local devnet appears stalled: burn block height stayed at {} and stacks tip height stayed at {} while waiting for confirmation.",
                end.burn_block_height, end.stacks_tip_height
            )
        }
        _ => "Local devnet tip did move during the wait, so the publish appears to be stuck independently of tip progression.".to_string(),
    };

    Err(anyhow!(
        "Devnet deploy did not finalize on the local Stacks core node.\n\
         The contract source never became available at http://localhost:20443 and the deployer nonce is still {nonce}.\n\
         This means the publish did not finalize on core, even if the explorer/mempool UI appears to show it.\n\
         {stall_hint}\n\
         {api_hint}\n\
         Try restarting devnet with `stacksdapp clean` and `stacksdapp dev`, then deploy again."
        ,
        stall_hint = stall_hint,
        api_hint = if stacks_api_healthy {
            "Local stacks-api responded normally, so the failure is on the core-chain side."
        } else {
            "Local stacks-api/indexer also appears unhealthy, so the explorer UI may be stale or misleading."
        }
    ))
}

async fn probe_stacks_api_health() -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    Ok(client
        .get("http://localhost:3999/v2/info")
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false))
}

async fn fetch_local_core_info() -> Result<CoreInfoResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    let response = client
        .get("http://localhost:20443/v2/info")
        .send()
        .await?;
    let response = response.error_for_status()?;
    Ok(response.json().await?)
}


fn parse_deployer_address_from_settings(toml_raw: &str) -> Option<String> {
    for line in toml_raw.lines() {
        let line = line.trim();
        if line.starts_with("# stx_address:") {
            return line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    None
}
fn parse_local_deps(source: &str, known_contracts: &HashSet<String>) -> Vec<String> {
    let mut deps = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Match: (contract-call? .token-name fn-name ...)
        //        (use-trait trait-name .token-name.trait-name)
        for pattern in &["contract-call? .", "use-trait "] {
            if let Some(pos) = trimmed.find(pattern) {
                let after = &trimmed[pos + pattern.len()..];
                // Extract the identifier up to the next whitespace or dot
                let name: String = after
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '.')
                    .collect();

                if !name.is_empty() && known_contracts.contains(&name) {
                    deps.push(name);
                }
            }
        }
    }

    deps.sort();
    deps.dedup();
    deps
}

fn topological_sort(
    contracts: &HashMap<String, Vec<String>>, // name → [deps]
) -> anyhow::Result<Vec<String>> {
    // Build in-degree map
    let mut in_degree: HashMap<&str, usize> = contracts
        .keys()
        .map(|k| (k.as_str(), 0))
        .collect();

    for deps in contracts.values() {
        for dep in deps {
            *in_degree.entry(dep.as_str()).or_insert(0) += 1;
        }
    }

    // Start with contracts that have no dependencies
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();

    // Sort for deterministic output
    let mut queue_vec: Vec<&str> = queue.drain(..).collect();
    queue_vec.sort();
    queue.extend(queue_vec);

    let mut sorted = Vec::new();

    while let Some(node) = queue.pop_front() {
        sorted.push(node.to_string());

        // Find all contracts that depend on this one and reduce their in-degree
        let mut next: Vec<&str> = contracts
            .iter()
            .filter(|(_, deps)| deps.iter().any(|d| d == node))
            .map(|(name, _)| name.as_str())
            .collect();
        next.sort();

        for dependent in next {
            let deg = in_degree.entry(dependent).or_insert(0);
            *deg = deg.saturating_sub(1);
            if *deg == 0 {
                queue.push_back(dependent);
            }
        }
    }

    if sorted.len() != contracts.len() {
        return Err(anyhow::anyhow!(
            "Circular contract dependency detected.\n\
             Check your contracts for circular contract-call? references.\n\
             Involved contracts: {}",
            contracts
                .keys()
                .filter(|k| !sorted.contains(k))
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(sorted)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::strip_version_suffix;

    #[test]
    fn test_strip_version_suffix() {
        assert_eq!(strip_version_suffix("counter"), "counter");
        assert_eq!(strip_version_suffix("counter-v2"), "counter");
        assert_eq!(strip_version_suffix("counter-v3"), "counter");
        assert_eq!(strip_version_suffix("counter-v10"), "counter");
        assert_eq!(strip_version_suffix("my-token-v2"), "my-token");
        // should not strip non-version suffixes
        assert_eq!(strip_version_suffix("counter-v"), "counter-v");
        assert_eq!(strip_version_suffix("counter-vault"), "counter-vault");
    }
}
