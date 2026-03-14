use anyhow::{anyhow, Result};
use std::path::Path;
use tokio::{fs, process::Command};

pub async fn dev() -> Result<()> {
    ensure_project_root()?;
    codegen::generate_all().await?;

    tokio::try_join!(
        spawn_clarinet_devnet(),
        spawn_next_dev(),
        watcher::watch_contracts(Path::new("contracts/contracts")),
    )?;

    Ok(())
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

async fn spawn_clarinet_devnet() -> Result<()> {
    Command::new("clarinet")
        .args(["devnet", "start"])
        .current_dir("contracts")
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

async fn spawn_next_dev() -> Result<()> {
    if fs::metadata("frontend/node_modules").await.is_err() {
        println!("Installing frontend dependencies with npm install...");
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
        .spawn()?
        .wait()
        .await?;
    Ok(())
}