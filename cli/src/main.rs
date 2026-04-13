use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
mod commands {
    pub mod doctor;
}
use commands::doctor;

#[derive(Parser)]
#[command(name = "stacksdapp", version, about = "Scaffold-Stacks CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new monorepo workspace
    New {
        /// Project name (becomes directory name)
        name: String,
        /// Skip git init
        #[arg(long)]
        no_git: bool,
    },
    /// Start dev environment — devnet/testnet/mainnet
    ///
    /// devnet  (default): spins up local Clarinet chain + Next.js + watcher
    /// testnet/mainnet:   runs Next.js pointed at remote network (no local chain)
    Dev {
        /// Network to target: devnet | testnet | mainnet
        #[arg(long, default_value = "devnet")]
        network: String,
    },
    /// Parse contracts and regenerate TypeScript bindings
    Generate,
    /// Add a new Clarity contract: stacks-dapp add <name> [--template blank|sip010|sip009]
    Add {
        /// Contract name
        name: String,
        /// Template to use
        #[arg(long, default_value = "blank")]
        template: String,
    },
    /// Deploy contracts to a network
    Deploy {
        #[arg(long, default_value = "devnet")]
        network: String,
    },
    /// Run contract tests (vitest) and frontend tests (vitest)
    Test,
    /// Type-check all Clarity contracts
    Check,
    /// Remove generated files and devnet state
    Clean,
    /// Check all prerequisites and print a status report
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::New { name, no_git } => stacksdapp_scaffold::new_project(&name, !no_git).await,
        Commands::Dev { network } => stacksdapp_process_supervisor::dev(&network).await,
        Commands::Generate => stacksdapp_codegen::generate_all().await,
        Commands::Add { name, template } => {
            stacksdapp_scaffold::add_contract(&name, &template).await
        }
        Commands::Deploy { network } => stacksdapp_deployer::deploy(&network).await,
        Commands::Test => run_test().await,
        Commands::Check => run_check().await,
        Commands::Clean => run_clean().await,
        Commands::Doctor => doctor::run().await,
    }
}

async fn run_test() -> Result<()> {
    use tokio::process::Command;

    println!("{}", "[test] Running contract tests (vitest)...".cyan());
    if tokio::fs::metadata("contracts/node_modules").await.is_err() {
        println!("{}", "[test] Installing contract dependencies...".cyan());
        let install = Command::new("npm")
            .arg("install")
            .current_dir("contracts")
            .status()
            .await;
        match install {
            Ok(s) if s.success() => {}
            Ok(_) => anyhow::bail!("npm install failed in contracts/"),
            Err(_) => anyhow::bail!("Node.js >=20 is required. Install from nodejs.org"),
        }
    }

    let contract_status = Command::new("npm")
        .args(["run", "test"])
        .current_dir("contracts")
        .status()
        .await;
    match contract_status {
        Ok(s) if !s.success() => anyhow::bail!("Contract tests failed."),
        Err(_) => anyhow::bail!("Node.js >=20 is required. Install from nodejs.org"),
        Ok(_) => println!("{}", "[test] Contract tests passed.".green()),
    }

    println!("{}", "[test] Running frontend tests (vitest)...".cyan());
    if tokio::fs::metadata("frontend/node_modules").await.is_err() {
        println!("{}", "[test] Installing frontend dependencies...".cyan());
        let install = Command::new("npm")
            .arg("install")
            .current_dir("frontend")
            .status()
            .await;
        match install {
            Ok(s) if s.success() => {}
            Ok(_) => anyhow::bail!("npm install failed in frontend/"),
            Err(_) => anyhow::bail!("Node.js >=20 is required. Install from nodejs.org"),
        }
    }

    let vitest_status = Command::new("npm")
        .args(["run", "test"])
        .current_dir("frontend")
        .status()
        .await;
    match vitest_status {
        Ok(s) if !s.success() => anyhow::bail!("Frontend tests failed."),
        Err(_) => anyhow::bail!("Node.js >=20 is required. Install from nodejs.org"),
        Ok(_) => println!("{}", "[test] Frontend tests passed.".green()),
    }

    println!("{}", "All tests passed.".green().bold());
    Ok(())
}

async fn run_check() -> Result<()> {
    use tokio::process::Command;

    println!("{}", "[check] Type-checking Clarity contracts...".cyan());

    let status = Command::new("clarinet")
        .args(["check"])
        .current_dir("contracts")
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            println!("{}", "[check] All contracts passed type-checking.".green());
            Ok(())
        }
        Ok(_) => anyhow::bail!("Clarity type-check failed. Fix the errors reported above."),
        Err(_) => anyhow::bail!(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet"
        ),
    }
}

async fn run_clean() -> Result<()> {
    use std::path::Path;
    use tokio::fs;

    println!(
        "{}",
        "[clean] Removing generated files and devnet state...".cyan()
    );

    let generated_dir = Path::new("frontend/src/generated");
    if generated_dir.exists() {
        fs::remove_dir_all(generated_dir).await?;
        println!("{}", "[clean] Removed frontend/src/generated/".yellow());
    }

    let devnet_dir = Path::new("contracts/.cache");
    if devnet_dir.exists() {
        fs::remove_dir_all(devnet_dir).await?;
        println!("{}", "[clean] Removed contracts/.cache/".yellow());
    }

    let devnet_data = Path::new("contracts/.devnet");
    if devnet_data.exists() {
        fs::remove_dir_all(devnet_data).await?;
        println!("{}", "[clean] Removed contracts/.devnet/".yellow());
    }

    for auto_generated in &["Simnet.toml", "Epoch25.toml", "Epoch30.toml"] {
        let path = Path::new("contracts/settings").join(auto_generated);
        if path.exists() {
            fs::remove_file(&path).await?;
            println!("[clean] Removed contracts/settings/{auto_generated}");
        }
    }

    fs::create_dir_all(generated_dir).await?;
    fs::write(
        generated_dir.join("deployments.json"),
        r#"{ "network": "", "deployed_at": "", "contracts": {} }"#,
    )
    .await?;

    println!(
        "{}",
        "[clean] Done. Run `stacks-dapp generate` to regenerate bindings.".green()
    );
    Ok(())
}
