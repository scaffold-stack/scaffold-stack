use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
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

    println!(
        "[dev] Detected [[project.requirements]] in Clarinet.toml — fetching external contracts..."
    );
    println!("[dev] This requires internet access. Run once; results are cached in ./.cache/");

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

    attach_prefixed_output(&mut child, "clarinet");

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

    println!("[dev] ✔ Requirements fetched and cached.");
    Ok(())
}

pub async fn dev(network: &str, auto_deploy: bool) -> Result<()> {
    ensure_project_root()?;

    match network {
        "devnet" => dev_devnet(auto_deploy).await,
        "testnet" | "mainnet" => {
            if auto_deploy {
                println!("[dev] --auto-deploy applies to devnet only; ignoring for {network}.");
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

async fn dev_devnet(auto_deploy: bool) -> Result<()> {
    println!("[dev] Starting devnet stack (Docker required)...");

    ensure_docker()?;

    // Clarinet devnet can get stuck on stale cached chainstate snapshots.
    // Starting from a clean local devnet state avoids reorg-corrupted API
    // indexes and frozen tips that block contract deployments from finalizing.
    reset_local_devnet_state().await?;

    write_network_env("devnet").await?;

    prefetch_requirements().await?;
    stacksdapp_codegen::generate_all().await?;

    let mut clarinet = spawn_clarinet_devnet()?;
    attach_prefixed_output(&mut clarinet, "clarinet");

    let health_monitor = tokio::spawn(monitor_devnet_chain_health());

    let deploy_task = if auto_deploy {
        println!("[dev] --auto-deploy enabled: will deploy contracts once devnet is ready.");
        Some(tokio::spawn(run_auto_deploy()))
    } else {
        None
    };

    let mut frontend = spawn_next_dev_process("devnet")?;
    attach_prefixed_output(&mut frontend, "next");

    let mut watcher = tokio::spawn(stacksdapp_watcher::watch_contracts(Path::new(
        "contracts/contracts",
    )));

    let result = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("[dev] Ctrl+C received — shutting down devnet stack...");
            Ok(())
        }
        status = clarinet.wait() => {
            match status {
                Ok(s) if s.success() => Err(anyhow!("[dev] Clarinet devnet exited unexpectedly.")),
                Ok(s) => Err(anyhow!("[dev] Clarinet devnet exited with status: {s}")),
                Err(e) => Err(anyhow!("[dev] Failed to wait for Clarinet devnet process: {e}")),
            }
        }
        status = frontend.wait() => {
            match status {
                Ok(s) if s.success() => Err(anyhow!("[dev] Frontend dev server exited unexpectedly.")),
                Ok(s) => Err(anyhow!("[dev] Frontend dev server exited with status: {s}")),
                Err(e) => Err(anyhow!("[dev] Failed to wait for frontend dev server: {e}")),
            }
        }
        watcher_result = &mut watcher => {
            match watcher_result {
                Ok(Ok(())) => Err(anyhow!("[dev] Contract watcher exited unexpectedly.")),
                Ok(Err(e)) => Err(anyhow!("[dev] Contract watcher failed: {e}")),
                Err(e) => Err(anyhow!("[dev] Contract watcher task join failed: {e}")),
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
    println!(
        "[dev] Starting frontend for {} (no local chain needed)...",
        network
    );

    check_deployments(network)?;
    write_network_env(network).await?;
    stacksdapp_codegen::generate_all().await?;
    spawn_next_dev(network).await?;
    Ok(())
}

async fn run_auto_deploy() {
    match stacksdapp_deployer::wait_for_devnet_node().await {
        Ok(()) => {
            if let Err(e) = stacksdapp_deployer::deploy("devnet", None, false).await {
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

fn attach_prefixed_output(child: &mut Child, prefix: &str) {
    let tag = prefix.to_string();
    if let Some(stdout) = child.stdout.take() {
        spawn_prefixed_stream(stdout, tag.clone(), false);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_prefixed_stream(stderr, tag, true);
    }
}

fn spawn_prefixed_stream<R>(reader: R, prefix: String, is_stderr: bool)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if is_stderr {
                eprintln!("[{prefix}] {line}");
            } else {
                println!("[{prefix}] {line}");
            }
        }
    });
}

async fn write_network_env(network: &str) -> Result<()> {
    let env_path = Path::new("frontend/.env.local");
    let content = format!(
        "# Auto-written by stacksdapp dev --network {network}\n\
         NEXT_PUBLIC_NETWORK={network}\n"
    );
    fs::write(env_path, content).await?;
    println!("[dev] Set NEXT_PUBLIC_NETWORK={network} in frontend/.env.local");
    Ok(())
}

fn check_deployments(network: &str) -> Result<()> {
    let path = Path::new("frontend/src/generated/deployments.json");
    if !path.exists() {
        println!(
            "[dev] Warning: deployments.json not found.\n\
             Run `stacksdapp deploy --network {network}` first so the frontend \
             knows your contract addresses."
        );
        return Ok(());
    }

    let raw = std::fs::read_to_string(path).unwrap_or_default();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
        let deployed_network = json["network"].as_str().unwrap_or("");
        if deployed_network != network && !deployed_network.is_empty() {
            println!(
                "[dev] Warning: deployments.json is for '{}' but you requested '{}'.\n\
                 Run `stacksdapp deploy --network {network}` to deploy to {network} first.",
                deployed_network, network
            );
        }
        let contracts = json["contracts"].as_object();
        if contracts.map(|c| c.is_empty()).unwrap_or(true) {
            println!(
                "[dev] Warning: No contracts in deployments.json.\n\
                 Run `stacksdapp deploy --network {network}` to populate it."
            );
        }
    }
    Ok(())
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
            println!("[dev] Removed stale {path}");
        }
    }
    Ok(())
}

async fn spawn_next_dev(network: &str) -> Result<()> {
    let mut child = spawn_next_dev_process(network)?;
    attach_prefixed_output(&mut child, "next");
    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!(
            "[dev] Frontend dev server exited with status: {status}"
        ));
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
    println!("[dev] Stopped {name}.");
}
