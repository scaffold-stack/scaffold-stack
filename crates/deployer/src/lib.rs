use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct NetworkConfig {
    pub stacks_node: String,
    pub bitcoin_node: String,
}

pub fn network_config(network: &str) -> NetworkConfig {
    match network {
        "devnet" => NetworkConfig {
            stacks_node: "http://localhost:3999".into(),
            bitcoin_node: "http://localhost:18443".into(),
        },
        "testnet" => NetworkConfig {
            stacks_node: "https://api.testnet.hiro.so".into(),
            bitcoin_node: "https://blockstream.info/testnet/api".into(),
        },
        "mainnet" => NetworkConfig {
            stacks_node: "https://api.hiro.so".into(),
            bitcoin_node: "https://blockstream.info/api".into(),
        },
        other => panic!("Unknown network: {other}"),
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

#[derive(Debug, Deserialize)]
struct BroadcastResponse {
    txid: Option<String>,
    error: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    nonce: u64,
}

pub async fn deploy(network: &str) -> Result<()> {
    let config = network_config(network);

    if !Path::new("contracts/Clarinet.toml").exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacks-dapp new"
        ));
    }

    println!("🚀 Deploying to {} ({})", network, config.stacks_node);

    match network {
        "devnet" => deploy_via_clarinet(network).await,
        "testnet" | "mainnet" => deploy_via_api(network, &config).await,
        other => Err(anyhow!("Unknown network: {other}")),
    }
}

// ── devnet ────────────────────────────────────────────────────────────────────

async fn deploy_via_clarinet(network: &str) -> Result<()> {
    // Wait for the Stacks node to be reachable before attempting deployment.
    // clarinet uses port 20443 internally; the API is on 3999.
    // We poll 3999 since that's what we advertise and control.
    wait_for_node("http://localhost:3999").await?;

    println!("[deploy] Generating deployment plan...");

    let generate = Command::new("clarinet")
        .args(["deployments", "generate", &format!("--{network}")])
        .current_dir("contracts")
        .status()
        .await
        .map_err(|_| anyhow!(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
        ))?;

    if !generate.success() {
        return Err(anyhow!(
            "Failed to generate deployment plan. Run `clarinet check` to validate your contracts."
        ));
    }

    println!("[deploy] Applying deployment plan to {}...", network);

    // Pipe "y\n" to stdin to auto-confirm the "Continue [Y/n]?" prompt.
    // Capture stdout so we can parse the real txids from clarinet output.
    let mut child = Command::new("clarinet")
        .args(["deployments", "apply", "--no-dashboard", &format!("--{network}")])
        .current_dir("contracts")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
        ))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"y\n").await?;
    }

    // Stream stdout to terminal while capturing it for txid parsing
    let output = child.wait_with_output().await?;

    // Always print what clarinet said
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    if !output.status.success() {
        return Err(anyhow!(
            "Deployment failed. Check that stacks-dapp dev is running and devnet is ready."
        ));
    }

    let clarinet_output = String::from_utf8_lossy(&output.stdout).to_string();
    write_deployments_from_clarinet_output(network, &clarinet_output).await
}

/// Poll the Stacks API until it responds or we time out.
/// Devnet takes ~30s to mine the first block and become ready.
async fn wait_for_node(url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    let info_url = format!("{url}/v2/info");

    println!("[deploy] Waiting for Stacks node at {url}...");

    for attempt in 1..=60 {
        if client.get(&info_url).send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            println!("[deploy] ✔ Node is ready (attempt {attempt})");
            return Ok(());
        }
        if attempt % 10 == 0 {
            println!("[deploy] Still waiting... ({attempt}s)");
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Err(anyhow!(
        "Stacks node at {url} did not become ready after 60s.\n\
         Make sure `stacks-dapp dev` is running and Docker is started."
    ))
}

// ── testnet / mainnet ─────────────────────────────────────────────────────────

async fn deploy_via_api(network: &str, config: &NetworkConfig) -> Result<()> {
    let private_key = std::env::var("DEPLOYER_PRIVATE_KEY").map_err(|_| {
        anyhow!(
            "Set DEPLOYER_PRIVATE_KEY env var or add deployer key to contracts/settings/{}.toml",
            capitalize(network)
        )
    })?;

    if private_key.trim().is_empty() {
        return Err(anyhow!(
            "Set DEPLOYER_PRIVATE_KEY env var or add deployer key to contracts/settings/{}.toml",
            capitalize(network)
        ));
    }

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow!("Failed to parse contracts/Clarinet.toml: {e}"))?;

    let contracts = clarinet.contracts.unwrap_or_default();
    if contracts.is_empty() {
        return Err(anyhow!("No contracts found in contracts/Clarinet.toml"));
    }

    let client = reqwest::Client::new();
    let deployer_address = derive_stx_address(&private_key, network)?;

    println!("[deploy] Deployer address: {deployer_address}");

    let mut nonce = fetch_nonce(&client, &config.stacks_node, &deployer_address).await?;
    let mut contracts_map = HashMap::new();
    let timestamp = chrono::Utc::now().to_rfc3339();

    for (contract_name, entry) in &contracts {
        let clar_path = Path::new("contracts").join(&entry.path);
        if !clar_path.exists() {
            eprintln!("[deploy] Warning: {} not found, skipping", clar_path.display());
            continue;
        }

        let source = fs::read_to_string(&clar_path).await?;
        println!("[deploy] Deploying contract: {contract_name}");

        let tx_bytes = build_contract_deploy_tx(&private_key, contract_name, &source, nonce, network)?;

        let txid = broadcast_with_retry(&client, &config.stacks_node, &tx_bytes, 3).await
            .map_err(|e| anyhow!(
                "Contract deploy failed (txid: none). Check node connectivity and account balance.\n{e}"
            ))?;

        let contract_id = format!("{deployer_address}.{contract_name}");
        println!("  ✔ {contract_name} | txid {txid} | address {contract_id}");

        contracts_map.insert(contract_name.clone(), DeploymentInfo {
            contract_id,
            tx_id: txid,
            block_height: 0,
        });

        nonce += 1;
    }

    write_deployments_json(network, &timestamp, contracts_map).await
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn fetch_nonce(client: &reqwest::Client, node: &str, address: &str) -> Result<u64> {
    let url = format!("{node}/v2/accounts/{address}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to reach Stacks node at {node}: {e}"))?
        .json::<AccountInfo>()
        .await
        .map_err(|e| anyhow!("Failed to parse account info: {e}"))?;
    Ok(resp.nonce)
}

async fn broadcast_with_retry(
    client: &reqwest::Client,
    node: &str,
    tx_bytes: &[u8],
    retries: u8,
) -> Result<String> {
    let url = format!("{node}/v2/transactions");
    let mut last_err = String::new();

    for attempt in 1..=retries {
        let resp = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(tx_bytes.to_vec())
            .send()
            .await;

        match resp {
            Ok(r) => {
                let parsed: BroadcastResponse = r
                    .json()
                    .await
                    .map_err(|e| anyhow!("Failed to parse broadcast response: {e}"))?;

                if let Some(txid) = parsed.txid {
                    return Ok(format!("0x{txid}"));
                }

                last_err = format!(
                    "{}: {}",
                    parsed.error.unwrap_or_default(),
                    parsed.reason.unwrap_or_default()
                );
                return Err(anyhow!("{last_err}"));
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt < retries {
                    eprintln!("[deploy] Attempt {attempt} failed: {e}. Retrying...");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    Err(anyhow!("All {retries} broadcast attempts failed: {last_err}"))
}

fn build_contract_deploy_tx(
    private_key: &str,
    contract_name: &str,
    source: &str,
    nonce: u64,
    network: &str,
) -> Result<Vec<u8>> {
    use std::io::Write;
    use std::process::Stdio;

    let script = Path::new("frontend/scripts/build-tx.mjs");
    if !script.exists() {
        return Err(anyhow!(
            "Transaction builder script not found at frontend/scripts/build-tx.mjs."
        ));
    }

    let input = serde_json::json!({
        "privateKey": private_key,
        "contractName": contract_name,
        "source": source,
        "nonce": nonce,
        "network": network,
    });

    let mut child = std::process::Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!("Node.js >=20 is required. Install from nodejs.org"))?;

    child.stdin.as_mut().unwrap()
        .write_all(serde_json::to_string(&input)?.as_bytes())?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to build transaction: {stderr}"));
    }

    let hex = String::from_utf8(output.stdout)?.trim().to_string();
    hex::decode(&hex).map_err(|e| anyhow!("Invalid transaction hex from build-tx.mjs: {e}"))
}

fn derive_stx_address(private_key: &str, network: &str) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let script = Path::new("frontend/scripts/build-tx.mjs");
    if !script.exists() {
        return Err(anyhow!(
            "frontend/scripts/build-tx.mjs not found. Re-scaffold with stacks-dapp new."
        ));
    }

    let input = serde_json::json!({
        "mode": "address",
        "privateKey": private_key,
        "network": network,
    });

    let mut child = std::process::Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!("Node.js >=20 is required. Install from nodejs.org"))?;

    child.stdin.as_mut().unwrap()
        .write_all(serde_json::to_string(&input)?.as_bytes())?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to derive address: {stderr}"));
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Parse txids directly from clarinet's broadcast output lines like:
/// ➡ Broadcasted(...) "86fa3030..." Publish ST1234.counter
async fn write_deployments_from_clarinet_output(network: &str, output: &str) -> Result<()> {
    let devnet_raw = fs::read_to_string("contracts/settings/Devnet.toml")
        .await
        .unwrap_or_default();
    let deployer_address = parse_deployer_address(&devnet_raw)
        .unwrap_or_else(|| "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM".to_string());

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow!("Failed to parse Clarinet.toml: {e}"))?;

    let mut contracts_map = HashMap::new();
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Parse txids from lines like:
    // ➡ Broadcasted(ContractPublish(...ContractName("counter")), "86fa3030...")
    let mut txid_map: HashMap<String, String> = HashMap::new();
    for line in output.lines() {
        // Match lines like:
        // Broadcasted(ContractPublish(...ContractName("counter")), "txid...")
        if !line.contains("Broadcasted") {
            continue;
        }
        // Extract contract name between ContractName(" and ")
        let cn_marker = r#"ContractName(""#;
        if let Some(pos) = line.find(cn_marker) {
            let rest = &line[pos + cn_marker.len()..];
            if let Some(end) = rest.find('"') {
                let contract_name = rest[..end].to_string();
                // Extract txid: last quoted string on the line
                // Format: ...ContractName("name")), "txid")
                let parts: Vec<&str> = line.split('"').collect();
                // txid is the second-to-last quoted segment (last is closing paren)
                if parts.len() >= 3 {
                    let txid = parts[parts.len() - 2].to_string();
                    if txid.len() == 64 {
                        txid_map.insert(contract_name, txid);
                    }
                }
            }
        }
    }
    for (name, _) in clarinet.contracts.unwrap_or_default() {
        let contract_id = format!("{deployer_address}.{name}");
        let txid = txid_map.get(&name)
            .map(|t| format!("0x{t}"))
            .unwrap_or_default();

        println!("  ✔ {name} | txid {} | address {contract_id}",
            if txid.is_empty() { "(pending)" } else { &txid });

        contracts_map.insert(name.clone(), DeploymentInfo {
            contract_id,
            tx_id: txid,
            block_height: 0,
        });
    }

    write_deployments_json(network, &timestamp, contracts_map).await
}

async fn write_deployments_from_scan(network: &str) -> Result<()> {
    let devnet_raw = fs::read_to_string("contracts/settings/Devnet.toml")
        .await
        .unwrap_or_default();
    let deployer_address = parse_deployer_address(&devnet_raw)
        .unwrap_or_else(|| "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM".to_string());

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow!("Failed to parse Clarinet.toml: {e}"))?;

    let mut contracts_map = HashMap::new();
    let timestamp = chrono::Utc::now().to_rfc3339();

    for (name, _) in clarinet.contracts.unwrap_or_default() {
        let contract_id = format!("{deployer_address}.{name}");
        contracts_map.insert(name.clone(), DeploymentInfo {
            contract_id: contract_id.clone(),
            tx_id: String::new(),
            block_height: 0,
        });
        println!("  ✔ {name} → {contract_id}");
    }

    write_deployments_json(network, &timestamp, contracts_map).await
}

fn parse_deployer_address(devnet_toml: &str) -> Option<String> {
    for line in devnet_toml.lines() {
        let line = line.trim();
        if line.starts_with("# stx_address:") {
            return line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    None
}

async fn write_deployments_json(
    network: &str,
    timestamp: &str,
    contracts: HashMap<String, DeploymentInfo>,
) -> Result<()> {
    let deployments = DeploymentFile {
        network: network.to_string(),
        deployed_at: timestamp.to_string(),
        contracts,
    };

    let json = serde_json::to_string_pretty(&deployments)?;
    let out_path = Path::new("frontend/src/generated/deployments.json");

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(out_path, &json).await?;
    println!("\n[deploy] Written to {}", out_path.display());
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}