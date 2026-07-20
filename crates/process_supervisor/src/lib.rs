use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::{fs, io::AsyncRead};

/// Returns true if Clarinet.toml contains any [[project.requirements]] entries.
async fn has_requirements() -> bool {
    let Ok(raw) = fs::read_to_string("contracts/Clarinet.toml").await else {
        return false;
    };
    raw.contains("[[project.requirements]]")
}

async fn prefetch_requirements() -> Result<()> {
    if !has_requirements().await {
        return Ok(());
    }

    let mut child = Command::new("clarinet")
        .args(["requirements"])
        .current_dir("contracts")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| {
            anyhow!(
                "clarinet is required. Install: brew install clarinet  OR  cargo install clarinet"
            )
        })?;

    attach_filtered_output(
        &mut child,
        OutputStyle::Clarinet,
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );

    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!(
            "Failed to fetch contract requirements.\n\
             \n\
             This usually means:\n\
             • No internet connection — requirements must be fetched online at least once\n\
             • Hiro API is temporarily down — try again in a few minutes\n\
             \n\
             Once fetched, requirements are cached in contracts/.cache/ and work offline.\n\
             Check which contracts you depend on in contracts/Clarinet.toml under [[project.requirements]]."
        ));
    }

    Ok(())
}

pub async fn dev(network: &str, auto_deploy: bool, keep_state: bool) -> Result<()> {
    ensure_project_root()?;

    match network {
        "devnet" => dev_devnet(auto_deploy, keep_state).await,
        "testnet" | "mainnet" => {
            if auto_deploy {
                stacksdapp_shell::warn(format!(
                    "[dev] --auto-deploy applies to devnet only; ignoring for {network}."
                ));
            }
            if keep_state {
                stacksdapp_shell::warn(format!(
                    "[dev] --keep-state applies to devnet only; ignoring for {network}."
                ));
            }
            dev_remote(network).await
        }
        other => Err(anyhow!(
            "Unknown network '{}'. Use: devnet, testnet, or mainnet",
            other
        )),
    }
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
        stacksdapp_shell::kv(
            "Deploy",
            "stacksdapp deploy --network devnet",
        );
        println!(
            "{}",
            "  Tip must keep advancing before deploy; stalled tips need: stacksdapp clean && stacksdapp dev"
                .truecolor(156, 163, 175)
        );
    } else {
        stacksdapp_shell::kv(
            "Deploy",
            &format!("stacksdapp deploy --network {network}"),
        );
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

async fn dev_devnet(auto_deploy: bool, keep_state: bool) -> Result<()> {
    stacksdapp_shell::print_banner("Development Mode 🌱");

    let step = stacksdapp_shell::begin_step("Environment configured");
    if let Err(e) = ensure_docker() {
        step.fail();
        return Err(e);
    }
    // Free ports held by leftover Clarinet Docker from other projects before we start.
    stop_stale_devnet_docker();
    if let Err(e) = ensure_devnet_fast_boot_settings().await {
        step.fail();
        return Err(e);
    }
    if !keep_state {
        if let Err(e) = reset_local_devnet_state().await {
            step.fail();
            return Err(e);
        }
    }
    if let Err(e) = write_network_env("devnet").await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let connected = format!("Connected to {}", network_display("devnet"));
    let step = stacksdapp_shell::begin_step(&connected);
    if let Err(e) = prefetch_requirements().await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let step = stacksdapp_shell::begin_step("Contract bindings ready");
    if let Err(e) = stacksdapp_codegen::generate_all_quiet().await {
        step.fail();
        return Err(e);
    }
    step.finish();

    let mut clarinet = spawn_clarinet_devnet()?;
    let fatal = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    attach_filtered_output(&mut clarinet, OutputStyle::Clarinet, std::sync::Arc::clone(&fatal));

    // Clarinet 3.21.x writes pox_5_* keys into Stacks.toml that stacks-core 3.4
    // (Clarinet's default image) rejects, so stacks-node dies instantly. Patch the
    // generated config as soon as it appears, before Clarinet starts that container.
    let stacks_toml_patcher = tokio::spawn(patch_generated_stacks_toml_until_ready(
        std::sync::Arc::clone(&fatal),
    ));

    let step = stacksdapp_shell::begin_step("Devnet nodes starting (bitcoin + stacks)");
    let tip_height = match wait_for_devnet_chain_ready(
        Duration::from_secs(300),
        &mut clarinet,
        std::sync::Arc::clone(&fatal),
    )
    .await
    {
        Ok(height) => {
            stacks_toml_patcher.abort();
            let _ = stacks_toml_patcher.await;
            step.finish();
            stacksdapp_shell::step_ok(&format!("Devnet ready — stacks tip #{height}"));
            Some(height)
        }
        Err(e) => {
            stacks_toml_patcher.abort();
            let _ = stacks_toml_patcher.await;
            step.fail();
            let _ = clarinet.start_kill();
            let _ = clarinet.wait().await;
            return Err(e);
        }
    };

    let health_monitor = tokio::spawn(monitor_devnet_chain_health());

    let deploy_task = if auto_deploy {
        Some(tokio::spawn(run_auto_deploy()))
    } else {
        None
    };

    let step = stacksdapp_shell::begin_step("Next.js started");
    let mut frontend = match spawn_next_dev_process("devnet") {
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
            print_ready_panel("devnet", &url, tip_height);
        }
        Ok(Err(_)) => {
            step.fail();
            return Err(anyhow!("Frontend dev server closed before becoming ready."));
        }
        Err(_) => {
            step.fail();
            return Err(anyhow!(
                "Frontend did not become ready within 120s. Check that ports 3000/3001 are free."
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
        status = clarinet.wait() => {
            match status {
                Ok(s) if s.success() => Err(anyhow!("Clarinet devnet exited unexpectedly.")),
                Ok(s) => Err(anyhow!("Clarinet devnet exited with status: {s}")),
                Err(e) => Err(anyhow!("Failed to wait for Clarinet devnet process: {e}")),
            }
        }
        status = frontend.wait() => {
            match status {
                Ok(s) if s.success() => Err(anyhow!("Frontend dev server exited unexpectedly.")),
                Ok(s) => Err(anyhow!("Frontend dev server exited with status: {s}")),
                Err(e) => Err(anyhow!("Failed to wait for frontend dev server: {e}")),
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

    health_monitor.abort();
    let _ = health_monitor.await;
    if let Some(task) = deploy_task {
        task.abort();
        let _ = task.await;
    }

    shutdown_child("clarinet devnet", &mut clarinet).await;
    shutdown_child("frontend dev server", &mut frontend).await;
    watcher.abort();
    let _ = watcher.await;

    result?;

    Ok(())
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
            print_ready_panel(network, &url, None);
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

async fn run_auto_deploy() {
    match stacksdapp_deployer::wait_for_devnet_node().await {
        Ok(()) => {
            if let Err(e) = stacksdapp_deployer::deploy("devnet", None, false, false).await {
                stacksdapp_shell::println_human_safe(format!("[dev] Auto-deploy failed: {e:#}"));
                stacksdapp_shell::println_human_safe(
                    "[dev] You can deploy manually in another terminal: stacksdapp deploy --network devnet",
                );
            }
        }
        Err(e) => {
            stacksdapp_shell::println_human_safe(format!(
                "[dev] Devnet did not become ready for auto-deploy: {e:#}"
            ));
        }
    }
}

/// Poll local stacks core until tip advances at least twice (proves blocks keep
/// producing under Nakamoto, not just a single tenure after boot).
async fn wait_for_devnet_chain_ready(
    timeout: Duration,
    clarinet: &mut Child,
    fatal: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) -> Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let started = Instant::now();
    let mut first_tip: Option<u64> = None;
    let mut second_tip: Option<u64> = None;
    let mut last_status = Instant::now() - Duration::from_secs(30);

    loop {
        if let Ok(guard) = fatal.lock() {
            if let Some(msg) = guard.as_ref() {
                return Err(anyhow!("{msg}"));
            }
        }

        match clarinet.try_wait() {
            Ok(Some(status)) => {
                let detail = fatal
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                    .unwrap_or_else(|| {
                        format!("Clarinet exited early with status {status} before stacks-node was ready.")
                    });
                return Err(anyhow!(
                    "{detail}\n\
                     Free leftover Devnet ports/containers, then retry:\n\
                       docker ps --filter name=devnet\n\
                       docker stop $(docker ps -q --filter name=devnet) 2>/dev/null\n\
                       pkill -f 'clarinet devnet' 2>/dev/null\n\
                       stacksdapp clean --force && stacksdapp dev"
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(anyhow!("Failed to poll Clarinet process: {e}"));
            }
        }

        // Only check for crashes after Clarinet has had time to create containers
        // (avoids false positives from unrelated leftover names; clean already stopped ours).
        if started.elapsed() >= Duration::from_secs(15) {
            if let Some(crash) = detect_stacks_node_crash().await {
                if let Ok(mut slot) = fatal.lock() {
                    if slot.is_none() {
                        *slot = Some(crash.clone());
                    }
                }
                return Err(anyhow!("{crash}"));
            }
        }

        if started.elapsed() > timeout {
            return Err(anyhow!(
                "Devnet did not become ready within {}s.\n\
                 Stacks core at http://localhost:20443 never kept producing blocks.\n\
                 Common causes:\n\
                 • bitcoin_controller_block_time too aggressive (signer cannot keep up)\n\
                 • Clarinet/stacks-core image mismatch (stacks-node exits on bad Stacks.toml)\n\
                 • Port conflict (e.g. 20445 / 5432 already in use) — leftover Clarinet/Docker from another project\n\
                 • Docker low on RAM/CPU — give Docker Desktop more resources\n\
                 • Stale state — try `stacksdapp clean --force` then `stacksdapp dev`\n\
                 Fix ports:\n\
                   docker stop $(docker ps -q --filter name=devnet) 2>/dev/null\n\
                   pkill -f 'clarinet devnet' 2>/dev/null",
                timeout.as_secs()
            ));
        }

        match fetch_local_stacks_tip(&client).await {
            Some(height) => match (first_tip, second_tip) {
                (None, _) => {
                    first_tip = Some(height);
                    if last_status.elapsed() >= Duration::from_secs(8) {
                        stacksdapp_shell::println_human_safe(format!(
                            "[devnet] stacks core up — tip #{height}, waiting for next block..."
                        ));
                        last_status = Instant::now();
                    }
                }
                (Some(first), None) if height > first => {
                    second_tip = Some(height);
                    stacksdapp_shell::println_human_safe(format!(
                        "[devnet] tip advanced to #{height} — confirming sustained block production..."
                    ));
                    last_status = Instant::now();
                }
                (Some(_), Some(second)) if height > second => return Ok(height),
                (Some(_), Some(second)) => {
                    if last_status.elapsed() >= Duration::from_secs(15) {
                        stacksdapp_shell::println_human_safe(format!(
                            "[devnet] waiting for tip to keep advancing (still #{second})..."
                        ));
                        last_status = Instant::now();
                    }
                }
                (Some(first), None) => {
                    if last_status.elapsed() >= Duration::from_secs(15) {
                        stacksdapp_shell::println_human_safe(format!(
                            "[devnet] waiting for tip to advance (still #{first})..."
                        ));
                        last_status = Instant::now();
                    }
                }
            },
            None => {
                if last_status.elapsed() >= Duration::from_secs(12) {
                    stacksdapp_shell::println_human_safe(
                        "[devnet] waiting for bitcoin-node / stacks-node (this can take 1–3 min)...",
                    );
                    last_status = Instant::now();
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Clarinet 3.21 writes `pox_5_*` under `[node]` for PoX-5 / Epoch 4, but its default
/// Docker image is still stacks-core 3.4.x which rejects those keys and exits immediately.
/// Strip them from every generated `Stacks.toml` until the tip is reachable.
async fn patch_generated_stacks_toml_until_ready(
    fatal: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    let mut announced = false;
    loop {
        if fatal.lock().ok().and_then(|g| g.clone()).is_some() {
            return;
        }
        match patch_pox5_fields_in_cache().await {
            Ok(n) if n > 0 && !announced => {
                stacksdapp_shell::println_human_safe(
                    "[devnet] Patched Stacks.toml (removed Clarinet pox_5_* keys incompatible with stacks-core 3.4).",
                );
                announced = true;
            }
            Ok(_) => {}
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn patch_pox5_fields_in_cache() -> Result<usize> {
    let cache = Path::new("contracts/.cache");
    if !cache.is_dir() {
        return Ok(0);
    }
    let mut patched = 0usize;
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
            patched += 1;
        }
    }
    Ok(patched)
}

/// Returns true when the file was rewritten.
async fn patch_stacks_toml_file(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path).await?;
    let (updated, changed) = strip_pox5_node_fields(&raw);
    if changed {
        fs::write(path, updated).await?;
    }
    Ok(changed)
}

/// Pure helper: drop Clarinet-injected `pox_5_*` keys that stacks-core 3.4 rejects.
pub fn strip_pox5_node_fields(raw: &str) -> (String, bool) {
    let mut out = Vec::new();
    let mut removed = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("pox_5_") {
            removed = true;
            continue;
        }
        out.push(line);
    }
    if !removed {
        return (raw.to_string(), false);
    }
    let mut updated = out.join("\n");
    if raw.ends_with('\n') && !updated.ends_with('\n') {
        updated.push('\n');
    }
    (updated, true)
}

/// Fail fast when Clarinet starts stacks-node and the container exits (bad config, OOM, etc.).
async fn detect_stacks_node_crash() -> Option<String> {
    let list = tokio::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=stacks-node",
            "--format",
            "{{.Names}}\t{{.Status}}\t{{.ID}}",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;

    if !list.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&list.stdout);
    let mut crashed_id: Option<String> = None;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("stacks-node") || !lower.contains("devnet") {
            continue;
        }
        if lower.contains("exited") || lower.contains("dead") {
            let id = line.split('\t').nth(2).unwrap_or("").trim().to_string();
            if !id.is_empty() {
                crashed_id = Some(id);
            }
        }
    }

    let id = crashed_id?;
    let logs = tokio::process::Command::new("docker")
        .args(["logs", "--tail", "40", &id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&logs.stdout),
        String::from_utf8_lossy(&logs.stderr)
    );
    let lower = combined.to_ascii_lowercase();
    if lower.contains("pox_5_sbtc") || lower.contains("unknown field `pox_5") {
        // Host conf is usually bind-mounted — patch + restart can recover this boot.
        let _ = patch_pox5_fields_in_cache().await;
        let _ = tokio::process::Command::new("docker")
            .args(["start", &id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        // If restart worked, tip poll will succeed; only fail if still dead.
        let still_dead = tokio::process::Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("id={id}"),
                "--format",
                "{{.Status}}",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()
            .map(|o| {
                let s = String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
                s.contains("exited") || s.contains("dead")
            })
            .unwrap_or(true);
        if !still_dead {
            stacksdapp_shell::println_human_safe(
                "[devnet] Restarted stacks-node after patching incompatible pox_5_* config keys.",
            );
            return None;
        }
        return Some(
            "Devnet stacks-node crashed: Clarinet wrote pox_5_* keys that stacks-core 3.4 rejects.\n\
             stacksdapp patches Stacks.toml automatically — retry after `stacksdapp clean --force`.\n\
             Or upgrade Clarinet (`brew upgrade clarinet`) once a fix release is available."
                .to_string(),
        );
    }
    if lower.contains("invalid config")
        || lower.contains("process abort")
        || lower.contains("fatal:")
        || lower.contains("invalid toml")
    {
        let snippet = combined
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Some(format!(
            "Devnet stacks-node container exited.\n{snippet}\n\
             Try: stacksdapp clean --force && stacksdapp dev"
        ));
    }
    Some(
        "Devnet stacks-node container exited before becoming ready.\n\
         Check: docker logs $(docker ps -aq --filter name=stacks-node | head -1)\n\
         Then: stacksdapp clean --force && stacksdapp dev"
            .to_string(),
    )
}

/// Poll local devnet tip height and warn if the chain appears stalled.
async fn monitor_devnet_chain_health() {
    tokio::time::sleep(Duration::from_secs(20)).await;

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut last_height: Option<u64> = None;
    let mut unchanged_since = Instant::now();
    let mut last_warning = Instant::now() - Duration::from_secs(120);

    loop {
        let current = fetch_local_stacks_tip(&client).await;

        if let Some(height) = current {
            if Some(height) == last_height {
                if unchanged_since.elapsed() >= Duration::from_secs(45)
                    && last_warning.elapsed() >= Duration::from_secs(60)
                {
                    stacksdapp_shell::println_human_safe(format!(
                        "[devnet] Warning: local chain tip stalled at height {height} for 45s+."
                    ));
                    stacksdapp_shell::println_human_safe(
                        "  Deployments may hang until blocks advance. This is often a Clarinet/Docker issue.",
                    );
                    stacksdapp_shell::println_human_safe(
                        "  Try: stacksdapp clean --force && stacksdapp dev",
                    );
                    last_warning = Instant::now();
                }
            } else {
                last_height = Some(height);
                unchanged_since = Instant::now();
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn fetch_local_stacks_tip(client: &reqwest::Client) -> Option<u64> {
    let response = client
        .get("http://localhost:20443/v2/info")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().await.ok()?;
    json.get("stacks_tip_height")?.as_u64()
}

#[derive(Clone, Copy)]
enum OutputStyle {
    Clarinet,
}

fn attach_filtered_output(
    child: &mut Child,
    style: OutputStyle,
    fatal: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    // Clarinet 3.2+ may prompt: "Do you want to continue? (y/N)" when the default
    // snapshot is incompatible with built-in PoX stacking defaults. Without an answer,
    // Docker never starts and we hang forever waiting for :20443.
    let stdin = child.stdin.take().map(|s| {
        std::sync::Arc::new(tokio::sync::Mutex::new(Some(s)))
    });

    if let Some(stdout) = child.stdout.take() {
        spawn_filtered_stream(
            stdout,
            style,
            std::sync::Arc::clone(&fatal),
            stdin.clone(),
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_filtered_stream(stderr, style, fatal, stdin);
    }
}

fn spawn_filtered_stream<R>(
    reader: R,
    style: OutputStyle,
    fatal: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    stdin: Option<std::sync::Arc<tokio::sync::Mutex<Option<tokio::process::ChildStdin>>>>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match style {
                OutputStyle::Clarinet => {
                    if clarinet_needs_continue_answer(&line) {
                        stacksdapp_shell::println_human_safe(
                            "[devnet] Clarinet snapshot skipped (default PoX stacking) — continuing boot...",
                        );
                        if let Some(stdin) = &stdin {
                            let mut guard = stdin.lock().await;
                            if let Some(pipe) = guard.as_mut() {
                                use tokio::io::AsyncWriteExt;
                                let _ = pipe.write_all(b"y\n").await;
                                let _ = pipe.flush().await;
                            }
                        }
                        continue;
                    }
                    if let Some(fatal_msg) = clarinet_fatal_message(&line) {
                        stacksdapp_shell::println_human_safe(format!("[devnet] {fatal_msg}"));
                        if let Ok(mut slot) = fatal.lock() {
                            if slot.is_none() {
                                *slot = Some(fatal_msg);
                            }
                        }
                        continue;
                    }
                    // Clarinet may rewrite Stacks.toml immediately before starting
                    // stacks-node — force another strip as soon as we see that log line.
                    let lower = strip_ansi(&line).to_ascii_lowercase();
                    if lower.contains("starting stacks-node")
                        || lower.contains("initiating devnet boot")
                        || lower.contains("copying stacks snapshot")
                    {
                        let _ = patch_pox5_fields_in_cache().await;
                    }
                    if let Some(msg) = format_clarinet_line(&line) {
                        stacksdapp_shell::println_human_safe(msg);
                    }
                }
            }
        }
    });
}

fn clarinet_needs_continue_answer(line: &str) -> bool {
    let lower = strip_ansi(line).to_ascii_lowercase();
    lower.contains("do you want to continue?")
        || (lower.contains("(y/n)") && lower.contains("continue"))
}

fn clarinet_fatal_message(line: &str) -> Option<String> {
    let plain = strip_ansi(line);
    let lower = plain.to_ascii_lowercase();
    if lower.contains("address already in use")
        || lower.contains("os error 48")
        || lower.contains("port is already allocated")
        || (lower.contains("bind for 0.0.0.0:") && lower.contains("failed"))
        || lower.contains("fatal:")
        || lower.contains("unable to start postgres")
        || (lower.contains("unable to start") && lower.contains("container"))
    {
        Some(format!(
            "Devnet failed to start: {plain}\n\
             Another Clarinet/Docker Devnet is still holding ports (often 20445 or 5432).\n\
             Free them, then retry:\n\
               docker stop $(docker ps -q --filter name=devnet) 2>/dev/null\n\
               pkill -f 'clarinet devnet' 2>/dev/null\n\
               stacksdapp clean --force && stacksdapp dev"
        ))
    } else {
        None
    }
}

fn strip_ansi(line: &str) -> String {
    let t = line.trim();
    t.chars()
        .filter(|c| *c == '\t' || (*c >= ' ' && *c != '\u{007f}'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn format_clarinet_line(line: &str) -> Option<String> {
    let plain = strip_ansi(line);
    if plain.is_empty() {
        return None;
    }
    let lower = plain.to_ascii_lowercase();

    // Boot progress — keep the spinner row from looking stuck.
    if lower.contains("initiating devnet boot")
        || lower.contains("starting bitcoin-node")
        || lower.contains("starting stacks-node")
        || lower.contains("starting postgres")
        || lower.contains("starting stacks-api")
        || lower.contains("starting stacks-signer")
        || lower.contains("copying bitcoin snapshot")
        || lower.contains("copying stacks snapshot")
        || lower.contains("snapshot copied")
        || lower.contains("continuing with startup")
        || lower.contains("default snapshot can not be used")
        || lower.contains("local devnet network ready")
        || lower.contains("stacks block #")
        || lower.contains("bitcoin-node - mining")
        || lower.contains("stacks-node - mining")
        || lower.contains("blockchain verification completed")
    {
        return Some(format!("[devnet] {plain}"));
    }
    if lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("unable to")
    {
        return Some(format!("[devnet] {plain}"));
    }
    None
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

/// Rewrite `contracts/settings/Devnet.toml` for Clarinet 3.2+ snapshot fast-boot.
/// Preserves accounts and other custom keys; only strips PoX stacking orders and
/// ensures explorers are off / API on / faster block time. Does not touch Testnet/Mainnet.
async fn ensure_devnet_fast_boot_settings() -> Result<()> {
    let path = Path::new("contracts/settings/Devnet.toml");
    if !path.is_file() {
        return Ok(());
    }
    let raw = fs::read_to_string(path).await?;
    let (updated, changed) = optimize_devnet_toml_for_fast_boot(&raw);
    if changed {
        fs::write(path, updated).await?;
        stacksdapp_shell::println_human_safe(
            "[devnet] Applied fast-boot settings (snapshot-friendly; explorers off).",
        );
    }
    Ok(())
}

/// Pure rewrite helper (unit-tested). Returns `(new_contents, did_change)`.
pub fn optimize_devnet_toml_for_fast_boot(raw: &str) -> (String, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut skipping_pox = false;
    let mut in_devnet = false;
    let mut saw_devnet = false;
    let mut has_btc_explorer = false;
    let mut has_stacks_explorer = false;
    let mut has_stacks_api = false;
    let mut has_block_time = false;
    let mut removed_pox = false;

    for line in raw.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            skipping_pox = false;
            in_devnet = trimmed == "[devnet]";
            if in_devnet {
                saw_devnet = true;
            }
            if trimmed == "[[devnet.pox_stacking_orders]]" {
                skipping_pox = true;
                removed_pox = true;
                continue;
            }
        }

        if skipping_pox {
            // Drop the whole array-of-tables block (keys until next [section]).
            continue;
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
                // Keep API enabled — frontend / deploy tooling may use :3999.
                out.push("disable_stacks_api = false".to_string());
                continue;
            }
            if trimmed.starts_with("bitcoin_controller_block_time") {
                has_block_time = true;
                // 15s: local UX stays usable; Nakamoto/signer can keep pace.
                // 1s burns ahead of stacks-node and tips stall after the first tenure.
                out.push("bitcoin_controller_block_time = 15_000".to_string());
                continue;
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
                    final_out.push("disable_stacks_api = false".to_string());
                    injected = true;
                }
                if !has_block_time {
                    final_out.push("bitcoin_controller_block_time = 15_000".to_string());
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
        out.push("disable_stacks_api = false".to_string());
        out.push("bitcoin_controller_block_time = 15_000".to_string());
        injected = true;
    }

    let mut updated = out.join("\n");
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    let normalized_raw = if raw.ends_with('\n') {
        raw.to_string()
    } else {
        format!("{raw}\n")
    };
    let changed = removed_pox || injected || updated != normalized_raw;
    (updated, changed)
}

fn spawn_clarinet_devnet() -> Result<Child> {
    // --no-dashboard is required: Clarinet's TUI corrupts our spinner/ready panel.
    // --from-genesis avoids Clarinet's default snapshot path, which with Clarinet 3.21
    // often boots a hybrid bitcoin/stacks snapshot that mines one Nakamoto tenure then stalls.
    // stdin must be piped so we can answer the snapshot "continue? (y/N)" prompt if it still appears.
    let child = Command::new("clarinet")
        .args(["devnet", "start", "--no-dashboard", "--from-genesis"])
        .current_dir("contracts")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(child)
}

async fn reset_local_devnet_state() -> Result<()> {
    for path in ["contracts/.cache", "contracts/.devnet"] {
        if fs::metadata(path).await.is_ok() {
            fs::remove_dir_all(path).await?;
        }
    }
    Ok(())
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
        optimize_devnet_toml_for_fast_boot, strip_pox5_node_fields, upsert_env_assignment,
    };

    #[test]
    fn strip_pox5_node_fields_removes_clarinet_keys() {
        let raw = r#"[node]
working_dir = "/devnet"
rpc_bind = "0.0.0.0:20443"
pox_5_sbtc_contract = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.sbtc-token"
pox_5_sbtc_registry_contract = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.sbtc-registry"
pox_5_bond_admin = "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM"

[connection_options]
auth_token = "12345"
"#;
        let (updated, changed) = strip_pox5_node_fields(raw);
        assert!(changed);
        assert!(!updated.contains("pox_5_"));
        assert!(updated.contains("rpc_bind = \"0.0.0.0:20443\""));
        assert!(updated.contains("auth_token = \"12345\""));
    }

    #[test]
    fn strip_pox5_node_fields_noop_when_absent() {
        let raw = "[node]\nrpc_bind = \"0.0.0.0:20443\"\n";
        let (updated, changed) = strip_pox5_node_fields(raw);
        assert!(!changed);
        assert_eq!(updated, raw);
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
    fn optimize_devnet_toml_strips_pox_and_disables_explorers() {
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

[[devnet.pox_stacking_orders]]
start_at_cycle = 1
wallet = "wallet_2"
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(changed);
        assert!(!updated.contains("pox_stacking_orders"));
        assert!(updated.contains("disable_bitcoin_explorer = true"));
        assert!(updated.contains("disable_stacks_explorer = true"));
        assert!(updated.contains("disable_stacks_api = false"));
        assert!(updated.contains("bitcoin_controller_block_time = 15_000"));
        assert!(updated.contains("[accounts.deployer]"));
        assert!(updated.contains("mnemonic = \"twice kind fence tip\""));
    }

    #[test]
    fn optimize_devnet_toml_is_idempotent_when_already_fast() {
        let raw = r#"[network]
name = "devnet"

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = false
bitcoin_controller_block_time = 15_000
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(!changed, "already optimized file should not rewrite");
        assert_eq!(updated, raw);
    }

    #[test]
    fn optimize_devnet_toml_upgrades_too_aggressive_block_time() {
        let raw = r#"[network]
name = "devnet"

[devnet]
disable_bitcoin_explorer = true
disable_stacks_explorer = true
disable_stacks_api = false
bitcoin_controller_block_time = 1_000
"#;
        let (updated, changed) = optimize_devnet_toml_for_fast_boot(raw);
        assert!(changed);
        assert!(updated.contains("bitcoin_controller_block_time = 15_000"));
        assert!(!updated.contains("bitcoin_controller_block_time = 1_000"));
    }
}
