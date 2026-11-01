use anyhow::{anyhow, Result};
use colored::Colorize;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::{fs, io::AsyncRead};

/// How `stacksdapp dev` should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevInvocation {
    /// `stacksdapp dev` — Clarinet owns this terminal.
    ClarinetToolchain,
    /// `stacksdapp dev --network <network>` — Next.js only.
    FrontendOnly { network: String },
}

pub fn classify_dev_invocation(network: Option<&str>) -> Result<DevInvocation> {
    match network {
        None => Ok(DevInvocation::ClarinetToolchain),
        Some(name @ ("devnet" | "testnet" | "mainnet")) => Ok(DevInvocation::FrontendOnly {
            network: name.to_string(),
        }),
        Some(other) => Err(anyhow!(
            "Unknown network '{other}'. Use: devnet, testnet, or mainnet"
        )),
    }
}

pub async fn dev(network: Option<&str>, auto_deploy: bool, keep_state: bool) -> Result<()> {
    ensure_project_root()?;

    match classify_dev_invocation(network)? {
        DevInvocation::ClarinetToolchain => dev_clarinet_toolchain(auto_deploy, keep_state).await,
        DevInvocation::FrontendOnly { network } => {
            if auto_deploy {
                stacksdapp_shell::warn(format!(
                    "[dev] --auto-deploy is not used. Deploy from another terminal: stacksdapp deploy --network {network}"
                ));
            }
            if keep_state {
                stacksdapp_shell::warn(
                    "[dev] --keep-state does not apply here. Clarinet keeps its own chain state.",
                );
            }
            dev_remote(&network).await
        }
    }
}

/// `stacksdapp dev` with no network flag: Clarinet's own devnet UI in this terminal.
/// Does not start Next.js, the contract watcher, or a deploy.
async fn dev_clarinet_toolchain(auto_deploy: bool, keep_state: bool) -> Result<()> {
    if auto_deploy {
        stacksdapp_shell::warn(
            "[dev] --auto-deploy is not used. Deploy from another terminal: stacksdapp deploy --network devnet",
        );
    }
    if keep_state {
        stacksdapp_shell::warn(
            "[dev] --keep-state is already how this command works. Reset the chain with `stacksdapp clean`.",
        );
    }

    ensure_docker()?;
    // Free ports held by a previous Clarinet devnet before the toolchain takes the terminal.
    stop_stale_devnet_docker();
    ensure_devnet_fast_boot_settings().await?;

    if !stacksdapp_shell::is_quiet() && !stacksdapp_shell::is_json() {
        println!();
        println!("{}", "Clarinet devnet".truecolor(244, 243, 239));
        println!(
            "{}",
            "  This terminal is the chain. Leave it running.".truecolor(156, 163, 175)
        );
        println!(
            "{}",
            "  Frontend   stacksdapp dev --network devnet".truecolor(156, 163, 175)
        );
        println!(
            "{}",
            "  Deploy     stacksdapp deploy --network devnet".truecolor(156, 163, 175)
        );
        println!();
    }

    // Clarinet 3.23 writes `rpc_ssl`, which stacks-node 4.0.5 rejects, and leaves
    // burnchain.wallet_name empty, which 4.0.5 also rejects. The `pox_5_*` lines
    // stay: 4.0.5 uses them to find the devnet sBTC contracts. Without them the
    // miner looks up a mainnet sbtc-registry and cannot build the PoX anchor.
    // Patch the file as it appears. Clarinet 3.26+ already emits a valid config
    // and starts its own bitcoin miner, so it is left alone.
    //
    // stacks-node 4.0.5 will not extend the restored snapshot tenure. Clarinet
    // only starts mining bitcoin after contract deployment, and deployment waits
    // for a Stacks block. Until that miner is running, a quiet chain gets one
    // bitcoin block so the node can publish a block-commit and leave the snapshot.
    let nudge_bitcoin = !stacksdapp_deployer::clarinet_version_at_least(3, 26, 0);
    let patcher = tokio::spawn(async move {
        let mut wallet_ready = !nudge_bitcoin;
        let mut stake_started = !nudge_bitcoin;
        let mut last_nudge_check = Instant::now();
        let mut observed_height: Option<u64> = None;
        let mut height_since = Instant::now();
        let mut miner_address: Option<String> = None;
        loop {
            let _ = patch_pox5_fields_in_cache().await;
            if !wallet_ready {
                wallet_ready = ensure_bitcoin_miner_wallet();
            }
            if nudge_bitcoin && wallet_ready && last_nudge_check.elapsed() >= BITCOIN_NUDGE_POLL {
                last_nudge_check = Instant::now();
                if !stake_started && stacks_node_accepts_connections() {
                    stake_started = spawn_pox5_stake();
                }
                nudge_bitcoin_if_chain_is_quiet(
                    &mut observed_height,
                    &mut height_since,
                    &mut miner_address,
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let mut clarinet = spawn_clarinet_foreground()?;
    let outcome = wait_for_clarinet_foreground(&mut clarinet).await;
    patcher.abort();
    let _ = patcher.await;

    match outcome? {
        ClarinetStop::Finished(status) if status.success() => Ok(()),
        ClarinetStop::Finished(status) => {
            Err(anyhow!("Clarinet devnet exited with status: {status}"))
        }
        ClarinetStop::Interrupted => Ok(()),
    }
}

enum ClarinetStop {
    Finished(std::process::ExitStatus),
    Interrupted,
}

async fn wait_for_clarinet_foreground(child: &mut Child) -> Result<ClarinetStop> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            status = child.wait() => {
                let status = status?;
                if status.success() {
                    Ok(ClarinetStop::Finished(status))
                } else if status.code().is_none() || status.code() == Some(130) || status.code() == Some(143) {
                    Ok(ClarinetStop::Interrupted)
                } else {
                    Ok(ClarinetStop::Finished(status))
                }
            }
            _ = sigint.recv() => {
                finish_clarinet_after_signal(child).await;
                Ok(ClarinetStop::Interrupted)
            }
            _ = sigterm.recv() => {
                finish_clarinet_after_signal(child).await;
                Ok(ClarinetStop::Interrupted)
            }
        }
    }
    #[cfg(not(unix))]
    {
        Ok(ClarinetStop::Finished(child.wait().await?))
    }
}

async fn finish_clarinet_after_signal(child: &mut Child) {
    // Clarinet is in this process group, so it already received the signal and
    // is shutting its containers down. Wait before forcing a kill.
    if tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .is_err()
        && child.id().is_some()
    {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

fn spawn_clarinet_foreground() -> Result<Child> {
    // No --no-dashboard: this command's job is Clarinet's own toolchain view.
    // Clarinet 3.23+ ships an epoch 4.0 / Clarity 6 snapshot. Older Clarinet needs
    // --from-genesis so a hybrid snapshot does not stall after one tenure.
    let mut args = vec!["devnet", "start"];
    if !stacksdapp_deployer::clarinet_version_at_least(3, 23, 0) {
        args.push("--from-genesis");
    }
    Command::new("clarinet")
        .args(&args)
        .current_dir("contracts")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| {
            anyhow!(
                "clarinet is required. Install: brew install clarinet  OR  cargo install clarinet"
            )
        })
}

fn ensure_project_root() -> Result<()> {
    if !Path::new("contracts/Clarinet.toml").exists()
        || !Path::new("frontend/package.json").exists()
    {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
        ));
    }
    Ok(())
}

fn network_display(network: &str) -> &'static str {
    match network {
        "mainnet" => "Mainnet",
        "devnet" => "Devnet",
        _ => "Testnet",
    }
}

fn print_ready_panel(network: &str, local_url: &str, tip_height: Option<u64>) {
    if stacksdapp_shell::is_quiet() {
        return;
    }
    println!();
    stacksdapp_shell::rule();
    println!();
    stacksdapp_shell::kv("Local", local_url);
    if let Some(height) = tip_height {
        stacksdapp_shell::kv("Devnet tip", &format!("#{height}"));
    }
    println!();
    if network == "devnet" {
        stacksdapp_shell::kv("Chain", "stacksdapp dev   (other terminal)");
        stacksdapp_shell::kv("Deploy", "stacksdapp deploy --network devnet");
    } else {
        stacksdapp_shell::kv("Deploy", &format!("stacksdapp deploy --network {network}"));
    }
    println!();
    stacksdapp_shell::rule();
    println!();
    println!(
        "{}",
        "Watching for file changes...".truecolor(156, 163, 175)
    );
    println!();
    stacksdapp_shell::rule();
}

/// JSON payload emitted once when `stacksdapp dev --json` reaches a ready frontend (not at start).
pub fn dev_ready_json_payload(
    network: &str,
    local_url: &str,
    tip_height: Option<u64>,
    auto_deploy: bool,
    keep_state: bool,
) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "command": "dev",
        "status": "ready",
        "network": network,
        "url": local_url,
        "tip_height": tip_height,
        "auto_deploy": auto_deploy,
        "keep_state": keep_state,
    })
}

fn notify_dev_ready(
    network: &str,
    local_url: &str,
    tip_height: Option<u64>,
    auto_deploy: bool,
    keep_state: bool,
) {
    if stacksdapp_shell::is_json() {
        stacksdapp_shell::emit_json(&dev_ready_json_payload(
            network,
            local_url,
            tip_height,
            auto_deploy,
            keep_state,
        ));
        return;
    }
    print_ready_panel(network, local_url, tip_height);
}

async fn dev_remote(network: &str) -> Result<()> {
    stacksdapp_shell::print_banner("Development Mode 🌱");

    let step = stacksdapp_shell::begin_step("Environment configured");
    check_deployments(network);
    if let Err(e) = write_network_env(network).await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let connected = format!("Connected to {}", network_display(network));
    let step = stacksdapp_shell::begin_step(&connected);
    // Network is selected via .env.local; no local chain handshake required.
    step.finish();

    let step = stacksdapp_shell::begin_step("Contract bindings ready");
    if let Err(e) = stacksdapp_codegen::generate_all_quiet().await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let step = stacksdapp_shell::begin_step("Next.js started");
    let mut frontend = match spawn_next_dev_process(network) {
        Ok(c) => c,
        Err(e) => {
            step.fail();
            return Err(e);
        }
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    attach_next_output(&mut frontend, Some(ready_tx));
    match tokio::time::timeout(Duration::from_secs(120), ready_rx).await {
        Ok(Ok(url)) => {
            step.finish();
            notify_dev_ready(network, &url, None, false, false);
        }
        Ok(Err(_)) => {
            step.fail();
            return Err(anyhow!("Frontend dev server closed before becoming ready."));
        }
        Err(_) => {
            step.fail();
            return Err(anyhow!(
                "Frontend did not become ready within 120s. Check that port 3000 is free."
            ));
        }
    }

    let mut watcher = tokio::spawn(stacksdapp_watcher::watch_contracts(Path::new(
        "contracts/contracts",
    )));

    let result = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            if !stacksdapp_shell::is_quiet() {
                println!();
                println!("{}", "Shutting down...".truecolor(156, 163, 175));
            }
            Ok(())
        }
        status = frontend.wait() => {
            match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(anyhow!("Frontend dev server exited with status: {s}")),
                Err(e) => Err(anyhow!("Failed to wait for frontend: {e}")),
            }
        }
        watcher_result = &mut watcher => {
            match watcher_result {
                Ok(Ok(())) => Err(anyhow!("Contract watcher exited unexpectedly.")),
                Ok(Err(e)) => Err(anyhow!("Contract watcher failed: {e}")),
                Err(e) => Err(anyhow!("Contract watcher task join failed: {e}")),
            }
        }
    };

    shutdown_child("frontend dev server", &mut frontend).await;
    watcher.abort();
    let _ = watcher.await;

    result
}

/// True when a generated Stacks.toml lost a field stacks-node 4.0.5 rejects.
async fn patch_pox5_fields_in_cache() -> Result<bool> {
    let cache = Path::new("contracts/.cache");
    if !cache.is_dir() {
        return Ok(false);
    }
    let mut removed_rejected = false;
    let mut entries = fs::read_dir(cache).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("stacks-devnet-") {
            continue;
        }
        let stacks_toml = entry.path().join("conf/Stacks.toml");
        if !stacks_toml.is_file() {
            continue;
        }
        if patch_stacks_toml_file(&stacks_toml).await? {
            removed_rejected = true;
        }
    }
    Ok(removed_rejected)
}

/// Returns true when the file contained a field stacks-node 4.0.5 rejects.
async fn patch_stacks_toml_file(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path).await?;
    let (updated, mut changed, removed_rejected) = strip_pox5_node_fields(&raw);
    let updated = if !stacksdapp_deployer::clarinet_version_at_least(3, 26, 0) {
        // Clarinet 3.23's proxy on 20445 does not forward /wallet/<name>.
        // stacks-node 4.0.5 lists miner UTXOs there and will not finish startup
        // until that call succeeds. The event observer on host:20445 stays put.
        let (rewritten, rpc_changed) = point_burnchain_rpc_at_bitcoind(&updated);
        changed |= rpc_changed;
        rewritten
    } else {
        updated
    };
    if changed {
        fs::write(path, updated).await?;
    }
    Ok(removed_rejected)
}

/// Clarinet 3.23 proxies Bitcoin RPC on port 20445 and does not forward `/wallet/<name>`.
/// stacks-node 4.0.5 calls that path, so the miner talks to bitcoind's published RPC instead.
fn point_burnchain_rpc_at_bitcoind(raw: &str) -> (String, bool) {
    let mut changed = false;
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim() == "rpc_port = 20445" {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent}rpc_port = 18443"));
            changed = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !changed {
        return (raw.to_string(), false);
    }
    let mut updated = out.join("\n");
    if raw.ends_with('\n') && !updated.ends_with('\n') {
        updated.push('\n');
    }
    (updated, true)
}

/// Clarinet's own miner runs every 45s once deployment finishes. Wait slightly
/// longer than that before adding a block, so the two miners do not both fire.
const BITCOIN_NUDGE_AFTER: Duration = Duration::from_secs(48);
const BITCOIN_NUDGE_POLL: Duration = Duration::from_secs(5);

fn nudge_bitcoin_if_chain_is_quiet(
    observed_height: &mut Option<u64>,
    height_since: &mut Instant,
    miner_address: &mut Option<String>,
) {
    if !stacks_node_accepts_connections() {
        return;
    }
    let Some(container) = bitcoin_container_name() else {
        return;
    };
    let Some(height) = bitcoin_block_count(&container) else {
        return;
    };
    if *observed_height != Some(height) {
        *observed_height = Some(height);
        *height_since = Instant::now();
        return;
    }
    if height_since.elapsed() < BITCOIN_NUDGE_AFTER {
        return;
    }
    if miner_address.is_none() {
        *miner_address = bitcoin_miner_address(&container);
    }
    let Some(address) = miner_address.clone() else {
        return;
    };
    if generate_to_miner(&container, &address) {
        *height_since = Instant::now();
    }
}

/// The epoch-4 snapshot has no PoX-5 stake for the next reward cycle. Clarinet
/// does not submit one on a restored chain, and stacks-node then cannot mine
/// the anchor block. Submit the devnet wallets' stake before the prepare phase.
fn spawn_pox5_stake() -> bool {
    let frontend = Path::new("frontend");
    if !frontend
        .join("node_modules/@stacks/transactions/package.json")
        .is_file()
    {
        return false;
    }
    let script_path = std::env::temp_dir().join("stacksdapp-pox5-stake.mjs");
    if std::fs::write(&script_path, include_str!("../pox5_stake.mjs")).is_err() {
        return false;
    }
    let log_path = std::env::temp_dir().join("stacksdapp-pox5-stake.log");
    let Ok(log) = std::fs::File::create(&log_path) else {
        return false;
    };
    let Ok(log_err) = log.try_clone() else {
        return false;
    };
    let frontend_dir = frontend
        .canonicalize()
        .unwrap_or_else(|_| frontend.to_path_buf());
    std::process::Command::new("node")
        .arg(&script_path)
        .env("STACKSDAPP_FRONTEND", &frontend_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .is_ok()
}

fn stacks_node_accepts_connections() -> bool {
    let Ok(addr) = "127.0.0.1:20443".parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// Drop burnchain fields stacks-node 4.0.5 rejects.
/// An empty `wallet_name` is also filled: 4.0.5 refuses to mine without one.
/// `pox_5_*` lines are kept; 4.0.5 still reads them.
/// `(updated, changed, removed_rejected_field)`.
pub fn strip_pox5_node_fields(raw: &str) -> (String, bool, bool) {
    let mut out = Vec::new();
    let mut removed_rejected = false;
    let mut changed = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if stacks_node_405_rejects(trimmed) {
            removed_rejected = true;
            changed = true;
            continue;
        }
        if trimmed == "wallet_name = \"\"" {
            let indent = &line[..line.len() - trimmed.len()];
            out.push(format!("{indent}wallet_name = \"miner\""));
            changed = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !changed {
        return (raw.to_string(), false, false);
    }
    let mut updated = out.join("\n");
    if raw.ends_with('\n') && !updated.ends_with('\n') {
        updated.push('\n');
    }
    (updated, true, removed_rejected)
}

/// Keys removed from stacks-node config in 4.0.5. An unknown field aborts startup.
fn stacks_node_405_rejects(trimmed: &str) -> bool {
    const KEYS: &[&str] = &[
        "rpc_ssl",
        "commit_anchor_block_within",
        "max_microblocks",
        "chain_liveness_poll_time_secs",
        "max_neighbors_per_host",
        "max_clients_per_host",
        "soft_max_neighbors_per_host",
        "full_inv_sync_interval",
    ];
    KEYS.iter().any(|key| {
        trimmed == *key
            || trimmed.starts_with(&format!("{key} "))
            || trimmed.starts_with(&format!("{key}="))
    })
}

/// stacks-node 4.0.5 mines through a named Bitcoin wallet. Clarinet 3.23 leaves
/// that wallet unset; create the same watch-only `miner` wallet Clarinet 3.26 creates.
fn ensure_bitcoin_miner_wallet() -> bool {
    let Some(name) = bitcoin_container_name() else {
        return false;
    };
    if bitcoin_cli(&name, &["createwallet", "miner", "true"]).is_some() {
        return true;
    }
    bitcoin_cli(&name, &["loadwallet", "miner"]).is_some()
}

fn bitcoin_container_name() -> Option<String> {
    let output = std::process::Command::new("docker")
        .args(["ps", "--format", "{{.Names}}", "--filter", "name=devnet"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|name| name.contains("bitcoin-node"))
        .map(str::to_string)
}

fn bitcoin_cli(container: &str, args: &[&str]) -> Option<String> {
    let mut cmd = vec![
        "exec",
        container,
        "bitcoin-cli",
        "-regtest",
        "-rpcuser=devnet",
        "-rpcpassword=devnet",
    ];
    cmd.extend_from_slice(args);
    let output = std::process::Command::new("docker")
        .args(&cmd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_string())
}

fn bitcoin_block_count(container: &str) -> Option<u64> {
    bitcoin_cli(container, &["getblockcount"])?.parse().ok()
}

fn bitcoin_miner_address(container: &str) -> Option<String> {
    let hash = bitcoin_cli(container, &["getbestblockhash"])?;
    let block = bitcoin_cli(container, &["getblock", &hash, "2"])?;
    coinbase_address_from_block(&block)
}

fn generate_to_miner(container: &str, address: &str) -> bool {
    bitcoin_cli(container, &["generatetoaddress", "1", address]).is_some()
}

/// Coinbase payout of the latest regtest block. Clarinet mines to this address.
pub fn coinbase_address_from_block(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let txs = value.get("tx")?.as_array()?;
    let coinbase = txs.iter().find(|tx| {
        tx.get("vin")
            .and_then(|vin| vin.as_array())
            .map(|vin| vin.iter().any(|input| input.get("coinbase").is_some()))
            .unwrap_or(false)
    })?;
    coinbase.get("vout")?.as_array()?.iter().find_map(|output| {
        output
            .pointer("/scriptPubKey/address")
            .and_then(|address| address.as_str())
            .map(str::to_string)
    })
}

fn attach_next_output(child: &mut Child, ready: Option<oneshot::Sender<String>>) {
    let ready = std::sync::Arc::new(tokio::sync::Mutex::new(ready));
    if let Some(stdout) = child.stdout.take() {
        spawn_next_stream(stdout, std::sync::Arc::clone(&ready));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_next_stream(stderr, ready);
    }
}

fn spawn_next_stream<R>(
    reader: R,
    ready: std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        let mut local_url = String::from("http://localhost:3000");
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = extract_local_url(&line) {
                local_url = url;
            }

            if is_next_ready_line(&line) {
                let mut slot = ready.lock().await;
                if let Some(tx) = slot.take() {
                    let _ = tx.send(local_url.clone());
                }
                continue;
            }

            // Don't spam startup banner lines before Ready.
            {
                let slot = ready.lock().await;
                if slot.is_some() && should_suppress_next_startup(&line) {
                    continue;
                }
            }

            if let Some(formatted) = format_next_line(&line) {
                stacksdapp_shell::println_human_safe(formatted);
            }
        }
    });
}

fn extract_local_url(line: &str) -> Option<String> {
    // "- Local:        http://localhost:3000" or "Local: http://127.0.0.1:3000"
    let lower = line.to_ascii_lowercase();
    if !lower.contains("local:") && !lower.contains("localhost:") && !lower.contains("127.0.0.1:") {
        return None;
    }
    for part in line.split_whitespace() {
        if part.starts_with("http://") || part.starts_with("https://") {
            return Some(part.trim_end_matches(',').to_string());
        }
    }
    None
}

fn is_next_ready_line(line: &str) -> bool {
    let t = line.trim();
    t.contains("Ready in")
        || t.contains("started server on")
        || t.contains("✓ Ready")
        || t.contains("✔ Ready")
}

fn should_suppress_next_startup(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    t.starts_with("▲")
        || t.contains("Next.js")
        || t.contains("Local:")
        || t.contains("Network:")
        || t.contains("Environments:")
        || t.contains("Experiments")
        || t.starts_with("- ")
}

fn format_next_line(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    if should_suppress_next_startup(t) {
        return None;
    }

    let ts = timestamp_now();

    // ✓ Compiled / Ready / Rebuilt in 248ms
    if t.contains("Compiled")
        || t.contains("compiled")
        || t.contains("Rebuilt")
        || t.contains("rebuilt")
    {
        if let Some(dur) = extract_duration_fragment(t) {
            return Some(format!(
                "[{ts}] {} {}",
                "✓".truecolor(52, 211, 153),
                format!("Rebuilt in {dur}").white()
            ));
        }
    }

    // GET /path 200  (and similar access logs)
    if let Some(formatted) = format_http_access(t) {
        return Some(format!("[{ts}] {formatted}"));
    }

    // Errors / warnings — keep visible
    let lower = t.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("warn") || lower.contains("failed") {
        return Some(format!("[{ts}] {t}"));
    }

    // Fast Refresh notices
    if t.contains("Fast Refresh") || t.contains("hot reloaded") {
        return Some(format!(
            "[{ts}] {} {}",
            "✓".truecolor(52, 211, 153),
            "Hot reloaded".white()
        ));
    }

    None
}

fn extract_duration_fragment(line: &str) -> Option<&str> {
    // "in 248ms" or "in 1.2s"
    let lower = line.to_ascii_lowercase();
    let idx = lower.find(" in ")?;
    let rest = line[idx + 4..].trim();
    let end = rest
        .find(|c: char| c.is_whitespace() || c == ')' || c == ',')
        .unwrap_or(rest.len());
    let dur = rest[..end].trim();
    if dur.ends_with("ms") || dur.ends_with('s') {
        Some(dur)
    } else {
        None
    }
}

fn format_http_access(line: &str) -> Option<String> {
    // Examples: "GET / 200", "○ GET /counter 200 in 12ms"
    let parts: Vec<&str> = line.split_whitespace().collect();
    let method_idx = parts.iter().position(|p| {
        matches!(
            *p,
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
        )
    })?;
    let method = parts[method_idx];
    let path = parts.get(method_idx + 1)?;
    let status = parts
        .iter()
        .skip(method_idx + 2)
        .find(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() == 3)?;

    Some(format!(
        "{}  {:<20} {}",
        method.truecolor(156, 163, 175),
        path.white(),
        status.truecolor(52, 211, 153)
    ))
}

fn timestamp_now() -> String {
    // Local HH:MM:SS without extra crates.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Approximate local time via UTC offset is hard without chrono; use UTC clock for stability.
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    format!("{hours:02}:{mins:02}:{s:02}")
}

async fn write_network_env(network: &str) -> Result<()> {
    let env_path = Path::new("frontend/.env.local");
    let existing = fs::read_to_string(env_path).await.unwrap_or_default();
    let content = upsert_env_assignment(
        &existing,
        "NEXT_PUBLIC_NETWORK",
        network,
        &format!("# Auto-written by stacksdapp dev --network {network}"),
    );
    fs::write(env_path, content).await?;
    Ok(())
}

fn upsert_env_assignment(existing: &str, key: &str, value: &str, header: &str) -> String {
    let mut kept = Vec::new();
    let mut replaced = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("{key}=")) {
            if !replaced {
                kept.push(format!("{key}={value}"));
                replaced = true;
            }
            continue;
        }
        if line.trim() == header {
            continue;
        }
        kept.push(line.to_string());
    }

    if !replaced {
        let mut out = vec![header.to_string(), format!("{key}={value}")];
        if !kept.is_empty() {
            out.push(String::new());
        }
        out.extend(kept);
        return out.join("\n") + "\n";
    }

    kept.join("\n") + "\n"
}

fn check_deployments(network: &str) {
    let path = Path::new("frontend/src/generated/deployments.json");
    if !path.exists() {
        stacksdapp_shell::warn(format!(
            "deployments.json not found — run `stacksdapp deploy --network {network}` so the frontend knows contract addresses."
        ));
        return;
    }

    let raw = std::fs::read_to_string(path).unwrap_or_default();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
        let deployed_network = json["network"].as_str().unwrap_or("");
        if deployed_network != network && !deployed_network.is_empty() {
            stacksdapp_shell::warn(format!(
                "deployments.json is for '{deployed_network}' but you requested '{network}'. Run `stacksdapp deploy --network {network}` first."
            ));
        }
        let contracts = json["contracts"].as_object();
        if contracts.map(|c| c.is_empty()).unwrap_or(true) {
            stacksdapp_shell::warn(format!(
                "No contracts in deployments.json — run `stacksdapp deploy --network {network}` to populate it."
            ));
        }
    }
}

fn ensure_docker() -> Result<()> {
    if which::which("docker").is_err() {
        return Err(anyhow!(
            "Docker is required for devnet. Install from https://docker.com"
        ));
    }
    let running = std::process::Command::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !running {
        return Err(anyhow!(
            "Docker is not running. Start Docker Desktop and try again."
        ));
    }
    Ok(())
}

/// Containers for this project's local devnet, if Clarinet has them running.
pub fn running_project_devnet_containers() -> Vec<String> {
    let Some(project) = clarinet_project_name(Path::new("contracts/Clarinet.toml")) else {
        return Vec::new();
    };
    let network_name = devnet_network_name(&project, Path::new("contracts/settings/Devnet.toml"));
    running_devnet_containers(&network_name).unwrap_or_default()
}

/// Restore the running local devnet to Clarinet's snapshot so the next deploy
/// can publish the current contract name. The devnet process stays up.
///
/// Returns the on-disk session directory that must be kept so the containers
/// still have their config and chain mounts. `Ok(None)` when no devnet is running.
pub async fn reset_running_devnet_chain() -> Result<Option<std::path::PathBuf>> {
    if which::which("docker").is_err() {
        return Ok(None);
    }
    let project = clarinet_project_name(Path::new("contracts/Clarinet.toml"))
        .ok_or_else(|| anyhow!("Could not read the project name from contracts/Clarinet.toml"))?;
    let network_name = devnet_network_name(&project, Path::new("contracts/settings/Devnet.toml"));
    let names = running_devnet_containers(&network_name)?;
    if names.is_empty() {
        return Ok(None);
    }

    let snapshot = select_snapshot_dir(&snapshot_cache_root()).ok_or_else(|| {
        anyhow!(
            "No Clarinet devnet snapshot in ~/.clarinet/cache/devnet. Start `stacksdapp dev` once so Clarinet can download it, then run `stacksdapp clean` again."
        )
    })?;
    let stacks_snapshot = snapshot.join("stacks");
    let bitcoin_snapshot = snapshot.join("bitcoin").join("regtest");
    if !stacks_snapshot.join("krypton").is_dir() || !bitcoin_snapshot.is_dir() {
        return Err(anyhow!(
            "The Clarinet devnet snapshot at {} is incomplete.",
            snapshot.display()
        ));
    }

    stacksdapp_shell::println_human_safe(
        "[clean] Resetting the running devnet chain to the snapshot. The devnet process stays up.",
    );

    let stacks = names.iter().find(|name| name.starts_with("stacks-node."));
    let bitcoin = names.iter().find(|name| name.starts_with("bitcoin-node."));
    let stacks_mount = stacks
        .as_ref()
        .and_then(|name| bind_mount(name, is_stacks_dest));
    let bitcoin_mount = bitcoin
        .as_ref()
        .and_then(|name| bind_mount(name, is_bitcoin_dest));

    if stacks.is_none() && stacks_mount.is_none() {
        return Err(anyhow!(
            "The devnet stacks-node container is not running, so the chain could not be reset."
        ));
    }

    let pause_targets: Vec<&str> = names
        .iter()
        .filter(|name| {
            name.starts_with("stacks-node.")
                || name.starts_with("bitcoin-node.")
                || name.starts_with("stacks-signer-")
        })
        .map(String::as_str)
        .collect();
    let host_mounts = stacks_mount.is_some() || bitcoin_mount.is_some();
    if host_mounts && !pause_targets.is_empty() {
        let mut args = vec!["pause"];
        args.extend(pause_targets.iter().copied());
        let _ = docker_status(&args);
    }

    let replaced = (|| -> Result<()> {
        if let Some(dest) = &stacks_mount {
            reset_directory_from_snapshot(dest, &stacks_snapshot)?;
        }
        if let Some(dest) = &bitcoin_mount {
            reset_directory_from_snapshot(&dest.join("regtest"), &bitcoin_snapshot)?;
        }
        Ok(())
    })();
    if host_mounts && !pause_targets.is_empty() {
        let mut args = vec!["unpause"];
        args.extend(pause_targets.iter().copied());
        let _ = docker_status(&args);
    }
    replaced?;

    if stacks_mount.is_none() {
        if let Some(container) = stacks {
            reset_container_dir(container, "/devnet", &stacks_snapshot)?;
        }
    }
    if bitcoin_mount.is_none() {
        if let Some(container) = bitcoin {
            reset_container_dir(
                container,
                "/home/bitcoin/.bitcoin/regtest",
                &bitcoin_snapshot,
            )?;
        }
    }

    let session = stacks_mount
        .as_ref()
        .and_then(|path| devnet_session_dir(path));

    // Restart bitcoin and stacks-node first. The signer is restarted only after
    // the node is serving, with an empty database, so it adopts the snapshot
    // tenure. Restarting them together leaves the signer on a newer Bitcoin
    // tenure and it rejects every block (SortitionViewMismatch).
    restart_containers(&names, &["bitcoin-node.", "stacks-node."])?;
    let port = stacks_rpc_port(Path::new("contracts/settings/Devnet.toml"));
    wait_for_stacks_rpc(port).await?;
    wipe_signer_state(&names, session.as_deref());
    restart_containers(&names, &["stacks-signer-"])?;
    wait_for_stacks_tip_advance(port, &names, session.as_deref()).await?;
    stacksdapp_shell::println_human_safe(
        "[clean] Devnet is mining again. This project's contracts are no longer on the chain.",
    );
    Ok(session)
}

/// Delete `path`, but keep `live_session` when it lives inside that directory.
pub fn remove_dev_cache(path: &Path, live_session: Option<&Path>) -> std::io::Result<()> {
    let Some(session) = live_session else {
        return std::fs::remove_dir_all(path);
    };
    if !dev_cache_holds_session(path, session) {
        return std::fs::remove_dir_all(path);
    }
    let path_abs = path.canonicalize()?;
    let session_abs = session
        .canonicalize()
        .unwrap_or_else(|_| session.to_path_buf());
    remove_except(&path_abs, &session_abs)
}

pub fn dev_cache_holds_session(path: &Path, session: &Path) -> bool {
    let Some(path_abs) = path.canonicalize().ok() else {
        return false;
    };
    let session_abs = session
        .canonicalize()
        .unwrap_or_else(|_| session.to_path_buf());
    session_abs.starts_with(path_abs)
}

fn clarinet_project_name(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    assignment_in_table(&raw, "project", "name")
}

fn devnet_network_name(project: &str, devnet_toml: &Path) -> String {
    let raw = std::fs::read_to_string(devnet_toml).unwrap_or_default();
    let network =
        assignment_in_table(&raw, "network", "name").unwrap_or_else(|| "devnet".to_string());
    match assignment_in_table(&raw, "devnet", "network_id") {
        Some(id) => format!("{project}.{id}.{network}"),
        None => format!("{project}.{network}"),
    }
}

fn assignment_in_table(raw: &str, table: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current = header.trim();
            continue;
        }
        if current != table {
            continue;
        }
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim();
        if !rest.starts_with('=') {
            continue;
        }
        let value = rest
            .trim_start_matches('=')
            .trim()
            .trim_matches('"')
            .trim_matches('\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn stacks_rpc_port(path: &Path) -> u16 {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return 20443;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("stacks_node_rpc_port") else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('=').trim();
        if let Ok(port) = rest.parse::<u16>() {
            return port;
        }
    }
    20443
}

fn snapshot_cache_root() -> std::path::PathBuf {
    dirs_home().join(".clarinet").join("cache").join("devnet")
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
}

fn select_snapshot_dir(root: &Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut found: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| path.join("stacks").join("krypton").is_dir())
        .filter(|path| path.join("bitcoin").join("regtest").is_dir())
        .collect();
    found.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    found.pop()
}

fn running_devnet_containers(network_name: &str) -> Result<Vec<String>> {
    let output = std::process::Command::new("docker")
        .args(["ps", "--format", "{{.Names}}"])
        .output()
        .map_err(|e| anyhow!("Could not list Docker containers: {e}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|name| container_belongs_to_devnet(name, network_name))
        .map(str::to_string)
        .collect())
}

fn container_belongs_to_devnet(name: &str, network_name: &str) -> bool {
    let suffix = format!(".{network_name}");
    if !name.ends_with(&suffix) {
        return false;
    }
    name.starts_with("stacks-node.")
        || name.starts_with("bitcoin-node.")
        || name.starts_with("stacks-signer-")
}

fn bind_mount(container: &str, dest_matches: fn(&str) -> bool) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("docker")
        .args(["inspect", "--format", "{{json .Mounts}}", container])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mounts: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).ok()?;
    mounts.iter().find_map(|mount| {
        if mount.get("Type").and_then(|v| v.as_str()) != Some("bind") {
            return None;
        }
        let dest = mount.get("Destination").and_then(|v| v.as_str())?;
        if !dest_matches(dest) {
            return None;
        }
        let source = mount.get("Source").and_then(|v| v.as_str())?;
        Some(host_bind_path(source))
    })
}

/// Docker Desktop reports some bind sources as `/host_mnt/Users/...`, which is
/// not a path on the Mac. The files are at `/Users/...`.
fn host_bind_path(source: &str) -> std::path::PathBuf {
    let raw = std::path::PathBuf::from(source);
    if raw.exists() {
        return raw;
    }
    if let Some(stripped) = source.strip_prefix("/host_mnt") {
        let host = std::path::PathBuf::from(stripped);
        if host.exists() {
            return host;
        }
    }
    raw
}

fn is_stacks_dest(dest: &str) -> bool {
    dest.trim_end_matches('/') == "/devnet"
}

fn is_bitcoin_dest(dest: &str) -> bool {
    let dest = dest.trim_end_matches('/');
    dest.ends_with(".bitcoin") || dest.ends_with("/bitcoin")
}

fn devnet_session_dir(stacks_data: &Path) -> Option<std::path::PathBuf> {
    let boot = stacks_data.parent()?;
    let data = boot.parent()?;
    if data.file_name()? != "data" {
        return None;
    }
    data.parent().map(std::path::Path::to_path_buf)
}

fn reset_directory_from_snapshot(dest: &Path, snapshot: &Path) -> Result<()> {
    if !snapshot.is_dir() {
        return Err(anyhow!(
            "Snapshot directory {} is missing.",
            snapshot.display()
        ));
    }
    std::fs::create_dir_all(dest)?;
    clear_dir_children(dest)?;
    copy_dir_contents(snapshot, dest)?;
    Ok(())
}

fn clear_dir_children(dir: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).map_err(|e| anyhow!("Could not read {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| anyhow!("Could not read {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| anyhow!("Could not remove {}: {e}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| anyhow!("Could not remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(src).map_err(|e| anyhow!("Could not read {}: {e}", src.display()))?
    {
        let entry = entry.map_err(|e| anyhow!("Could not read {}: {e}", src.display()))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_contents(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| {
                anyhow!("Could not copy {} to {}: {e}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

fn reset_container_dir(container: &str, dest: &str, snapshot: &Path) -> Result<()> {
    let script = format!("rm -rf {dest} && mkdir -p {dest}");
    docker_must(
        &["exec", container, "sh", "-c", &script],
        "clear chain data in the devnet container",
    )?;
    let src = format!("{}/.", snapshot.display());
    let target = format!("{container}:{dest}");
    docker_must(
        &["cp", &src, &target],
        "copy the snapshot into the devnet container",
    )?;
    Ok(())
}

fn wipe_signer_state(names: &[String], session: Option<&Path>) {
    for name in names
        .iter()
        .filter(|name| name.starts_with("stacks-signer-"))
    {
        let _ = docker_status(&[
            "exec",
            name,
            "sh",
            "-c",
            "rm -f stacks-signer-*.sqlite stacks-signer-*.sqlite-shm stacks-signer-*.sqlite-wal /stacks-signer-*.sqlite /stacks-signer-*.sqlite-shm /stacks-signer-*.sqlite-wal",
        ]);
    }
    let Some(session) = session else {
        return;
    };
    let Ok(boots) = std::fs::read_dir(session.join("data")) else {
        return;
    };
    for boot in boots.flatten() {
        let signer = boot.path().join("signer");
        if signer.is_dir() {
            let _ = std::fs::remove_dir_all(&signer);
            let _ = std::fs::create_dir_all(&signer);
        }
    }
}

fn remove_except(dir: &Path, keep: &Path) -> std::io::Result<()> {
    if dir == keep {
        return Ok(());
    }
    if !keep.starts_with(dir) {
        return std::fs::remove_dir_all(dir);
    }
    for entry in std::fs::read_dir(dir)? {
        let child = entry?.path();
        if child == keep {
            continue;
        }
        if keep.starts_with(&child) {
            if child.is_dir() {
                remove_except(&child, keep)?;
            }
            continue;
        }
        if child.is_dir() {
            std::fs::remove_dir_all(&child)?;
        } else {
            std::fs::remove_file(&child)?;
        }
    }
    Ok(())
}

fn docker_status(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("docker")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow!("docker {} failed to start: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("docker {} failed", args.join(" ")))
    }
}

fn restart_containers(names: &[String], prefixes: &[&str]) -> Result<()> {
    let mut args = vec!["restart".to_string(), "-t".to_string(), "30".to_string()];
    for prefix in prefixes {
        for name in names {
            if name.starts_with(prefix) {
                args.push(name.clone());
            }
        }
    }
    if args.len() == 3 {
        return Ok(());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    docker_must(&borrowed, "restart devnet containers")
}

fn docker_must(args: &[&str], action: &str) -> Result<()> {
    let output = std::process::Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| anyhow!("Could not {action}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(anyhow!("Could not {action}: {}", stderr.trim()))
}

async fn wait_for_stacks_rpc(port: u16) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| anyhow!("Could not build an HTTP client: {e}"))?;
    let url = format!("http://127.0.0.1:{port}/v2/info");
    let started = Instant::now();
    let mut announced = false;
    loop {
        if started.elapsed() > Duration::from_secs(180) {
            return Err(anyhow!(
                "The devnet chain was reset, but stacks-node did not answer on port {port} within 180s."
            ));
        }
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
        if !announced && started.elapsed() > Duration::from_secs(5) {
            stacksdapp_shell::println_human_safe(
                "[clean] Waiting for the devnet node to come back...",
            );
            announced = true;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn wait_for_stacks_tip_advance(
    port: u16,
    names: &[String],
    session: Option<&Path>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| anyhow!("Could not build an HTTP client: {e}"))?;
    let url = format!("http://127.0.0.1:{port}/v2/info");
    let start = stacks_tip_height(&client, &url).await?;
    let started = Instant::now();
    let mut healed = false;
    loop {
        if let Ok(tip) = stacks_tip_height(&client, &url).await {
            if tip > start {
                stacksdapp_shell::println_human_safe(format!(
                    "[clean] Stacks tip advanced to {tip}."
                ));
                return Ok(());
            }
        }
        if !healed && started.elapsed() > Duration::from_secs(35) {
            stacksdapp_shell::println_human_safe(
                "[clean] Stacks tip has not moved. Resetting the signer so it can accept blocks.",
            );
            wipe_signer_state(names, session);
            restart_containers(names, &["stacks-signer-"])?;
            healed = true;
        }
        if started.elapsed() > Duration::from_secs(120) {
            return Err(anyhow!(
                "The devnet chain was reset, but Stacks tip stayed at {start}. The signer is not accepting blocks."
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn stacks_tip_height(client: &reqwest::Client, url: &str) -> Result<u64> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Could not read the devnet tip: {e}"))?;
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("Could not parse the devnet tip: {e}"))?;
    json.get("stacks_tip_height")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("The devnet tip response has no stacks_tip_height."))
}

/// Stop leftover Clarinet Devnet containers that hold ports (5432, 20445, 3999, …).
/// Safe for testnet/mainnet: only matches Docker names containing `devnet`.
pub fn stop_stale_devnet_docker() {
    let Ok(output) = std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=devnet"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let ids = String::from_utf8_lossy(&output.stdout);
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return;
    }
    stacksdapp_shell::println_human_safe(format!(
        "[devnet] Stopping {} leftover Devnet container(s) to free ports...",
        ids.len()
    ));
    let mut cmd = std::process::Command::new("docker");
    cmd.arg("stop").args(&ids);
    let _ = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Rewrite `contracts/settings/Devnet.toml` for Clarinet 3.23+ snapshot fast-boot.
/// Preserves accounts and other custom keys; ensures default PoX stacking orders
/// (required for the epoch-4.0 snapshot), explorers off, and faster block time.
/// Does not touch Testnet/Mainnet.
async fn ensure_devnet_fast_boot_settings() -> Result<()> {
    let path = Path::new("contracts/settings/Devnet.toml");
    if !path.is_file() {
        return Ok(());
    }
    let raw = fs::read_to_string(path).await?;
    // Clarinet 3.26+ already starts stacks-node 4.0.5. Older Clarinet defaults to 4.0.1,
    // whose signer halts at the first PoX reward-cycle boundary after the epoch 4.0 snapshot.
    let pin_stacks_405 = !stacksdapp_deployer::clarinet_version_at_least(3, 26, 0);
    let (updated, changed) = optimize_devnet_toml_for_fast_boot_with(&raw, pin_stacks_405);
    if changed {
        fs::write(path, updated).await?;
        let detail = if pin_stacks_405 {
            "snapshot-compatible PoX stacking; stacks-node 4.0.5; explorers off"
        } else {
            "snapshot-compatible PoX stacking; explorers off"
        };
        stacksdapp_shell::println_human_safe(format!(
            "[devnet] Applied fast-boot settings ({detail})."
        ));
    }
    Ok(())
}

/// Snapshot-compatible PoX stacking block (matches Clarinet 3.23 `DevnetConfig::default()`).
const SNAPSHOT_POX_STACKING_ORDERS: &str = r#"
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

const SNAPSHOT_WALLET_ACCOUNTS: &str = r#"
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
"#;

/// stacks-node / signer that can extend a tenure across a PoX reward-cycle boundary.
const STACKS_NODE_IMAGE_405: &str = "ghcr.io/stacks-network/stacks-core:4.0.5-alpine";
const STACKS_SIGNER_IMAGE_405: &str = "ghcr.io/stacks-network/stacks-signer:4.0.5-alpine";

/// Pure rewrite helper (unit-tested). Does not pin stacks images.
pub fn optimize_devnet_toml_for_fast_boot(raw: &str) -> (String, bool) {
    optimize_devnet_toml_for_fast_boot_with(raw, false)
}

/// When `pin_stacks_405` is set, Clarinet older than 3.26 is pointed at stacks-node 4.0.5.
/// A newer official tag, or any non-official image, is left as written.
pub fn optimize_devnet_toml_for_fast_boot_with(raw: &str, pin_stacks_405: bool) -> (String, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut in_devnet = false;
    let mut saw_devnet = false;
    let mut has_btc_explorer = false;
    let mut has_stacks_explorer = false;
    let mut has_stacks_api = false;
    let mut has_block_time = false;
    let mut has_node_image = false;
    let mut has_signer_image = false;

    for line in raw.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_devnet = trimmed == "[devnet]";
            if in_devnet {
                saw_devnet = true;
            }
        }

        if in_devnet {
            if trimmed.starts_with("disable_bitcoin_explorer") {
                has_btc_explorer = true;
                out.push("disable_bitcoin_explorer = true".to_string());
                continue;
            }
            if trimmed.starts_with("disable_stacks_explorer") {
                has_stacks_explorer = true;
                out.push("disable_stacks_explorer = true".to_string());
                continue;
            }
            if trimmed.starts_with("disable_stacks_api") {
                has_stacks_api = true;
                // Off by default: stacks-api + Clarinet snapshot often desync (blocks stall at ~38).
                out.push("disable_stacks_api = true".to_string());
                continue;
            }
            if trimmed.starts_with("bitcoin_controller_block_time") {
                has_block_time = true;
                out.push("bitcoin_controller_block_time = 45_000".to_string());
                continue;
            }
            if pin_stacks_405 {
                if let Some(line) = rewrite_stacks_image_line(
                    trimmed,
                    "stacks_node_image_url",
                    STACKS_NODE_IMAGE_405,
                ) {
                    has_node_image = true;
                    out.push(line);
                    continue;
                }
                if let Some(line) = rewrite_stacks_image_line(
                    trimmed,
                    "stacks_signer_image_url",
                    STACKS_SIGNER_IMAGE_405,
                ) {
                    has_signer_image = true;
                    out.push(line);
                    continue;
                }
            }
        }

        out.push(line.to_string());
    }

    let mut injected = false;
    if saw_devnet {
        // Inject any missing keys just after the `[devnet]` header.
        let mut final_out: Vec<String> = Vec::with_capacity(out.len() + 4);
        for line in out {
            final_out.push(line.clone());
            if line.trim() == "[devnet]" {
                if !has_btc_explorer {
                    final_out.push("disable_bitcoin_explorer = true".to_string());
                    injected = true;
                }
                if !has_stacks_explorer {
                    final_out.push("disable_stacks_explorer = true".to_string());
                    injected = true;
                }
                if !has_stacks_api {
                    final_out.push("disable_stacks_api = true".to_string());
                    injected = true;
                }
                if !has_block_time {
                    final_out.push("bitcoin_controller_block_time = 45_000".to_string());
                    injected = true;
                }
                if pin_stacks_405 && !has_node_image {
                    final_out.push(format!(
                        "stacks_node_image_url = \"{STACKS_NODE_IMAGE_405}\""
                    ));
                    injected = true;
                }
                if pin_stacks_405 && !has_signer_image {
                    final_out.push(format!(
                        "stacks_signer_image_url = \"{STACKS_SIGNER_IMAGE_405}\""
                    ));
                    injected = true;
                }
            }
        }
        out = final_out;
    } else {
        out.push(String::new());
        out.push("[devnet]".to_string());
        out.push("disable_bitcoin_explorer = true".to_string());
        out.push("disable_stacks_explorer = true".to_string());
        out.push("disable_stacks_api = true".to_string());
        out.push("bitcoin_controller_block_time = 45_000".to_string());
        if pin_stacks_405 {
            out.push(format!(
                "stacks_node_image_url = \"{STACKS_NODE_IMAGE_405}\""
            ));
            out.push(format!(
                "stacks_signer_image_url = \"{STACKS_SIGNER_IMAGE_405}\""
            ));
        }
        injected = true;
    }

    let mut updated = out.join("\n");
    if !updated.ends_with('\n') {
        updated.push('\n');
    }

    let needs_wallets = !updated.contains("[accounts.wallet_1]");
    if !snapshot_pox_orders_complete(&updated) {
        updated = strip_pox_stacking_order_blocks(&updated);
        updated.push_str(SNAPSHOT_POX_STACKING_ORDERS);
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        injected = true;
    }
    if needs_wallets {
        if let Some(devnet_pos) = updated.find("\n[devnet]") {
            updated.insert_str(devnet_pos, SNAPSHOT_WALLET_ACCOUNTS);
        } else {
            updated.push_str(SNAPSHOT_WALLET_ACCOUNTS);
        }
        injected = true;
    }

    let normalized_raw = if raw.ends_with('\n') {
        raw.to_string()
    } else {
        format!("{raw}\n")
    };
    let changed = injected || updated != normalized_raw;
    (updated, changed)
}

fn rewrite_stacks_image_line(trimmed: &str, key: &str, pinned: &str) -> Option<String> {
    let rest = trimmed.strip_prefix(key)?;
    if !(rest.starts_with(' ') || rest.starts_with('=')) {
        return None;
    }
    let value = trimmed
        .split_once('=')
        .map(|(_, value)| value.trim().trim_matches('"'))
        .unwrap_or("");
    if official_stacks_image_older_than_405(value) {
        Some(format!("{key} = \"{pinned}\""))
    } else {
        Some(trimmed.to_string())
    }
}

fn official_stacks_image_older_than_405(url: &str) -> bool {
    if !url.starts_with("ghcr.io/stacks-network/stacks-") {
        return false;
    }
    let Some(tag) = url.rsplit(':').next() else {
        return false;
    };
    let tag = tag.strip_suffix("-alpine").unwrap_or(tag);
    let mut parts = tag.split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(minor) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(patch) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    (major, minor, patch) < (4, 0, 5)
}

fn snapshot_pox_orders_complete(raw: &str) -> bool {
    [
        "mr1iPkD9N3RJZZxXRk7xF9d36gffa6exNC",
        "muYdXKmX9bByAueDe6KFfHd5Ff1gdN9ErG",
        "mvZtbibDAAA3WLpY7zXXFqRa3T4XSknBX7",
    ]
    .iter()
    .all(|addr| raw.contains(addr))
}

fn strip_pox_stacking_order_blocks(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut skipping_pox = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping_pox = trimmed == "[[devnet.pox_stacking_orders]]";
        }
        if skipping_pox {
            continue;
        }
        out.push(line.to_string());
    }
    let mut updated = out.join("\n");
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

fn spawn_next_dev_process(network: &str) -> Result<Child> {
    let frontend_dir = Path::new("frontend");
    let next_bin = frontend_dir.join("node_modules/next/dist/bin/next");
    let child = if next_bin.exists() {
        Command::new("node")
            .arg("node_modules/next/dist/bin/next")
            .arg("dev")
            .current_dir(frontend_dir)
            .env("NEXT_PUBLIC_NETWORK", network)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
    } else {
        Command::new("npm")
            .args(["run", "dev"])
            .current_dir(frontend_dir)
            .env("NEXT_PUBLIC_NETWORK", network)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
    };
    Ok(child)
}

async fn shutdown_child(name: &str, child: &mut Child) {
    if child.id().is_none() {
        return;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    stacksdapp_shell::debug(1, format!("Stopped {name}."));
}

#[cfg(test)]
mod tests {
    use super::{
        coinbase_address_from_block, optimize_devnet_toml_for_fast_boot,
        optimize_devnet_toml_for_fast_boot_with, strip_pox5_node_fields, upsert_env_assignment,
    };

    #[test]
    fn strip_pox5_node_fields_keeps_devnet_sbtc_contracts() {
        let raw = r#"[node]
working_dir = "/devnet"
rpc_bind = "0.0.0.0:20443"
pox_5_sbtc_contract = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.sbtc-token"
pox_5_sbtc_registry_contract = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.sbtc-registry"
pox_5_bond_admin = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM"

[connection_options]
auth_token = "12345"
"#;
        let (updated, changed, rejected) = strip_pox5_node_fields(raw);
        assert!(!changed);
        assert!(!rejected);
        assert!(updated.contains("pox_5_sbtc_registry_contract"));
        assert!(updated.contains("rpc_bind = \"0.0.0.0:20443\""));
        assert!(updated.contains("auth_token = \"12345\""));
    }

    #[test]
    fn strip_pox5_node_fields_noop_when_absent() {
        let raw = "[node]\nrpc_bind = \"0.0.0.0:20443\"\n";
        let (updated, changed, rejected) = strip_pox5_node_fields(raw);
        assert!(!changed);
        assert!(!rejected);
        assert_eq!(updated, raw);
    }

    #[test]
    fn strip_pox5_node_fields_drops_keys_stacks_node_405_rejects() {
        let raw = "[burnchain]\nmode = \"krypton\"\nrpc_ssl = false\nusername = \"devnet\"\n";
        let (updated, changed, rejected) = strip_pox5_node_fields(raw);
        assert!(changed);
        assert!(rejected);
        assert!(!updated.contains("rpc_ssl"));
        assert!(updated.contains("mode = \"krypton\""));
        assert!(updated.contains("username = \"devnet\""));
    }

    #[test]
    fn coinbase_address_from_block_reads_miner_payout() {
        let raw = r#"{"tx":[{"vin":[{"coinbase":"00"}],"vout":[{"value":50,"scriptPubKey":{"address":"mr1iPkD9N3RJZZxXRk7xF9d36gffa6exNC"}}]}]}"#;
        assert_eq!(
            coinbase_address_from_block(raw).as_deref(),
            Some("mr1iPkD9N3RJZZxXRk7xF9d36gffa6exNC")
        );
    }

    #[test]
    fn burnchain_rpc_targets_bitcoind_when_proxy_cannot_serve_wallets() {
        let raw = "[burnchain]\nrpc_port = 20445\npeer_port = 18444\nendpoint = \"host.docker.internal:20445\"\n";
        let (updated, changed) = super::point_burnchain_rpc_at_bitcoind(raw);
        assert!(changed);
        assert!(updated.contains("rpc_port = 18443"));
        assert!(updated.contains("host.docker.internal:20445"));
        assert!(!updated.contains("rpc_port = 20445"));
    }

    #[test]
    fn strip_pox5_node_fields_sets_miner_wallet_name() {
        let raw = "[burnchain]\nmode = \"krypton\"\nwallet_name = \"\"\nusername = \"devnet\"\n";
        let (updated, changed, rejected) = strip_pox5_node_fields(raw);
        assert!(changed);
        assert!(!rejected);
        assert!(updated.contains("wallet_name = \"miner\""));
        assert!(!updated.contains("wallet_name = \"\""));
    }

    #[test]
    fn upsert_env_assignment_preserves_other_values() {
        let existing = "# existing\nFOO=bar\nNEXT_PUBLIC_NETWORK=testnet\nAPI_KEY=secret\n";
        let updated = upsert_env_assignment(
            existing,
            "NEXT_PUBLIC_NETWORK",
            "mainnet",
            "# Auto-written by stacksdapp dev --network mainnet",
        );
        assert!(updated.contains("FOO=bar"));
        assert!(updated.contains("API_KEY=secret"));
        assert!(updated.contains("NEXT_PUBLIC_NETWORK=mainnet"));
        assert!(!updated.contains("NEXT_PUBLIC_NETWORK=testnet"));
    }

    #[test]
    fn upsert_env_assignment_bootstraps_new_file() {
        let updated = upsert_env_assignment(
            "",
            "NEXT_PUBLIC_NETWORK",
            "devnet",
            "# Auto-written by stacksdapp dev --network devnet",
        );
        assert!(updated.starts_with("# Auto-written by stacksdapp dev --network devnet"));
        assert!(updated.contains("NEXT_PUBLIC_NETWORK=devnet"));
    }

    #[test]
    fn optimize_devnet_toml_injects_snapshot_pox_and_disables_explorers() {
        let raw = r#"# WARNING: public mnemonics

[network]
name = "devnet"

[accounts.deployer]
mnemonic = "twice kind fence tip"

[devnet]
disable_stacks_explorer = false
disable_stacks_api = false

[[devnet.pox_stacking_orders]]
start_at_cycle = 1
wallet = "wallet_1"
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(changed);
        assert!(updated.contains("[[devnet.pox_stacking_orders]]"));
        assert!(updated.contains("wallet = \"wallet_3\""));
        assert!(updated.contains("[accounts.wallet_1]"));
        assert!(updated.contains("disable_bitcoin_explorer = true"));
        assert!(updated.contains("disable_stacks_explorer = true"));
        assert!(updated.contains("disable_stacks_api = true"));
        assert!(updated.contains("bitcoin_controller_block_time = 45_000"));
        assert!(updated.contains("[accounts.deployer]"));
        assert!(updated.contains("mnemonic = \"twice kind fence tip\""));
    }

    #[test]
    fn optimize_devnet_toml_injects_missing_snapshot_block_for_minimal_devnet() {
        let raw = r#"[network]
name = "devnet"

[accounts.deployer]
mnemonic = "twice kind fence tip"

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = true
bitcoin_controller_block_time = 45_000
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(changed);
        assert!(updated.contains("[accounts.wallet_1]"));
        assert!(updated.contains("[[devnet.pox_stacking_orders]]"));
        assert!(updated.contains("btc_address = \"mvZtbibDAAA3WLpY7zXXFqRa3T4XSknBX7\""));
    }

    #[test]
    fn optimize_devnet_toml_is_idempotent_when_snapshot_compatible() {
        let raw = r#"[network]
name = "devnet"

[accounts.deployer]
mnemonic = "twice kind fence tip"

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

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = true
bitcoin_controller_block_time = 45_000

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
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(
            !changed,
            "already snapshot-compatible file should not rewrite"
        );
        assert_eq!(updated, raw);
    }

    #[test]
    fn optimize_devnet_toml_upgrades_too_aggressive_block_time() {
        let raw = r#"[network]
name = "devnet"

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = true
bitcoin_controller_block_time = 1_000
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(changed);
        assert!(updated.contains("bitcoin_controller_block_time = 45_000"));
        assert!(!updated.contains("bitcoin_controller_block_time = 1_000"));
    }

    #[test]
    fn optimize_devnet_toml_pins_stacks_node_405_on_older_clarinet() {
        let raw = r#"[network]
name = "devnet"

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = true
bitcoin_controller_block_time = 45_000
stacks_node_image_url = "ghcr.io/stacks-network/stacks-core:4.0.1-alpine"
stacks_signer_image_url = "ghcr.io/stacks-network/stacks-signer:4.0.1-alpine"
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot_with(raw, true);
        assert!(changed);
        assert!(updated.contains(
            "stacks_node_image_url = \"ghcr.io/stacks-network/stacks-core:4.0.5-alpine\""
        ));
        assert!(updated.contains(
            "stacks_signer_image_url = \"ghcr.io/stacks-network/stacks-signer:4.0.5-alpine\""
        ));
        assert!(!updated.contains("4.0.1"));
    }

    #[test]
    fn optimize_devnet_toml_leaves_custom_and_newer_stacks_images() {
        let raw = r#"[network]
name = "devnet"

[devnet]
stacks_node_image_url = "ghcr.io/example/stacks-core:9.0.0"
stacks_signer_image_url = "ghcr.io/stacks-network/stacks-signer:4.0.6-alpine"
"#;
        let (updated, _) = optimize_devnet_toml_for_fast_boot_with(raw, true);
        assert!(updated.contains("ghcr.io/example/stacks-core:9.0.0"));
        assert!(updated.contains("stacks-signer:4.0.6-alpine"));
        assert!(!updated.contains("stacks-core:4.0.5-alpine"));
    }

    #[test]
    fn bare_dev_is_the_clarinet_toolchain() {
        assert_eq!(
            super::classify_dev_invocation(None).unwrap(),
            super::DevInvocation::ClarinetToolchain
        );
    }

    #[test]
    fn network_flag_is_frontend_only_including_devnet() {
        assert_eq!(
            super::classify_dev_invocation(Some("devnet")).unwrap(),
            super::DevInvocation::FrontendOnly {
                network: "devnet".into()
            }
        );
        assert_eq!(
            super::classify_dev_invocation(Some("testnet")).unwrap(),
            super::DevInvocation::FrontendOnly {
                network: "testnet".into()
            }
        );
        assert_eq!(
            super::classify_dev_invocation(Some("mainnet")).unwrap(),
            super::DevInvocation::FrontendOnly {
                network: "mainnet".into()
            }
        );
        assert!(super::classify_dev_invocation(Some("simnet")).is_err());
    }

    #[test]
    fn dev_ready_json_payload_uses_ready_status_not_starting() {
        let payload =
            super::dev_ready_json_payload("devnet", "http://localhost:3000", Some(42), true, false);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["command"], "dev");
        assert_eq!(payload["status"], "ready");
        assert_ne!(payload["status"], "starting");
        assert_eq!(payload["network"], "devnet");
        assert_eq!(payload["url"], "http://localhost:3000");
        assert_eq!(payload["tip_height"], 42);
        assert_eq!(payload["auto_deploy"], true);
        assert_eq!(payload["keep_state"], false);
    }

    #[test]
    fn devnet_network_name_appends_optional_network_id() {
        let dir = std::env::temp_dir().join(format!("stacksdapp-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Devnet.toml");
        std::fs::write(
            &path,
            "[network]\nname = \"devnet\"\n\n[devnet]\nnetwork_id = \"lab\"\n",
        )
        .unwrap();
        assert_eq!(
            super::devnet_network_name("countercontract", &path),
            "countercontract.lab.devnet"
        );
        std::fs::write(&path, "[network]\nname = \"devnet\"\n").unwrap();
        assert_eq!(
            super::devnet_network_name("countercontract", &path),
            "countercontract.devnet"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn container_match_is_limited_to_this_projects_chain() {
        assert!(super::container_belongs_to_devnet(
            "stacks-node.countercontract.devnet",
            "countercontract.devnet"
        ));
        assert!(super::container_belongs_to_devnet(
            "stacks-signer-0.countercontract.devnet",
            "countercontract.devnet"
        ));
        assert!(super::container_belongs_to_devnet(
            "bitcoin-node.countercontract.devnet",
            "countercontract.devnet"
        ));
        assert!(!super::container_belongs_to_devnet(
            "stacks-api.countercontract.devnet",
            "countercontract.devnet"
        ));
        assert!(!super::container_belongs_to_devnet(
            "stacks-node.other.devnet",
            "countercontract.devnet"
        ));
    }

    #[test]
    fn session_dir_is_the_parent_of_the_data_mount() {
        let stacks =
            std::path::Path::new("/tmp/proj/contracts/.cache/stacks-devnet-1/data/1/stacks");
        assert_eq!(
            super::devnet_session_dir(stacks).unwrap(),
            std::path::PathBuf::from("/tmp/proj/contracts/.cache/stacks-devnet-1")
        );
    }

    #[test]
    fn remove_dev_cache_keeps_the_live_session() {
        let root = std::env::temp_dir().join(format!("stacksdapp-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cache = root.join(".cache");
        let session = cache.join("stacks-devnet-9");
        std::fs::create_dir_all(session.join("conf")).unwrap();
        std::fs::write(session.join("conf").join("Stacks.toml"), "keep").unwrap();
        std::fs::write(cache.join("abi-export-cache.json"), "{}").unwrap();
        std::fs::create_dir_all(cache.join("requirements")).unwrap();

        super::remove_dev_cache(&cache, Some(&session)).unwrap();

        assert!(session.join("conf").join("Stacks.toml").is_file());
        assert!(!cache.join("abi-export-cache.json").exists());
        assert!(!cache.join("requirements").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn select_snapshot_dir_requires_stacks_and_bitcoin() {
        let root = std::env::temp_dir().join(format!("stacksdapp-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let incomplete = root.join("incomplete");
        std::fs::create_dir_all(incomplete.join("stacks").join("krypton")).unwrap();
        let complete = root.join("complete");
        std::fs::create_dir_all(complete.join("stacks").join("krypton")).unwrap();
        std::fs::create_dir_all(complete.join("bitcoin").join("regtest")).unwrap();
        assert_eq!(super::select_snapshot_dir(&root).unwrap(), complete);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn host_bind_path_strips_docker_desktop_prefix() {
        let dir = std::env::temp_dir().join(format!("stacksdapp-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let canonical = dir.canonicalize().unwrap();
        let prefixed = format!("/host_mnt{}", canonical.display());
        assert!(!std::path::Path::new(&prefixed).exists());
        assert_eq!(super::host_bind_path(&prefixed), canonical);
        assert_eq!(
            super::host_bind_path(&canonical.display().to_string()),
            canonical
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
