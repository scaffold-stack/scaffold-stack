use anyhow::{anyhow, Result};
use std::path::Path;
use tokio::{fs, process::Command};


/// Returns true if Clarinet.toml contains any [[project.requirements]] entries.
async fn has_requirements() -> bool {
    let Ok(raw) = fs::read_to_string("contracts/Clarinet.toml").await else {
        return false;
    };
    // A [[project.requirements]] section always contains a contract_id line
    raw.contains("[[project.requirements]]")
}

async fn prefetch_requirements() -> Result<()> {
    if !has_requirements().await {
        return Ok(()); // nothing to do
    }

    println!(
        "[dev] Detected [[project.requirements]] in Clarinet.toml — fetching external contracts..."
    );
    println!("[dev] This requires internet access. Run once; results are cached in ./.cache/");

    let status = tokio::process::Command::new("clarinet")
        .args(["requirements"])
        .current_dir("contracts")
        .status()
        .await
        .map_err(|_| anyhow!(
            "clarinet is required. Install: brew install clarinet  OR  cargo install clarinet"
        ))?;

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

pub async fn dev(network: &str) -> Result<()> {
    ensure_project_root()?;

    match network {
        "devnet" => dev_devnet().await,
        "testnet" | "mainnet" => dev_remote(network).await,
        other => Err(anyhow!(
            "Unknown network '{}'. Use: devnet, testnet, or mainnet", other
        )),
    }
}

fn ensure_project_root() -> Result<()> {
    if !Path::new("contracts/Clarinet.toml").exists()
        || !Path::new("frontend/package.json").exists()
    {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacks-dapp new"
        ));
    }
    Ok(())
}

// ── devnet ────────────────────────────────────────────────────────────────────
// Spins up a full local stack: Clarinet devnet + Next.js + file watcher.
// Requires Docker Desktop to be running.

async fn dev_devnet() -> Result<()> {
    println!("[dev] Starting devnet stack (Docker required)...");

    // Check Docker is available before wasting time on codegen
    ensure_docker()?;

    // Set NEXT_PUBLIC_NETWORK=devnet in the frontend env
    write_network_env("devnet").await?;

    prefetch_requirements().await?;
    codegen::generate_all().await?;

    tokio::try_join!(
        spawn_clarinet_devnet(),
        spawn_next_dev("devnet"),
        watcher::watch_contracts(Path::new("contracts/contracts")),
    )?;

    Ok(())
}

// ── testnet / mainnet ─────────────────────────────────────────────────────────
// No local chain — just run the frontend pointed at the remote network.
// Contracts must already be deployed (`stacks-dapp deploy --network testnet`).

async fn dev_remote(network: &str) -> Result<()> {
    println!("[dev] Starting frontend for {} (no local chain needed)...", network);

    // Verify deployments.json has entries for this network
    check_deployments(network)?;

    // Write the correct NEXT_PUBLIC_NETWORK to .env.local
    write_network_env(network).await?;

    // Regenerate bindings so they reflect the current contracts
    codegen::generate_all().await?;

    // Just run Next.js — no devnet, no watcher needed
    spawn_next_dev(network).await?;

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Overwrite frontend/.env.local with the correct NEXT_PUBLIC_NETWORK.
/// This means `stacks-dapp dev --network testnet` automatically switches
/// the frontend without the developer having to touch .env.local manually.
async fn write_network_env(network: &str) -> Result<()> {
    let env_path = Path::new("frontend/.env.local");
    let content = format!(
        "# Auto-written by stacks-dapp dev --network {network}\n\
         NEXT_PUBLIC_NETWORK={network}\n"
    );
    fs::write(env_path, content).await?;
    println!("[dev] Set NEXT_PUBLIC_NETWORK={network} in frontend/.env.local");
    Ok(())
}

/// Warn if deployments.json doesn't have contracts for the requested network.
fn check_deployments(network: &str) -> Result<()> {
    let path = Path::new("frontend/src/generated/deployments.json");
    if !path.exists() {
        println!(
            "[dev] Warning: deployments.json not found.\n\
             Run `stacks-dapp deploy --network {network}` first so the frontend \
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
                 Run `stacks-dapp deploy --network {network}` to deploy to {network} first.",
                deployed_network, network
            );
        }
        let contracts = json["contracts"].as_object();
        if contracts.map(|c| c.is_empty()).unwrap_or(true) {
            println!(
                "[dev] Warning: No contracts in deployments.json.\n\
                 Run `stacks-dapp deploy --network {network}` to populate it."
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

async fn spawn_clarinet_devnet() -> Result<()> {
    Command::new("clarinet")
        .args(["devnet", "start"])
        .current_dir("contracts")
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

async fn spawn_next_dev(network: &str) -> Result<()> {
    if fs::metadata("frontend/node_modules").await.is_err() {
        println!("[dev] Installing frontend dependencies...");
        Command::new("npm")
            .arg("install")
            .current_dir("frontend")
            .spawn()?
            .wait()
            .await?;
    }

    Command::new("npm")
        .args(["run", "dev"])
        .current_dir("frontend")
        .env("NEXT_PUBLIC_NETWORK", network)
        .spawn()?
        .wait()
        .await?;
    Ok(())
}