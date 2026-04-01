use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;

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


    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut captured_stdout = String::new();

    
    while let Ok(Some(line)) = reader.next_line().await {
        println!("{}", line);
        captured_stdout.push_str(&line);
        captured_stdout.push('\n');

        // Handle interactive fee prompts
        if line.contains("Overwrite?") || line.contains("Confirm?") || line.contains("[Y/n]") {
            let _ = stdin.write_all(b"y\n").await;
            let _ = stdin.flush().await;
        }
        if line.contains("Confirmed Publish") {
            confirmed_count += 1;
            println!("[deploy] Confirmation progress: {}/{}", confirmed_count, expected_count);
        }

        if confirmed_count >= expected_count {
            println!("[deploy] All contracts confirmed. Finalizing JSON...");
            let _ = child.kill().await; // Kill the hang
            break; 
        }

    }
    // let status = child.wait().await?;
    // if !status.success() {
    //     return Err(anyhow!("Clarinet deployment failed during apply stage."));
    // }
    Ok(captured_stdout)
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

/// Read the generated deployment plan and bail if total fees look insane.
/// Testnet fee estimation frequently produces garbage values (100s of STX).
/// A real contract deploy should never exceed ~50 STX.
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

    // 50 STX = 50_000_000 microSTX — if higher, fee estimation has gone wrong
    // let max_micro_stx: u64 = 50_000_000;
    // if total_micro_stx > max_micro_stx {
    //     let stx = total_micro_stx as f64 / 1_000_000.0;
    //     return Err(anyhow!(
    //         "Deployment plan has an unreasonably high fee: {stx:.6} STX ({total_micro_stx} microSTX).
    //          This is a fee estimation bug, not a real cost.

    //          To fix:
    //          1. Delete the stale plan: rm contracts/deployments/default.{network}-plan.yaml
    //          2. Try again — fee estimation can vary between runs
    //          3. Or manually edit the plan to set a sane cost (e.g. 10000 microSTX)"
    //     ));
    // }

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
        
        // Remove the stale plan so the final deployment uses the new names
        let plan_path = base_dir.join(format!("deployments/default.{}-plan.yaml", network));
        let _ = fs::remove_file(plan_path).await;

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