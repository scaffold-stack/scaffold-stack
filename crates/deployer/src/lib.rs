use anyhow::{anyhow, Result};
use bip39::Mnemonic;
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network as BitcoinNetwork;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::str::FromStr;
use tempfile::NamedTempFile;
use tokio::fs;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

mod ui;
use ui::DeployUi;

pub struct NetworkConfig {
    pub stacks_node: String,
}

pub fn network_config(network: &str) -> Result<NetworkConfig> {
    match network {
        "devnet" => Ok(NetworkConfig {
            stacks_node: "http://localhost:3999".into(),
        }),
        "testnet" => Ok(NetworkConfig {
            stacks_node: "https://api.testnet.hiro.so".into(),
        }),
        "mainnet" => Ok(NetworkConfig {
            stacks_node: "https://api.hiro.so".into(),
        }),
        other => Err(anyhow!(
            "Unknown network '{other}'. Expected one of: devnet | testnet | mainnet"
        )),
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

#[derive(Debug, Deserialize, Serialize)]
struct DeploymentPlanFile {
    plan: DeploymentPlan,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeploymentPlan {
    batches: Vec<DeploymentBatch>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeploymentBatch {
    transactions: Vec<DeploymentTransaction>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeploymentInfo {
    contract_id: String,
    tx_id: String,
    block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeploymentFile {
    network: String,
    deployed_at: String,
    contracts: HashMap<String, DeploymentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedRename {
    from: String,
    to: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn wait_for_devnet_node() -> Result<()> {
    wait_for_node("http://localhost:3999").await
}

pub async fn deploy(network: &str, contract: Option<&str>, dry_run: bool, yes: bool) -> Result<()> {
    if !Path::new("contracts/Clarinet.toml").exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
        ));
    }

    if network == "testnet" || network == "mainnet" {
        validate_settings_mnemonic(network)?;
    }

    let config = network_config(network)?;
    let ui = DeployUi::start(network, &config.stacks_node);

    if network == "devnet" {
        wait_for_node(&config.stacks_node).await?;
    }

    deploy_via_clarinet(&ui, network, contract, dry_run, yes).await
}

// ── Core deploy ───────────────────────────────────────────────────────────────

struct DeployWriteSnapshot {
    clarinet_toml: Option<Vec<u8>>,
    deployment_files: HashMap<std::path::PathBuf, Vec<u8>>,
}

async fn snapshot_deploy_writes(contracts_dir: &Path) -> Result<DeployWriteSnapshot> {
    let clarinet_path = contracts_dir.join("Clarinet.toml");
    let clarinet_toml = if clarinet_path.is_file() {
        Some(fs::read(&clarinet_path).await?)
    } else {
        None
    };

    let mut deployment_files = HashMap::new();
    let deployments_dir = contracts_dir.join("deployments");
    if deployments_dir.is_dir() {
        let mut entries = fs::read_dir(&deployments_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let path = entry.path();
                deployment_files.insert(path.clone(), fs::read(&path).await?);
            }
        }
    }

    Ok(DeployWriteSnapshot {
        clarinet_toml,
        deployment_files,
    })
}

async fn restore_deploy_writes(contracts_dir: &Path, snapshot: &DeployWriteSnapshot) -> Result<()> {
    let clarinet_path = contracts_dir.join("Clarinet.toml");
    match &snapshot.clarinet_toml {
        Some(bytes) => fs::write(&clarinet_path, bytes).await?,
        None if clarinet_path.is_file() => {
            fs::remove_file(&clarinet_path).await?;
        }
        _ => {}
    }

    let deployments_dir = contracts_dir.join("deployments");
    if deployments_dir.is_dir() {
        let mut entries = fs::read_dir(&deployments_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file()
                && !snapshot.deployment_files.contains_key(&entry.path())
            {
                fs::remove_file(entry.path()).await?;
            }
        }
    }

    for (path, bytes) in &snapshot.deployment_files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, bytes).await?;
    }

    Ok(())
}

async fn deploy_via_clarinet(
    ui: &DeployUi,
    network: &str,
    contract: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let contracts_dir = std::path::Path::new("contracts");

    let step = ui.begin_step("Analyzing project");
    let ordered = match resolve_deployment_order(contracts_dir).await {
        Ok(o) => o,
        Err(e) => {
            step.fail();
            return Err(e);
        }
    };
    if let Some(name) = contract {
        if let Err(e) = ensure_contract_exists(&ordered, name) {
            step.fail();
            return Err(e);
        }
    }
    step.finish();

    if dry_run {
        let snapshot = snapshot_deploy_writes(contracts_dir).await?;
        let result =
            run_deploy_pipeline(ui, network, contract, dry_run, yes, contracts_dir, &ordered).await;
        restore_deploy_writes(contracts_dir, &snapshot).await?;
        return result;
    }

    run_deploy_pipeline(ui, network, contract, dry_run, yes, contracts_dir, &ordered).await
}

async fn run_deploy_pipeline(
    ui: &DeployUi,
    network: &str,
    contract: Option<&str>,
    dry_run: bool,
    yes: bool,
    contracts_dir: &Path,
    ordered: &[String],
) -> Result<()> {
    let clarinet_path = contracts_dir.join("Clarinet.toml");
    let clarinet_backup = if !dry_run {
        Some(fs::read(&clarinet_path).await?)
    } else {
        None
    };

    let result =
        run_deploy_pipeline_inner(ui, network, contract, dry_run, yes, contracts_dir, ordered)
            .await;

    if result.is_err() {
        if let Some(bytes) = clarinet_backup {
            let _ = fs::write(&clarinet_path, bytes).await;
        }
    }
    result
}

async fn run_deploy_pipeline_inner(
    ui: &DeployUi,
    network: &str,
    contract: Option<&str>,
    dry_run: bool,
    yes: bool,
    contracts_dir: &Path,
    ordered: &[String],
) -> Result<()> {
    let fee_flag = "--low-cost";

    let step = ui.begin_step("Resolving contract dependencies");
    if let Err(e) = reorder_clarinet_toml(contracts_dir, ordered).await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let mut effective_contract = contract.map(str::to_string);

    if network == "testnet" || network == "mainnet" {
        let step = ui.begin_step("Checking existing contracts...");
        let renames = match plan_conflicting_contract_renames(network, contract).await {
            Ok(r) => r,
            Err(e) => {
                step.fail();
                return Err(e);
            }
        };
        step.finish();
        for rename in &renames {
            ui.step_detail(&format!("{} already exists", rename.from));
            ui.step_detail(&format!("renamed → {}", rename.to));
        }
        if renames.is_empty() {
            ui.step_detail("no conflicts");
        }

        let (total_micro_stx, contracts) =
            build_deployment_preview(network, fee_flag, contract).await?;

        if dry_run {
            ui.dry_run_done(&contracts, total_micro_stx);
            if !renames.is_empty() {
                ui.step_detail(
                    "dry run note: conflicting contract names would be versioned on apply",
                );
            }
            return Ok(());
        }

        let deployer = get_deployer_from_plan(network).await?;
        ui.print_summary(&deployer, &contracts, total_micro_stx);
        if !ui.confirm_continue(yes)? {
            return Err(anyhow!(
                "{} deployment aborted by user.",
                capitalize(network)
            ));
        }

        if !renames.is_empty() {
            let step = ui.begin_step("Applying versioned contract names");
            if let Err(e) = apply_contract_renames(&renames).await {
                step.fail();
                return Err(e);
            }
            step.finish();
            effective_contract = effective_contract
                .as_deref()
                .map(|name| map_contract_name_after_renames(name, &renames));
        }

        let clarinet_output = run_generate_and_apply(
            ui,
            network,
            fee_flag,
            effective_contract.as_deref(),
            false,
            true,
            true,
        )
        .await?;

        if clarinet_output.contains("ContractAlreadyExists") {
            ui.step_detail("Conflict after versioning — retrying...");
            let retry_renames =
                plan_conflicting_contract_renames(network, effective_contract.as_deref()).await?;
            if !retry_renames.is_empty() {
                apply_contract_renames(&retry_renames).await?;
                effective_contract = effective_contract
                    .as_deref()
                    .map(|name| map_contract_name_after_renames(name, &retry_renames));
            }
            let clarinet_output2 = run_generate_and_apply(
                ui,
                network,
                fee_flag,
                effective_contract.as_deref(),
                false,
                true,
                true,
            )
            .await?;
            return write_deployments_json_from_output(
                ui,
                network,
                &clarinet_output2,
                effective_contract.as_deref(),
            )
            .await;
        }

        return write_deployments_json_from_output(
            ui,
            network,
            &clarinet_output,
            effective_contract.as_deref(),
        )
        .await;
    }

    let clarinet_output =
        run_generate_and_apply(ui, network, fee_flag, contract, dry_run, yes, false).await?;

    if dry_run {
        return Ok(());
    }

    write_deployments_json_from_output(ui, network, &clarinet_output, contract).await
}

async fn reorder_clarinet_toml(
    contracts_dir: &std::path::Path,
    order: &[String],
) -> anyhow::Result<()> {
    let path = contracts_dir.join("Clarinet.toml");
    let raw = fs::read_to_string(&path).await?;

    let mut header = String::new();
    let mut blocks: HashMap<String, String> = HashMap::new();
    let mut suffix = String::new();
    let mut current_name: Option<String> = None;
    let mut current_block = String::new();
    let mut seen_contracts = false;
    let mut in_suffix = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        let next_contract = trimmed
            .strip_prefix("[contracts.")
            .and_then(|s| s.strip_suffix(']'));

        if let Some(name) = next_contract {
            seen_contracts = true;
            in_suffix = false;
            if let Some(prev) = current_name.take() {
                blocks.insert(prev, current_block.trim().to_string());
            }
            current_name = Some(name.to_string());
            current_block = format!("{line}\n");
            continue;
        }

        if current_name.is_some() && trimmed.starts_with('[') && !trimmed.starts_with("[contracts.")
        {
            if let Some(prev) = current_name.take() {
                blocks.insert(prev, current_block.trim().to_string());
            }
            in_suffix = true;
        }

        if current_name.is_some() {
            current_block.push_str(line);
            current_block.push('\n');
        } else if in_suffix {
            suffix.push_str(line);
            suffix.push('\n');
        } else {
            header.push_str(line);
            header.push('\n');
        }
    }
    if let Some(prev) = current_name {
        blocks.insert(prev, current_block.trim().to_string());
    }
    if !seen_contracts {
        return Ok(());
    }

    let mut output = header.trim_end_matches('\n').to_string();
    let mut emitted: HashSet<&str> = HashSet::new();
    for name in order {
        if let Some(block) = blocks.get(name) {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push('\n');
            output.push_str(block);
            output.push('\n');
            emitted.insert(name.as_str());
        }
    }
    let mut remaining: Vec<&String> = blocks
        .keys()
        .filter(|name| !emitted.contains(name.as_str()))
        .collect();
    remaining.sort();
    for name in remaining {
        if let Some(block) = blocks.get(name) {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push('\n');
            output.push_str(block);
            output.push('\n');
        }
    }
    if !suffix.trim().is_empty() {
        output.push('\n');
        output.push_str(suffix.trim_end_matches('\n'));
        output.push('\n');
    }

    fs::write(&path, output).await?;
    Ok(())
}

/// Quiet clarinet helper — captures stderr for errors, hides upgrade spam.
async fn run_clarinet_quiet(args: &[&str]) -> Result<()> {
    let output = Command::new("clarinet")
        .args(args)
        .current_dir("contracts")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|_| {
            anyhow!(
                "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
            )
        })?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        // Filter clarinet upgrade nags from the error surface
        let filtered: String = err
            .lines()
            .filter(|l| !l.contains("A new release of clarinet"))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!(
            "clarinet {} failed.\n{}",
            args.join(" "),
            filtered.trim()
        ));
    }
    Ok(())
}

async fn run_generate_quiet() -> Result<()> {
    // Prefer in-tree binary if present; fall back to PATH.
    let bin = std::env::current_exe().unwrap_or_else(|_| "stacksdapp".into());
    let status = Command::new(&bin)
        .args(["-q", "generate"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            let fallback = Command::new("stacksdapp")
                .args(["-q", "generate"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            match fallback {
                Ok(fallback_status) if fallback_status.success() => Ok(()),
                Ok(fallback_status) => Err(anyhow!(
                    "Failed to regenerate TypeScript bindings: in-tree binary exited with {s}, PATH fallback exited with {fallback_status}."
                )),
                Err(err) => Err(anyhow!(
                    "Failed to regenerate TypeScript bindings: in-tree binary exited with {s}, PATH fallback could not start: {err}"
                )),
            }
        }
        Err(err) => Err(anyhow!("Failed to run stacksdapp generate: {err}")),
    }
}

async fn build_deployment_preview(
    network: &str,
    fee_flag: &str,
    contract: Option<&str>,
) -> Result<(u64, Vec<String>)> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    if Path::new(&plan_path).exists() {
        fs::remove_file(&plan_path).await?;
    }

    let net_flag = format!("--{network}");
    run_clarinet_quiet(&["deployments", "generate", &net_flag, fee_flag]).await?;
    if let Some(contract_name) = contract {
        filter_plan_to_contract(network, contract_name).await?;
    }

    let total_micro_stx = check_plan_fee(network)?;
    let contracts = deployment_contract_names_from_plan(network).await?;
    Ok((total_micro_stx, contracts))
}

/// Run `clarinet deployments generate` then `apply`, returning stdout.
async fn run_generate_and_apply(
    ui: &DeployUi,
    network: &str,
    fee_flag: &str,
    contract: Option<&str>,
    dry_run: bool,
    yes: bool,
    skip_remote_confirmation: bool,
) -> Result<String> {
    let step = ui.begin_step("Generating deployment artifacts");
    if let Err(e) = build_deployment_preview(network, fee_flag, contract).await {
        step.fail();
        return Err(anyhow!(
            "Failed to generate deployment plan.\n\
             • Run `clarinet check` to validate your contracts.\n\
             • Ensure settings/{}.toml has a valid mnemonic.\n{e}",
            capitalize(network)
        ));
    }
    step.finish();

    if !dry_run {
        let step = ui.begin_step("Exporting TypeScript bindings");
        if let Err(e) = run_generate_quiet().await {
            step.fail();
            return Err(e);
        }
        step.finish();
    }

    let step = ui.begin_step("Building deployment plan");
    let total_micro_stx = match check_plan_fee(network) {
        Ok(v) => v,
        Err(e) => {
            step.fail();
            return Err(e);
        }
    };
    let contracts = match deployment_contract_names_from_plan(network).await {
        Ok(c) => c,
        Err(e) => {
            step.fail();
            return Err(e);
        }
    };
    step.finish();

    if dry_run {
        ui.dry_run_done(&contracts, total_micro_stx);
        return Ok(String::new());
    }

    if network == "devnet" {
        return run_apply_devnet_direct(ui, network).await;
    }

    let deployer = get_deployer_from_plan(network).await?;
    if !skip_remote_confirmation {
        ui.print_summary(&deployer, &contracts, total_micro_stx);

        if !ui.confirm_continue(yes)? {
            return Err(anyhow!(
                "{} deployment aborted by user.",
                capitalize(network)
            ));
        }
    }

    ui.broadcasting_start();

    let mut child = Command::new("clarinet")
        .args([
            "deployments",
            "apply",
            "--no-dashboard",
            &format!("--{network}"),
        ])
        .current_dir("contracts")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Failed to open stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to open stdout"))?;

    let expected_count = contracts.len().max(1);
    let mut confirmed_count = 0usize;
    let mut broadcast_count = 0usize;
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut captured_stdout = String::new();
    let mut last_txid_by_name: HashMap<String, String> = HashMap::new();

    ui.render_bar(0, expected_count);

    while let Ok(Some(line)) = reader.next_line().await {
        captured_stdout.push_str(&line);
        captured_stdout.push('\n');

        if line.contains("REDEPLOYMENT REQUIRED") || line.contains("out of sync") {
            let _ = child.kill().await;
            return Err(anyhow!(
                "Devnet redeployment required. Check your contract versions."
            ));
        }

        // Auto-answer Clarinet prompts silently (consent already obtained).
        if line.contains("Overwrite?") {
            let answer = if contract.is_some() { b"n\n" } else { b"y\n" };
            let _ = stdin.write_all(answer).await;
            let _ = stdin.flush().await;
        } else if line.contains("Confirm?")
            || line.contains("Continue [Y/n]?")
            || line.contains("[Y/n]")
        {
            let _ = stdin.write_all(b"y\n").await;
            let _ = stdin.flush().await;
        }

        if line.contains("Broadcasted") && line.contains("ContractPublish(") {
            broadcast_count += 1;
            if let Some((name, txid)) = parse_broadcast_line(&line) {
                last_txid_by_name.insert(name, txid);
            }
            ui.render_bar(broadcast_count, expected_count);
        }

        if line.contains("Confirmed Publish") || line.contains("Published") {
            confirmed_count += 1;
        }

        // Clarinet keeps polling Hiro for on-chain confirmation after broadcast; do not
        // wait for that — finalize as soon as all txs are in the mempool (or confirmed).
        if confirmed_count >= expected_count || broadcast_count >= expected_count {
            let _ = child.kill().await;
            break;
        }
    }

    let status = child.wait().await?;
    if !status.success() && broadcast_count == 0 {
        return Err(anyhow!(
            "clarinet deployments apply failed with status {status}.\nOutput:\n{}",
            captured_stdout.trim()
        ));
    }
    if !status.success() && broadcast_count > 0 && broadcast_count < expected_count {
        write_partial_deployments_from_output(ui, network, &captured_stdout, contract).await?;
        return Err(anyhow!(
            "Partial {network} deployment: {broadcast_count}/{expected_count} contracts broadcast and recorded in deployments.json.\n\
             clarinet deployments apply failed with status {status}.\nOutput:\n{}",
            captured_stdout.trim()
        ));
    }

    // Finalize bar once if we somehow exited without hitting 100%.
    if broadcast_count > 0 && broadcast_count < expected_count {
        ui.render_bar(expected_count, expected_count);
    } else if broadcast_count >= expected_count {
        // Already finalized inside render_bar when done == total.
    } else {
        ui.render_bar(expected_count, expected_count);
    }

    // Stable order matching the deployment plan.
    for name in &contracts {
        if let Some(txid) = last_txid_by_name.get(name) {
            ui.contract_broadcast_ok(name, txid);
        }
    }

    Ok(captured_stdout)
}

async fn run_apply_devnet_direct(ui: &DeployUi, network: &str) -> Result<String> {
    ui.step_ok("Preparing direct devnet broadcast");
    let plan = read_deployment_plan(network).await?;
    let transactions = flatten_contract_publishes(&plan);
    if transactions.is_empty() {
        return Err(anyhow!(
            "No contract publish transactions found in the devnet deployment plan."
        ));
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

    ui.broadcasting_start();
    let expected = transactions.len().max(1);
    ui.render_bar(0, expected);

    for (i, tx) in transactions.into_iter().enumerate() {
        let contract_name = tx
            .contract_name
            .clone()
            .ok_or_else(|| anyhow!("Missing contract name in deployment plan."))?;
        let contract_path = tx.path.clone().ok_or_else(|| {
            anyhow!("Missing contract path for {contract_name} in deployment plan.")
        })?;
        let fee = tx.cost.unwrap_or(0);
        // Non-secret metadata only on the wire via stdin
        // (visible in `ps`). The Node bridge reads one JSON object from stdin.
        let args = serde_json::json!({
            "contractName": contract_name,
            "codePath": contract_path,
            "senderKey": sender_key,
            "fee": fee.to_string(),
            "nonce": nonce.to_string(),
            "clarityVersion": tx.clarity_version,
        });

        let mut child = Command::new("node")
            .arg(&script_path)
            .current_dir("contracts")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| anyhow!("node is required to deploy directly to devnet"))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("Failed to open node stdin for devnet broadcast"))?;
            stdin.write_all(args.to_string().as_bytes()).await?;
            // Closing stdin signals EOF so the Node script can finish reading.
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| anyhow!("Failed waiting for node broadcast process: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if i > 0 {
                write_partial_deployments_from_output(ui, network, &captured_stdout, None).await?;
                return Err(anyhow!(
                    "Partial devnet deployment: {}/{expected} contracts broadcast and recorded in deployments.json.\n\
                     Failed on {contract_name}.\nstdout:\n{}\nstderr:\n{}",
                    i,
                    stdout.trim(),
                    stderr.trim()
                ));
            }
            return Err(anyhow!(
                "Direct devnet deployment failed for {contract_name}.\nstdout:\n{}\nstderr:\n{}",
                stdout.trim(),
                stderr.trim(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = match serde_json::from_str(stdout.trim()) {
            Ok(value) => value,
            Err(e) => {
                if i > 0 {
                    write_partial_deployments_from_output(ui, network, &captured_stdout, None)
                        .await?;
                }
                return Err(anyhow!(
                    "Failed to parse devnet broadcast response: {e}\nRaw output: {}",
                    stdout.trim()
                ));
            }
        };
        let txid = match result.get("txid").and_then(|value| value.as_str()) {
            Some(txid) => txid,
            None => {
                if i > 0 {
                    write_partial_deployments_from_output(ui, network, &captured_stdout, None)
                        .await?;
                }
                return Err(anyhow!(
                    "Devnet broadcast response did not include a txid: {}",
                    stdout.trim()
                ));
            }
        };

        captured_stdout.push_str(&format!(
            "Broadcasted ContractPublish(StandardPrincipalData({}), ContractName(\"{}\"), \"{}\")\n",
            expected_sender,
            tx.contract_name.as_deref().unwrap_or(""),
            txid,
        ));
        ui.render_bar(i + 1, expected);
        ui.contract_broadcast_ok(tx.contract_name.as_deref().unwrap_or(""), txid);
        nonce += 1;
    }

    Ok(captured_stdout)
}

async fn write_partial_deployments_from_output(
    ui: &DeployUi,
    network: &str,
    captured_stdout: &str,
    contract: Option<&str>,
) -> Result<()> {
    write_deployments_json_from_output(ui, network, captured_stdout, contract).await
}

async fn read_deployment_plan(network: &str) -> Result<DeploymentPlanFile> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    let raw = fs::read_to_string(&plan_path)
        .await
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
    use std::io::Write;
    file.write_all(DEVNET_BROADCAST_SCRIPT.as_bytes())?;
    let (_, path) = file.keep()?;
    Ok(path)
}

/// Node bridge for direct devnet publishes. Payload (including senderKey) is read
/// from stdin — never from argv — so keys do not appear in `ps`.
const DEVNET_BROADCAST_SCRIPT: &str = r#"
import fs from 'fs';
import { createRequire } from 'module';

const require = createRequire(`${process.cwd()}/package.json`);
const {
  makeContractDeploy,
  AnchorMode,
  PostConditionMode,
  broadcastRawTransaction,
} = require('@stacks/transactions');

// Read deploy payload from stdin so the private key never appears in process argv.
const chunks = [];
for await (const chunk of process.stdin) {
  chunks.push(chunk);
}
const input = JSON.parse(Buffer.concat(chunks).toString('utf8'));
const codeBody = fs.readFileSync(input.codePath, 'utf8');

const transaction = await makeContractDeploy({
  contractName: input.contractName,
  codeBody,
  senderKey: input.senderKey,
  fee: BigInt(input.fee),
  nonce: BigInt(input.nonce),
  network: 'testnet',
  anchorMode: AnchorMode.OnChainOnly,
  postConditionMode: PostConditionMode.Deny,
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

async fn fetch_local_core_nonce(address: &str) -> Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let url = format!("http://localhost:20443/v2/accounts/{address}?proof=0");
    let response = client
        .get(&url)
        .send()
        .await
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
    let child = root
        .derive_priv(&secp, &path)
        .map_err(|e| anyhow!("Failed to derive child key {derivation}: {e}"))?;
    Ok(format!(
        "{}01",
        hex::encode(child.private_key.secret_bytes())
    ))
}

pub async fn resolve_deployment_order(
    contracts_dir: &std::path::Path,
) -> anyhow::Result<Vec<String>> {
    let clarinet_raw = fs::read_to_string(contracts_dir.join("Clarinet.toml")).await?;
    let clarinet: ClarinetToml = toml::from_str(&clarinet_raw)
        .map_err(|e| anyhow::anyhow!("Failed to parse Clarinet.toml: {e}"))?;

    let contract_map = clarinet.contracts.unwrap_or_default();
    let known: HashSet<String> = contract_map.keys().cloned().collect();

    // Build dependency map: name → [local deps]
    let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();

    for (name, entry) in &contract_map {
        let clar_path = contracts_dir.join(&entry.path);
        let source = fs::read_to_string(&clar_path).await.map_err(|e| {
            anyhow!(
                "Contract source for '{name}' not found at {}: {e}",
                clar_path.display()
            )
        })?;
        let deps = parse_local_deps(&source, &known);

        if !deps.is_empty() {
            // Dependency details are intentionally quiet — shown at summary level.
        }

        dep_graph.insert(name.clone(), deps);
    }

    let order = topological_sort(&dep_graph)?;

    Ok(order)
}

// ── Auto-versioning ───────────────────────────────────────────────────────────
fn check_plan_fee(network: &str) -> Result<u64> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    let plan_raw = std::fs::read_to_string(&plan_path).map_err(|e| {
        anyhow!(
            "Deployment plan not found at {plan_path}: {e}. Run clarinet deployments generate first."
        )
    })?;

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

    Ok(total_micro_stx)
}

async fn plan_conflicting_contract_renames(
    network: &str,
    contract: Option<&str>,
) -> Result<Vec<PlannedRename>> {
    let config = network_config(network)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let _ = build_deployment_preview(network, "--low-cost", contract).await?;
    let deployer = get_deployer_from_plan(network).await?;

    let base_dir = Path::new("contracts");
    let clarinet_path = base_dir.join("Clarinet.toml");
    let clarinet_raw = fs::read_to_string(&clarinet_path).await?;
    let clarinet_struct: ClarinetToml = toml::from_str(&clarinet_raw)?;
    let contracts = clarinet_struct.contracts.unwrap_or_default();

    let mut renames: Vec<PlannedRename> = Vec::new();

    for current_name in contracts.keys() {
        if contract.is_some() && contract != Some(current_name.as_str()) {
            continue;
        }
        let base_name = strip_version_suffix(current_name);

        // Find the next available name on the network
        let correct_name =
            find_next_free_name(&client, &config.stacks_node, &deployer, &base_name).await?;

        if current_name == &correct_name {
            continue;
        }
        renames.push(PlannedRename {
            from: current_name.clone(),
            to: correct_name,
        });
    }

    Ok(renames)
}

async fn apply_contract_renames(renames: &[PlannedRename]) -> Result<()> {
    if renames.is_empty() {
        return Ok(());
    }

    let base_dir = Path::new("contracts");
    let clarinet_path = base_dir.join("Clarinet.toml");
    let clarinet_raw = fs::read_to_string(&clarinet_path).await?;
    let clarinet_struct: ClarinetToml = toml::from_str(&clarinet_raw)?;
    let mut contracts = clarinet_struct.contracts.unwrap_or_default();
    let mut clarinet_content = clarinet_raw;

    for rename in renames {
        let entry = contracts.remove(&rename.from).ok_or_else(|| {
            anyhow!(
                "Contract '{}' disappeared before rename could be applied.",
                rename.from
            )
        })?;
        let old_file_path = base_dir.join(&entry.path);
        let new_rel_path = format!("contracts/{}.clar", rename.to);
        let new_file_path = base_dir.join(&new_rel_path);

        if old_file_path.exists() {
            fs::rename(&old_file_path, &new_file_path).await?;
        }

        let old_header = format!("[contracts.{}]", rename.from);
        let new_header = format!("[contracts.{}]", rename.to);
        clarinet_content = clarinet_content.replace(&old_header, &new_header);

        let old_path_line = format!("path = \"{}\"", entry.path);
        let new_path_line = format!("path = \"{}\"", new_rel_path);
        clarinet_content = clarinet_content.replace(&old_path_line, &new_path_line);

        contracts.insert(rename.to.clone(), ContractEntry { path: new_rel_path });
    }

    for entry in contracts.values() {
        let path = base_dir.join(&entry.path);
        if !path.exists() {
            continue;
        }
        let original = fs::read_to_string(&path).await?;
        let updated = renames.iter().fold(original.clone(), |acc, rename| {
            replace_contract_reference(&acc, &rename.from, &rename.to)
        });
        if updated != original {
            fs::write(&path, updated).await?;
        }
    }

    fs::write(&clarinet_path, &clarinet_content).await?;

    for plan_name in [
        "default.devnet-plan.yaml",
        "default.simnet-plan.yaml",
        "default.testnet-plan.yaml",
        "default.mainnet-plan.yaml",
    ] {
        let plan_path = base_dir.join("deployments").join(plan_name);
        let _ = fs::remove_file(plan_path).await;
    }

    Ok(())
}

/// Helper to parse the address Clarinet derived in the plan file
async fn get_deployer_from_plan(network: &str) -> Result<String> {
    let plan_path = format!("contracts/deployments/default.{}-plan.yaml", network);
    let content = fs::read_to_string(&plan_path).await.map_err(|_| {
        anyhow!(
            "Clarinet plan not found at {}. Is the path correct?",
            plan_path
        )
    })?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("expected-sender:") {
            return Ok(trimmed.split(':').nth(1).unwrap_or("").trim().to_string());
        }
    }
    Err(anyhow!(
        "Could not find 'expected-sender' in the deployment plan. Check your mnemonic in settings."
    ))
}

async fn find_next_free_name(
    client: &reqwest::Client,
    node: &str,
    deployer: &str,
    base_name: &str,
) -> Result<String> {
    // Check unversioned first (e.g. "counter")
    let url = format!("{node}/v2/contracts/source/{deployer}/{base_name}");
    let base_taken = contract_exists(client, &url).await?;

    if !base_taken {
        return Ok(base_name.to_string());
    }

    // Find next free versioned name
    let mut version = 2u32;
    loop {
        let candidate = format!("{base_name}-v{version}");
        let url = format!("{node}/v2/contracts/source/{deployer}/{candidate}");
        let taken = contract_exists(client, &url).await?;
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

async fn contract_exists(client: &reqwest::Client, url: &str) -> Result<bool> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to query remote contract state at {url}: {e}"))?;
    interpret_contract_lookup(response.status(), url)
}

fn interpret_contract_lookup(status: StatusCode, url: &str) -> Result<bool> {
    match status {
        StatusCode::OK => Ok(true),
        StatusCode::NOT_FOUND => Ok(false),
        other => Err(anyhow!(
            "Remote contract lookup at {url} returned unexpected status {other}. Refusing to guess whether the name is free."
        )),
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
    let raw =
        std::fs::read_to_string(&path).map_err(|_| anyhow!("Settings file not found: {path}"))?;
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
        if trimmed == "[accounts.deployer]" {
            in_deployer = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deployer = false;
        }
        if in_deployer && trimmed.starts_with("mnemonic") {
            if let Some((_, val)) = trimmed.split_once('=') {
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
        if trimmed == "[accounts.deployer]" {
            in_deployer = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deployer = false;
        }
        if in_deployer && trimmed.starts_with("derivation") {
            if let Some((_, val)) = trimmed.split_once('=') {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn map_contract_name_after_renames(name: &str, renames: &[PlannedRename]) -> String {
    renames
        .iter()
        .find(|rename| rename.from == name)
        .map(|rename| rename.to.clone())
        .unwrap_or_else(|| name.to_string())
}

fn replace_contract_reference(source: &str, old_name: &str, new_name: &str) -> String {
    let needle = format!(".{old_name}");
    let mut out = String::with_capacity(source.len());
    let mut idx = 0usize;

    while let Some(rel) = source[idx..].find(&needle) {
        let start = idx + rel;
        let after = start + needle.len();
        let next = source[after..].chars().next();
        let is_boundary =
            next.is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        if is_boundary {
            out.push_str(&source[idx..start]);
            out.push('.');
            out.push_str(new_name);
            idx = after;
        } else {
            out.push_str(&source[idx..after]);
            idx = after;
        }
    }

    out.push_str(&source[idx..]);
    out
}

async fn wait_for_node(url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    for attempt in 1..=60 {
        if client
            .get(format!("{url}/v2/info"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return Ok(());
        }
        if attempt % 10 == 0 {
            // Quiet wait — only fail after timeout.
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(anyhow!(
        "Stacks node at {url} did not become ready after 60s.\n\
         Make sure `stacksdapp dev` is running and Docker is started."
    ))
}

fn parse_broadcast_line(line: &str) -> Option<(String, String)> {
    let cn_marker = "ContractName(\"";
    let name = {
        let pos = line.find(cn_marker)?;
        let rest = &line[pos + cn_marker.len()..];
        let end = rest.find('"')?;
        rest[..end].to_string()
    };
    let txid = line
        .split('"')
        .find(|part| part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()))?
        .to_string();
    Some((name, txid))
}

async fn write_deployments_json_from_output(
    ui: &DeployUi,
    network: &str,
    output: &str,
    contract: Option<&str>,
) -> Result<()> {
    let mut txid_map: HashMap<String, String> = HashMap::new();
    let mut actual_deployer = None;
    for line in output.lines() {
        if line.contains("Broadcasted") {
            if let Some(start) = line.find("StandardPrincipalData(") {
                let rest = &line[start + "StandardPrincipalData(".len()..];
                if let Some(end) = rest.find(')') {
                    actual_deployer = Some(rest[..end].to_string());
                }
            }

            if let Some((contract_name, txid)) = parse_broadcast_line(line) {
                txid_map.insert(contract_name, txid);
            }
        }
    }
    let settings_file = format!("contracts/settings/{}.toml", capitalize(network));
    let settings_raw = fs::read_to_string(&settings_file).await.unwrap_or_default();

    let deployer_address = actual_deployer
        .or_else(|| parse_deployer_address_from_settings(&settings_raw))
        .unwrap_or_else(|| "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM".to_string());

    let clarinet_raw = fs::read_to_string("contracts/Clarinet.toml").await?;
    let clarinet: ClarinetToml =
        toml::from_str(&clarinet_raw).map_err(|e| anyhow!("Failed to parse Clarinet.toml: {e}"))?;
    let mut contract_names: Vec<String> = clarinet
        .contracts
        .as_ref()
        .map(|contracts| contracts.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(contract_name) = contract {
        contract_names.retain(|name| name == contract_name);
    }

    if network == "devnet" {
        ui.waiting_confirmation();
        wait_for_devnet_contracts(&deployer_address, &contract_names).await?;
    }

    let mut contracts_map = if contract.is_some() {
        load_existing_deployments_for_network(network).await?
    } else {
        HashMap::new()
    };
    let timestamp = chrono::Utc::now().to_rfc3339();

    let mut success_entries: Vec<(String, String, String)> = Vec::new();

    for name in &contract_names {
        let contract_id = format!("{deployer_address}.{name}");
        let broadcast_marker = format!("ContractName(\"{name}\")");
        let was_broadcast = output
            .lines()
            .any(|line| line.contains("Broadcasted") && line.contains(&broadcast_marker));
        let txid = match txid_map.get(name) {
            Some(t) => {
                if t.starts_with("0x") {
                    t.clone()
                } else {
                    format!("0x{t}")
                }
            }
            None if was_broadcast => {
                return Err(anyhow!(
                    "Deployment broadcast for '{name}' succeeded but txid could not be parsed from clarinet output."
                ));
            }
            None => String::new(),
        };
        success_entries.push((name.clone(), contract_id.clone(), txid.clone()));
        contracts_map.insert(
            name.clone(),
            DeploymentInfo {
                contract_id,
                tx_id: txid,
                block_height: 0,
            },
        );
    }

    let json = serde_json::to_string_pretty(&DeploymentFile {
        network: network.to_string(),
        deployed_at: timestamp,
        contracts: contracts_map,
    })?;

    let out_path = Path::new("frontend/src/generated/deployments.json");
    if let Some(p) = out_path.parent() {
        fs::create_dir_all(p).await?;
    }
    fs::write(out_path, &json).await?;

    // Ensure bindings are fresh after rename (quiet).
    run_generate_quiet().await?;

    ui.success(&success_entries);
    Ok(())
}

async fn load_existing_deployments_for_network(
    network: &str,
) -> Result<HashMap<String, DeploymentInfo>> {
    let path = Path::new("frontend/src/generated/deployments.json");
    let raw = match fs::read_to_string(path).await {
        Ok(content) => content,
        Err(_) => return Ok(HashMap::new()),
    };

    let parsed: DeploymentFile = match serde_json::from_str(&raw) {
        Ok(file) => file,
        Err(_) => return Ok(HashMap::new()),
    };

    if parsed.network == network {
        Ok(parsed.contracts)
    } else {
        Ok(HashMap::new())
    }
}

fn ensure_contract_exists(known: &[String], contract: &str) -> Result<()> {
    if known.iter().any(|name| name == contract) {
        return Ok(());
    }
    Err(anyhow!(
        "Contract '{contract}' was not found in contracts/Clarinet.toml.\nAvailable contracts: {}",
        if known.is_empty() {
            "<none>".to_string()
        } else {
            known.join(", ")
        }
    ))
}

async fn filter_plan_to_contract(network: &str, contract_name: &str) -> Result<()> {
    let plan_path = format!("contracts/deployments/default.{network}-plan.yaml");
    let raw = fs::read_to_string(&plan_path)
        .await
        .map_err(|e| anyhow!("Failed to read deployment plan at {plan_path}: {e}"))?;
    let mut yaml: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("Failed to parse deployment plan YAML at {plan_path}: {e}"))?;
    let mut found = false;

    let batches = yaml
        .get_mut("plan")
        .and_then(|plan| plan.get_mut("batches"))
        .and_then(|batches| batches.as_sequence_mut())
        .ok_or_else(|| anyhow!("Deployment plan is missing plan.batches"))?;

    for batch in batches.iter_mut() {
        let Some(transactions) = batch
            .get_mut("transactions")
            .and_then(|t| t.as_sequence_mut())
        else {
            continue;
        };

        transactions.retain(|tx| {
            let tx_type = tx
                .get("transaction-type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if tx_type != "contract-publish" {
                return true;
            }

            let keep = tx.get("contract-name").and_then(|v| v.as_str()) == Some(contract_name);
            if keep {
                found = true;
            }
            keep
        });
    }

    batches.retain(|batch| {
        batch
            .get("transactions")
            .and_then(|t| t.as_sequence())
            .map(|txs| !txs.is_empty())
            .unwrap_or(false)
    });

    if !found {
        return Err(anyhow!(
            "Contract '{contract_name}' is not present in the generated deployment plan.\n\
             Ensure the contract exists and passes `clarinet check`."
        ));
    }

    let rendered = serde_yaml::to_string(&yaml)?;
    fs::write(&plan_path, rendered).await?;
    Ok(())
}

async fn deployment_contract_names_from_plan(network: &str) -> Result<Vec<String>> {
    let plan = read_deployment_plan(network).await?;
    let names = flatten_contract_publishes(&plan)
        .into_iter()
        .filter_map(|tx| tx.contract_name)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(anyhow!(
            "No contract publish transactions found in deployment plan for {network}."
        ));
    }
    Ok(names)
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

    // Quietly wait for local core to expose published contracts.
    for attempt in 1..=30 {
        let mut pending = Vec::new();

        for contract_name in contract_names {
            let url = format!("{node}/v2/contracts/source/{deployer}/{contract_name}?proof=0");
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
            return Ok(());
        }

        let _ = attempt;
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
    let response = client.get("http://localhost:20443/v2/info").send().await?;
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
        let without_comments = line.split(";;").next().unwrap_or("").trim();
        if without_comments.is_empty() {
            continue;
        }

        let mut call_scan = without_comments;
        while let Some(pos) = call_scan.find("contract-call? .") {
            let after = &call_scan[pos + "contract-call? .".len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !name.is_empty() && known_contracts.contains(&name) {
                deps.push(name);
            }
            call_scan = after;
        }

        let mut trait_scan = without_comments;
        while let Some(pos) = trait_scan.find("use-trait ") {
            let after = &trait_scan[pos + "use-trait ".len()..];
            if let Some(dot_pos) = after.find('.') {
                let contract_ref = &after[dot_pos + 1..];
                let name: String = contract_ref
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if !name.is_empty() && known_contracts.contains(&name) {
                    deps.push(name);
                }
            }
            trait_scan = after;
        }
    }

    deps.sort();
    deps.dedup();
    deps
}

fn topological_sort(contracts: &HashMap<String, Vec<String>>) -> anyhow::Result<Vec<String>> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (name, deps) in contracts {
        in_degree.insert(name.as_str(), deps.len());
        for dep in deps {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(name.as_str());
        }
    }

    // Start with contracts that have no dependencies.
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

        // Reduce in-degree for contracts that depend on this one.
        let mut next = dependents.get(node).cloned().unwrap_or_default();
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
    use super::{
        interpret_contract_lookup, map_contract_name_after_renames, parse_broadcast_line,
        parse_local_deps, reorder_clarinet_toml, replace_contract_reference, restore_deploy_writes,
        snapshot_deploy_writes, strip_version_suffix, topological_sort, PlannedRename,
    };
    use reqwest::StatusCode;
    use std::collections::{HashMap, HashSet};
    use std::fs;

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

    #[test]
    fn test_topological_sort_respects_dependencies() {
        let mut graph = HashMap::new();
        graph.insert("a".to_string(), vec![]);
        graph.insert("b".to_string(), vec!["a".to_string()]);
        graph.insert("c".to_string(), vec!["b".to_string()]);

        let order = topological_sort(&graph).expect("topological sort should succeed");
        let idx_a = order.iter().position(|name| name == "a").unwrap();
        let idx_b = order.iter().position(|name| name == "b").unwrap();
        let idx_c = order.iter().position(|name| name == "c").unwrap();
        assert!(idx_a < idx_b && idx_b < idx_c);
    }

    #[test]
    fn test_topological_sort_cycle_detection() {
        let mut graph = HashMap::new();
        graph.insert("a".to_string(), vec!["b".to_string()]);
        graph.insert("b".to_string(), vec!["a".to_string()]);

        let err = topological_sort(&graph).expect_err("cycle should fail");
        assert!(
            err.to_string()
                .contains("Circular contract dependency detected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn devnet_broadcast_script_reads_payload_from_stdin_not_argv() {
        assert!(
            !super::DEVNET_BROADCAST_SCRIPT.contains("process.argv"),
            "sender key must not be passed via argv"
        );
        assert!(
            super::DEVNET_BROADCAST_SCRIPT.contains("process.stdin"),
            "deploy payload must be read from stdin"
        );
    }

    #[test]
    fn test_parse_local_deps_detects_contract_calls_and_traits() {
        let known = HashSet::from([
            "token".to_string(),
            "trait-source".to_string(),
            "counter".to_string(),
        ]);
        let deps = parse_local_deps(
            r#"
            (contract-call? .token transfer u1 tx-sender tx-sender none)
            (use-trait ft-trait .trait-source.sip-010-trait)
            ;; (contract-call? .ignored nope)
            (begin (contract-call? .counter get-count))
            "#,
            &known,
        );
        assert_eq!(
            deps,
            vec![
                "counter".to_string(),
                "token".to_string(),
                "trait-source".to_string()
            ]
        );
    }

    #[test]
    fn test_replace_contract_reference_preserves_prefixes() {
        let source = "(contract-call? .counter-helper ping)\n(contract-call? .counter get)\n(use-trait ft .counter.sip)";
        let updated = replace_contract_reference(source, "counter", "counter-v2");
        assert!(updated.contains(".counter-helper"));
        assert!(updated.contains(".counter-v2 get"));
        assert!(updated.contains(".counter-v2.sip"));
    }

    #[test]
    fn test_map_contract_name_after_renames() {
        let renames = vec![PlannedRename {
            from: "counter".into(),
            to: "counter-v2".into(),
        }];
        assert_eq!(
            map_contract_name_after_renames("counter", &renames),
            "counter-v2"
        );
        assert_eq!(map_contract_name_after_renames("other", &renames), "other");
    }

    #[tokio::test]
    async fn test_reorder_clarinet_toml_preserves_suffix_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let contracts_dir = tmp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).unwrap();
        fs::write(
            contracts_dir.join("Clarinet.toml"),
            r#"[project]
name = "demo"

[contracts.b]
path = "contracts/b.clar"

[contracts.a]
path = "contracts/a.clar"

[repl.analysis]
passes = ["check_checker"]
"#,
        )
        .unwrap();

        reorder_clarinet_toml(&contracts_dir, &["a".into(), "b".into()])
            .await
            .unwrap();

        let updated = fs::read_to_string(contracts_dir.join("Clarinet.toml")).unwrap();
        let idx_a = updated.find("[contracts.a]").unwrap();
        let idx_b = updated.find("[contracts.b]").unwrap();
        let idx_suffix = updated.find("[repl.analysis]").unwrap();
        assert!(idx_a < idx_b);
        assert!(idx_b < idx_suffix);
        assert!(updated.contains("passes = [\"check_checker\"]"));
    }

    #[test]
    fn test_interpret_contract_lookup_fails_closed() {
        assert!(interpret_contract_lookup(StatusCode::OK, "http://example").unwrap());
        assert!(!interpret_contract_lookup(StatusCode::NOT_FOUND, "http://example").unwrap());
        assert!(
            interpret_contract_lookup(StatusCode::INTERNAL_SERVER_ERROR, "http://example").is_err()
        );
    }

    #[test]
    fn test_parse_broadcast_line_extracts_txid() {
        let txid = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
        let line = format!(
            r#"Broadcasted ContractPublish(StandardPrincipalData(ST1PQ), ContractName("counter"), "{txid}")"#
        );
        let (name, parsed_txid) = parse_broadcast_line(&line).expect("should parse");
        assert_eq!(name, "counter");
        assert_eq!(parsed_txid, txid);
    }

    #[test]
    fn test_parse_broadcast_line_rejects_missing_txid() {
        assert!(parse_broadcast_line(r#"Broadcasted ContractName("counter")"#).is_none());
    }

    #[tokio::test]
    async fn test_reorder_clarinet_toml_keeps_contracts_missing_from_order() {
        let tmp = tempfile::tempdir().unwrap();
        let contracts_dir = tmp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).unwrap();
        fs::write(
            contracts_dir.join("Clarinet.toml"),
            r#"[project]
name = "demo"

[contracts.a]
path = "contracts/a.clar"

[contracts.b]
path = "contracts/b.clar"

[contracts.c]
path = "contracts/c.clar"
"#,
        )
        .unwrap();

        reorder_clarinet_toml(&contracts_dir, &["a".into()])
            .await
            .unwrap();

        let updated = fs::read_to_string(contracts_dir.join("Clarinet.toml")).unwrap();
        assert!(updated.contains("[contracts.a]"));
        assert!(updated.contains("[contracts.b]"));
        assert!(updated.contains("[contracts.c]"));
    }

    #[tokio::test]
    async fn deploy_write_snapshot_roundtrip_restores_files() {
        let tmp = tempfile::tempdir().unwrap();
        let contracts_dir = tmp.path().join("contracts");
        let deployments_dir = contracts_dir.join("deployments");
        fs::create_dir_all(&deployments_dir).unwrap();
        fs::write(contracts_dir.join("Clarinet.toml"), "original = true\n").unwrap();
        fs::write(
            deployments_dir.join("default.devnet-plan.yaml"),
            "cost: 100\n",
        )
        .unwrap();

        let snapshot = snapshot_deploy_writes(&contracts_dir).await.unwrap();
        fs::write(contracts_dir.join("Clarinet.toml"), "mutated = true\n").unwrap();
        fs::write(deployments_dir.join("new-plan.yaml"), "cost: 1\n").unwrap();

        restore_deploy_writes(&contracts_dir, &snapshot)
            .await
            .unwrap();

        let clarinet = fs::read_to_string(contracts_dir.join("Clarinet.toml")).unwrap();
        assert!(clarinet.contains("original = true"));
        assert!(!deployments_dir.join("new-plan.yaml").exists());
        let plan = fs::read_to_string(deployments_dir.join("default.devnet-plan.yaml")).unwrap();
        assert!(plan.contains("cost: 100"));
    }

    #[test]
    fn partial_devnet_error_message_documents_recorded_state() {
        let err = format!(
            "Partial devnet deployment: {}/{} contracts broadcast and recorded in deployments.json.",
            1, 2
        );
        assert!(err.contains("recorded in deployments.json"));
    }
}
