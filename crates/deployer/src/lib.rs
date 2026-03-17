use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct NetworkConfig {
    pub stacks_node: String,
}

pub fn network_config(network: &str) -> NetworkConfig {
    match network {
        "devnet"   => NetworkConfig { stacks_node: "http://localhost:3999".into() },
        "testnet"  => NetworkConfig { stacks_node: "https://api.testnet.hiro.so".into() },
        "mainnet"  => NetworkConfig { stacks_node: "https://api.hiro.so".into() },
        other      => panic!("Unknown network: {other}"),
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

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn deploy(network: &str) -> Result<()> {
    if !Path::new("contracts/Clarinet.toml").exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacks-dapp new"
        ));
    }

    // Validate settings file has a real mnemonic for testnet/mainnet
    if network == "testnet" || network == "mainnet" {
        validate_settings_mnemonic(network)?;
    }

    let config = network_config(network);
    println!("🚀 Deploying to {} ({})", network, config.stacks_node);

    // For devnet, wait for the node to be ready first
    if network == "devnet" {
        wait_for_node(&config.stacks_node).await?;
    }

    deploy_via_clarinet(network).await
}

// ── Core deploy — delegates entirely to clarinet ──────────────────────────────
// clarinet handles:
//   - Reading mnemonic from settings/<Network>.toml
//   - Signing transactions
//   - Broadcasting to the correct node
//   - Retry logic
// We just need to: generate the plan, apply it, then write deployments.json.

async fn deploy_via_clarinet(network: &str) -> Result<()> {
    println!("[deploy] Generating deployment plan...");

    let fee_flag = match network {
        "mainnet" => "--high-cost",
        "testnet" => "--medium-cost",
        _         => "--low-cost",
    };

    // Pipe "y\n" to auto-confirm the "Overwrite? [Y/n]" prompt during generate.
    let mut gen_child = Command::new("clarinet")
        .args(["deployments", "generate", &format!("--{network}"), fee_flag])
        .current_dir("contracts")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
        ))?;

    if let Some(mut stdin) = gen_child.stdin.take() {
        stdin.write_all(b"y\n").await?;
    }

    let gen_status = gen_child.wait().await?;
    if !gen_status.success() {
        return Err(anyhow!(
            "Failed to generate deployment plan.\n             • Run `clarinet check` to validate your contracts.\n             • For testnet/mainnet: ensure settings/{}.toml has a valid mnemonic.",
            capitalize(network)
        ));
    }


    println!("[deploy] Applying deployment plan to {}...", network);

    // Pipe "y\n" to auto-confirm the "Continue [Y/n]?" prompt.
    // Capture stdout to parse txids from clarinet's broadcast output.
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
        stdin.write_all(b"y\ny\ny\n").await?;
    }

    let output = child.wait_with_output().await?;

    // Print what clarinet said
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    if !output.status.success() {
        let hint = if network == "testnet" || network == "mainnet" {
            format!(
                "• Ensure settings/{}.toml has a funded mnemonic.\n\
                 • Get testnet STX: https://explorer.hiro.so/sandbox/faucet?chain=testnet",
                capitalize(network)
            )
        } else {
            "• Check that `stacks-dapp dev` is running and devnet is ready.".to_string()
        };
        return Err(anyhow!("Deployment failed.\n{hint}"));
    }

    let clarinet_output = String::from_utf8_lossy(&output.stdout).to_string();
    write_deployments_json_from_output(network, &clarinet_output).await
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Validate that settings/<Network>.toml has a non-placeholder mnemonic.
fn validate_settings_mnemonic(network: &str) -> Result<()> {
    let path = format!("contracts/settings/{}.toml", capitalize(network));
    let raw = std::fs::read_to_string(&path).map_err(|_| {
        anyhow!("Settings file not found: {path}")
    })?;

    let mnemonic = parse_mnemonic(&raw).unwrap_or_default();

    if mnemonic.is_empty() || mnemonic.contains('<') || mnemonic.contains('>') {
        return Err(anyhow!(
            "No valid mnemonic in {path}.\n\
             Add your deployer seed phrase:\n\
             \n\
             [accounts.deployer]\n\
             mnemonic = \"your 24 words here\"\n\
             \n\
             Get testnet STX: https://explorer.hiro.so/sandbox/faucet?chain=testnet"
        ));
    }
    Ok(())
}

fn parse_mnemonic(toml_raw: &str) -> Option<String> {
    let mut in_deployer = false;
    for line in toml_raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[accounts.deployer]" {
            in_deployer = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deployer = false;
        }
        if in_deployer && trimmed.starts_with("mnemonic") {
            if let Some(val) = trimmed.splitn(2, '=').nth(1) {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Poll the Stacks API until it responds or we time out.
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
            println!("[deploy] ✔ Node is ready");
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

/// Parse txids from clarinet broadcast output and write deployments.json.
async fn write_deployments_json_from_output(network: &str, output: &str) -> Result<()> {
    let devnet_raw = fs::read_to_string("contracts/settings/Devnet.toml")
        .await
        .unwrap_or_default();
    let deployer_address = parse_deployer_address_from_settings(&devnet_raw, network)
        .unwrap_or_else(|| "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM".to_string());

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow!("Failed to parse Clarinet.toml: {e}"))?;

    // Parse txids from clarinet output lines like:
    // ➡ Broadcasted(ContractPublish(...ContractName("counter")), "txid...")
    let mut txid_map: HashMap<String, String> = HashMap::new();
    for line in output.lines() {
        if !line.contains("Broadcasted") {
            continue;
        }
        let cn_marker = r#"ContractName(""#;
        if let Some(pos) = line.find(cn_marker) {
            let rest = &line[pos + cn_marker.len()..];
            if let Some(end) = rest.find('"') {
                let contract_name = rest[..end].to_string();
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 3 {
                    let txid = parts[parts.len() - 2].to_string();
                    if txid.len() == 64 {
                        txid_map.insert(contract_name, txid);
                    }
                }
            }
        }
    }

    let mut contracts_map = HashMap::new();
    let timestamp = chrono::Utc::now().to_rfc3339();

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

    let deployments = DeploymentFile {
        network: network.to_string(),
        deployed_at: timestamp,
        contracts: contracts_map,
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

fn parse_deployer_address_from_settings(toml_raw: &str, _network: &str) -> Option<String> {
    // Extract from comment: # stx_address: ST1PQHQ...
    for line in toml_raw.lines() {
        let line = line.trim();
        if line.starts_with("# stx_address:") {
            return line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    None
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}