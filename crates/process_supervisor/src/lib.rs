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

    attach_filtered_output(&mut child, OutputStyle::Clarinet);

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

fn print_ready_panel(local_url: &str) {
    if stacksdapp_shell::is_quiet() {
        return;
    }
    println!();
    stacksdapp_shell::rule();
    println!();
    stacksdapp_shell::kv("Local", local_url);
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
    attach_filtered_output(&mut clarinet, OutputStyle::Clarinet);

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
            print_ready_panel(&url);
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
            print_ready_panel(&url);
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
                eprintln!("[dev] Auto-deploy failed: {e:#}");
                eprintln!(
                    "[dev] You can deploy manually in another terminal: stacksdapp deploy --network devnet"
                );
            }
        }
        Err(e) => {
            eprintln!("[dev] Devnet did not become ready for auto-deploy: {e:#}");
        }
    }
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
                    eprintln!(
                        "\n[devnet] Warning: local chain tip stalled at height {height} for 45s+."
                    );
                    eprintln!(
                        "  Deployments may hang until blocks advance. This is often a Clarinet/Docker devnet issue."
                    );
                    eprintln!("  Try: stacksdapp clean && stacksdapp dev");
                    eprintln!(
                        "  Track upstream: https://github.com/scaffold-stack/scaffold-stack/issues/53\n"
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

fn attach_filtered_output(child: &mut Child, style: OutputStyle) {
    if let Some(stdout) = child.stdout.take() {
        spawn_filtered_stream(stdout, style, false);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_filtered_stream(stderr, style, true);
    }
}

fn spawn_filtered_stream<R>(reader: R, style: OutputStyle, is_stderr: bool)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match style {
                OutputStyle::Clarinet => {
                    // Keep Clarinet noise down during startup; surface failures.
                    let lower = line.to_ascii_lowercase();
                    if lower.contains("error")
                        || lower.contains("failed")
                        || lower.contains("panic")
                        || is_stderr && !line.trim().is_empty()
                    {
                        eprintln!("{}", line);
                    }
                }
            }
        }
    });
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
                println!("{formatted}");
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

fn spawn_clarinet_devnet() -> Result<Child> {
    let child = Command::new("clarinet")
        .args(["devnet", "start"])
        .current_dir("contracts")
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
    use super::upsert_env_assignment;

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
}
