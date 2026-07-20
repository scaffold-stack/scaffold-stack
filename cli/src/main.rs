use anyhow::Result;
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;
use serde_json::json;
use stacksdapp_shell::{self as shell, status};
use std::io;
use std::process::ExitCode;

mod commands {
    pub mod doctor;
}
mod error;

use commands::doctor;
use error::{code_name_for, exit_code_for, CliError};

const CLI_BEFORE_HELP: &str = r#"
  ┌─ Scaffold Stacks ─────────────────────────────────────────┐
  │  Clarity contracts · Next.js · Bitcoin L2 (Stacks)         │
  │  new · dev · generate · deploy · test — one workspace.      │
  └───────────────────────────────────────────────────────────┘
"#;

#[derive(Parser)]
#[command(
    name = "stacksdapp",
    version,
    about = "Full-stack toolkit for Stacks: scaffold, run, and ship Clarity + Next.js apps.",
    before_help = CLI_BEFORE_HELP,
    after_help = "Examples:\n  stacksdapp new my-dapp && cd my-dapp && stacksdapp dev\n  stacksdapp generate\n  stacksdapp deploy --network testnet\n  stacksdapp doctor --json\n  cd frontend && stacksdapp check   # walks up to project root\n  stacksdapp --root ../my-dapp test\n  stacksdapp completions zsh > ~/.zfunc/_stacksdapp"
)]
struct Cli {
    /// Verbosity (-v, -vv, …). Higher values print more diagnostic detail.
    #[arg(short = 'v', long = "verbosity", action = ArgAction::Count, global = true)]
    verbosity: u8,

    /// Suppress non-error log messages
    #[arg(short = 'q', long, global = true, conflicts_with = "verbosity")]
    quiet: bool,

    /// Color of log messages: auto | always | never
    #[arg(long, global = true, default_value = "auto", value_parser = ["auto", "always", "never"])]
    color: String,

    /// Format command results as JSON (implies quiet human logs)
    #[arg(long, global = true)]
    json: bool,

    /// Project root (default: walk up for stacksdapp.toml or contracts/Clarinet.toml)
    #[arg(long, global = true, value_name = "PATH", env = "STACKSDAPP_ROOT")]
    root: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new monorepo workspace
    New {
        /// Project name (single directory segment: letters, digits, -, _)
        name: String,
        /// Skip git init
        #[arg(long)]
        no_git: bool,
    },
    /// Start dev environment — devnet/testnet/mainnet
    ///
    /// devnet  (default): spins up local Clarinet chain + Next.js + watcher
    /// testnet/mainnet:   runs Next.js pointed at remote network (no local chain)
    /// Use --auto-deploy with devnet to deploy contracts once the local chain is ready.
    Dev {
        /// Network to target: devnet | testnet | mainnet
        #[arg(long, value_parser = ["devnet", "testnet", "mainnet"])]
        network: Option<String>,
        /// After devnet is ready, deploy contracts automatically (devnet only)
        #[arg(long)]
        auto_deploy: bool,
        /// Preserve local devnet state/cache between runs (devnet only)
        #[arg(long)]
        keep_state: bool,
    },
    /// Parse contracts and regenerate TypeScript bindings
    Generate {
        /// Watch contracts and regenerate bindings on change
        #[arg(long)]
        watch: bool,
    },
    /// Adopt an existing Clarinet project in the current directory
    Init,
    /// Add a new Clarity contract: stacksdapp add <name> [--template blank|sip010|sip009]
    Add {
        /// Contract name (Clarity id: letter, then letters/digits/-/_; max 40)
        name: String,
        /// Template to use
        #[arg(long, default_value = "blank", value_parser = ["blank", "sip010", "sip009"])]
        template: String,
    },
    /// Deploy contracts to a network (defaults.network from stacksdapp.toml when omitted)
    Deploy {
        #[arg(long, value_parser = ["devnet", "testnet", "mainnet"])]
        network: Option<String>,
        /// Deploy a single contract by name (must exist in contracts/Clarinet.toml)
        #[arg(long)]
        contract: Option<String>,
        /// Generate plan and fee estimate without broadcasting transactions
        #[arg(long)]
        dry_run: bool,
        /// Skip interactive confirmation and auto-answer Clarinet fee prompts
        /// (required for non-interactive testnet/mainnet deploys)
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Run contract and frontend tests (vitest)
    Test,
    /// Type-check all Clarity contracts
    Check,
    /// Remove generated files and devnet state
    Clean {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Check all prerequisites and print a status report
    Doctor {
        /// Treat warnings as failures (non-zero exit)
        #[arg(long)]
        strict: bool,
    },
    /// Refresh dependencies and regenerate bindings
    Upgrade,
    /// Generate shell completions (bash, zsh, fish, powershell, elvish)
    #[command(visible_alias = "com")]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_shell(&cli);
    shell::debug(
        1,
        format!(
            "stacksdapp starting (verbosity={}, quiet={}, json={}, color={})",
            cli.verbosity, cli.quiet, cli.json, cli.color
        ),
    );

    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let code = exit_code_for(&e);
            let kind = code_name_for(&e);
            if shell::is_json() {
                if !shell::json_already_emitted() {
                    shell::emit_json(&json!({
                        "ok": false,
                        "command": "stacksdapp",
                        "error": e.to_string(),
                        "code": kind,
                        "exit_code": code,
                    }));
                }
            } else {
                eprintln!("Error: {e:#}");
                shell::debug(1, format!("exit_code={code} ({kind})"));
            }
            ExitCode::from(code as u8)
        }
    }
}

fn init_shell(cli: &Cli) {
    let color = shell::ColorMode::parse(&cli.color).unwrap_or(shell::ColorMode::Auto);
    let format = if cli.json {
        shell::Format::Json
    } else {
        shell::Format::Human
    };
    shell::init(shell::Shell {
        verbosity: cli.verbosity,
        quiet: cli.quiet,
        format,
        color,
    });
}

async fn dispatch(cli: Cli) -> Result<()> {
    enter_project_context(&cli)?;

    match cli.command {
        Commands::New { name, no_git } => {
            stacksdapp_scaffold::new_project(&name, !no_git)
                .await
                .map_err(map_scaffold_err)?;
            emit_command_ok("new", json!({ "name": name, "git": !no_git }));
            Ok(())
        }
        Commands::Dev {
            network,
            auto_deploy,
            keep_state,
        } => {
            let network = resolve_default_network(network.as_deref())?;
            emit_command_ok(
                "dev",
                json!({
                    "network": network,
                    "auto_deploy": auto_deploy,
                    "keep_state": keep_state,
                    "status": "starting",
                }),
            );
            stacksdapp_process_supervisor::dev(&network, auto_deploy, keep_state).await
        }
        Commands::Generate { watch } => run_generate(watch).await,
        Commands::Init => {
            stacksdapp_scaffold::init_project()
                .await
                .map_err(map_scaffold_err)?;
            emit_command_ok("init", json!({}));
            Ok(())
        }
        Commands::Add { name, template } => {
            stacksdapp_scaffold::add_contract(&name, &template)
                .await
                .map_err(map_scaffold_err)?;
            emit_command_ok("add", json!({ "name": name, "template": template }));
            Ok(())
        }
        Commands::Deploy {
            network,
            contract,
            dry_run,
            yes,
        } => {
            let network = resolve_default_network(network.as_deref())?;
            stacksdapp_deployer::deploy(&network, contract.as_deref(), dry_run, yes)
                .await
                .map_err(|e| {
                    let msg = format!("{e:#}");
                    let lower = msg.to_ascii_lowercase();
                    if lower.contains("refusing to deploy")
                        || lower.contains("aborted")
                        || lower.contains("confirmation cancelled")
                    {
                        anyhow::Error::new(CliError::Aborted(msg))
                    } else {
                        anyhow::Error::new(CliError::Deploy(msg))
                    }
                })?;
            emit_command_ok(
                "deploy",
                json!({
                    "network": network,
                    "contract": contract,
                    "dry_run": dry_run,
                    "yes": yes,
                }),
            );
            Ok(())
        }
        Commands::Test => run_test().await,
        Commands::Check => run_check().await,
        Commands::Clean { force } => run_clean(force).await,
        Commands::Doctor { strict } => doctor::run(strict).await,
        Commands::Upgrade => {
            stacksdapp_scaffold::upgrade_project()
                .await
                .map_err(map_scaffold_err)?;
            emit_command_ok("upgrade", json!({}));
            Ok(())
        }
        Commands::Completions { shell } => print_completions(shell),
    }
}

fn emit_command_ok(command: &str, details: serde_json::Value) {
    if !shell::is_json() {
        return;
    }
    let mut payload = json!({ "ok": true, "command": command });
    if let (Some(base), Some(extra)) = (payload.as_object_mut(), details.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    shell::emit_json(&payload);
}

fn resolve_default_network(cli_value: Option<&str>) -> Result<String> {
    if let Some(value) = cli_value {
        return Ok(value.to_string());
    }
    let root = std::env::current_dir().ok();
    let config = root
        .as_deref()
        .and_then(|cwd| shell::load_config(cwd).ok())
        .unwrap_or_default();
    let network = config
        .defaults
        .and_then(|defaults| defaults.network)
        .unwrap_or_else(|| "devnet".to_string());
    shell::validate_network(&network).map_err(|e| anyhow::anyhow!(e))?;
    Ok(network)
}

fn map_scaffold_err(e: anyhow::Error) -> anyhow::Error {
    let msg = format!("{e:#}");
    let lower = msg.to_ascii_lowercase();
    if lower.contains("invalid project name")
        || lower.contains("invalid contract name")
        || lower.contains("cannot contain path")
        || lower.contains("cannot be an absolute path")
        || lower.contains("must be a single directory")
        || lower.contains("must be at most")
        || lower.contains("cannot be empty")
        || lower.contains("cannot be '.' or '..'")
    {
        anyhow::Error::new(CliError::Validation(msg))
    } else {
        e
    }
}

fn print_completions(shell: Shell) -> Result<()> {
    use std::io::Write;

    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    generate(shell, &mut cmd, "stacksdapp", &mut buf);
    match io::stdout().write_all(&buf) {
        Ok(()) => Ok(()),
        // `… | head` closes the pipe early — not a real failure.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Foundry-style root discovery: chdir into the project before path-relative commands.
fn enter_project_context(cli: &Cli) -> Result<()> {
    use std::env;

    match &cli.command {
        // Create / diagnose / completions relative to the caller's cwd — do not walk up.
        Commands::New { .. } | Commands::Doctor { .. } | Commands::Completions { .. } => Ok(()),

        // Init: prefer nearest Clarinet or scaffold root, else stay in cwd.
        Commands::Init => {
            if let Some(ref explicit) = cli.root {
                let root = if explicit.is_absolute() {
                    explicit.clone()
                } else {
                    env::current_dir()?.join(explicit)
                };
                let root = root.canonicalize().map_err(|e| {
                    anyhow::Error::new(CliError::Project(format!(
                        "Invalid --root '{}': {e}",
                        explicit.display()
                    )))
                })?;
                let is_ok = root.join(shell::CONFIG_FILE).is_file()
                    || root.join("contracts").join("Clarinet.toml").is_file()
                    || root.join("Clarinet.toml").is_file();
                if !is_ok {
                    return Err(CliError::Project(format!(
                        "Directory '{}' is not a Clarinet/stacksdapp project.",
                        root.display()
                    ))
                    .into());
                }
                env::set_current_dir(&root)?;
                shell::debug(1, format!("init: using --root {}", root.display()));
                return Ok(());
            }
            let cwd = env::current_dir()?;
            if let Some(root) = shell::find_init_root(&cwd) {
                if root != cwd {
                    shell::debug(1, format!("init: walking up to {}", root.display()));
                    env::set_current_dir(&root)?;
                } else {
                    shell::debug(1, format!("init: project root {}", root.display()));
                }
            }
            Ok(())
        }

        // All other commands require a scaffold project root.
        _ => {
            let root = shell::enter_scaffold_root(cli.root.as_deref()).map_err(|msg| {
                anyhow::Error::new(
                    if msg.to_ascii_lowercase().contains("invalid --root")
                        || msg.contains("No stacksdapp project")
                        || msg.contains("is not a stacksdapp project")
                    {
                        CliError::Project(msg)
                    } else {
                        CliError::Other(msg)
                    },
                )
            })?;
            shell::debug(1, format!("project root {}", root.display()));
            if let Ok(cfg) = shell::load_config(&root) {
                if let Some(name) = cfg.project.and_then(|p| p.name) {
                    shell::debug(1, format!("stacksdapp.toml project.name={name}"));
                }
            }
            Ok(())
        }
    }
}

async fn run_generate(watch: bool) -> Result<()> {
    use std::path::Path;
    stacksdapp_codegen::generate_all()
        .await
        .map_err(|e| CliError::Generate(format!("{e:#}")))?;
    if watch {
        status(
            "[generate] Watching contracts/contracts for .clar changes..."
                .cyan()
                .to_string(),
        );
        stacksdapp_watcher::watch_contracts(Path::new("contracts/contracts"))
            .await
            .map_err(|e| CliError::Generate(format!("{e:#}")))?;
    }
    if shell::is_json() {
        shell::emit_json(&json!({ "ok": true, "command": "generate", "watch": watch }));
    }
    Ok(())
}

async fn run_test() -> Result<()> {
    if tokio::fs::metadata("contracts/Clarinet.toml")
        .await
        .is_err()
    {
        return Err(CliError::Project(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
                .into(),
        )
        .into());
    }

    run_vitest_suite("contracts", "contract", VitestRunMode::NpmScript).await?;

    if tokio::fs::metadata("frontend/package.json").await.is_ok() {
        run_vitest_suite("frontend", "frontend", VitestRunMode::RunPassWithNoTests).await?;
    }

    status("All tests passed.".green().bold().to_string());
    if shell::is_json() {
        shell::emit_json(&json!({ "ok": true, "command": "test" }));
    }
    Ok(())
}

enum VitestRunMode {
    /// Uses `npm run test` (contracts use `vitest run` in package.json).
    NpmScript,
    /// Runs `vitest run --passWithNoTests` so empty frontend suites still pass.
    RunPassWithNoTests,
}

async fn ensure_npm_dependencies(dir: &str) -> Result<()> {
    use tokio::process::Command;

    if tokio::fs::metadata(format!("{dir}/node_modules"))
        .await
        .is_ok()
    {
        return Ok(());
    }

    status(
        format!("[test] Installing {dir} dependencies...")
            .cyan()
            .to_string(),
    );
    let subcommand = if tokio::fs::metadata(format!("{dir}/package-lock.json"))
        .await
        .is_ok()
    {
        "ci"
    } else {
        "install"
    };
    let install = Command::new("npm")
        .arg(subcommand)
        .args([
            "--no-audit",
            "--no-fund",
            "--prefer-offline",
            "--progress=false",
            "--loglevel=error",
        ])
        .current_dir(dir)
        .status()
        .await;

    match install {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(CliError::Other(format!("npm install failed in {dir}/")).into()),
        Err(_) => Err(CliError::Prerequisite(
            "Node.js >=20 is required. Install from nodejs.org".into(),
        )
        .into()),
    }
}

async fn run_vitest_suite(dir: &str, label: &str, mode: VitestRunMode) -> Result<()> {
    use tokio::process::Command;

    ensure_npm_dependencies(dir).await?;

    status(
        format!("[test] Running {label} tests (vitest)...")
            .cyan()
            .to_string(),
    );
    shell::debug(1, format!("vitest cwd={dir}"));

    let status_code = match mode {
        VitestRunMode::NpmScript | VitestRunMode::RunPassWithNoTests => {
            Command::new("npm")
                .args(["run", "test"])
                .current_dir(dir)
                .status()
                .await
        }
    };

    match status_code {
        Ok(s) if s.success() => {
            status(
                format!("[test] {} tests passed.", capitalize_test_label(label))
                    .green()
                    .to_string(),
            );
            Ok(())
        }
        Ok(_) => {
            Err(CliError::Test(format!("{} tests failed.", capitalize_test_label(label))).into())
        }
        Err(_) => Err(CliError::Prerequisite(
            "Node.js >=20 is required. Install from nodejs.org".into(),
        )
        .into()),
    }
}

fn capitalize_test_label(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

async fn run_check() -> Result<()> {
    use tokio::process::Command;

    status(
        "[check] Type-checking Clarity contracts..."
            .cyan()
            .to_string(),
    );

    let cmd_status = Command::new("clarinet")
        .args(["check"])
        .current_dir("contracts")
        .status()
        .await;

    match cmd_status {
        Ok(s) if s.success() => {
            status(
                "[check] All contracts passed type-checking."
                    .green()
                    .to_string(),
            );
            if shell::is_json() {
                shell::emit_json(&json!({ "ok": true, "command": "check" }));
            }
            Ok(())
        }
        Ok(_) => Err(CliError::Check(
            "Clarity type-check failed. Fix the errors reported above.".into(),
        )
        .into()),
        Err(_) => Err(CliError::Prerequisite(
            "clarinet is required. Install: brew install clarinet OR cargo install clarinet".into(),
        )
        .into()),
    }
}

async fn run_clean(force: bool) -> Result<()> {
    use std::io::{self, IsTerminal, Write};
    use std::path::{Path, PathBuf};
    use tokio::fs;

    if !Path::new("contracts/Clarinet.toml").exists() {
        return Err(CliError::Project(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
                .into(),
        )
        .into());
    }

    let mut targets: Vec<(PathBuf, &'static str)> = Vec::new();
    let generated_dir = PathBuf::from("frontend/src/generated");
    if generated_dir.exists() {
        targets.push((generated_dir.clone(), "directory"));
    }
    for dir in ["contracts/.cache", "contracts/.devnet"] {
        let path = PathBuf::from(dir);
        if path.exists() {
            targets.push((path, "directory"));
        }
    }
    for auto_generated in ["Simnet.toml", "Epoch25.toml", "Epoch30.toml"] {
        let path = Path::new("contracts/settings").join(auto_generated);
        if path.exists() {
            targets.push((path, "file"));
        }
    }

    if targets.is_empty() {
        status("[clean] Nothing to remove.".green().to_string());
        if shell::is_json() {
            shell::emit_json(&json!({
                "ok": true,
                "command": "clean",
                "removed": [],
            }));
        }
        return Ok(());
    }

    status("[clean] Will remove:".cyan().to_string());
    for (path, kind) in &targets {
        status(format!("  • {} ({kind})", path.display()));
    }

    if !force {
        if shell::is_json() || !io::stdin().is_terminal() {
            return Err(CliError::Aborted(
                "Refusing to clean without confirmation in a non-interactive terminal.\n\
                 Re-run with: stacksdapp clean --force"
                    .into(),
            )
            .into());
        }
        print!("\nType 'y' to confirm deletion: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() != "y" {
            return Err(CliError::Aborted("Clean aborted.".into()).into());
        }
    }

    status(
        "[clean] Removing generated files and devnet state..."
            .cyan()
            .to_string(),
    );

    let mut removed = Vec::new();
    for (path, kind) in &targets {
        match *kind {
            "directory" => fs::remove_dir_all(path).await?,
            _ => fs::remove_file(path).await?,
        }
        removed.push(path.display().to_string());
        status(
            format!("[clean] Removed {}", path.display())
                .yellow()
                .to_string(),
        );
    }

    fs::create_dir_all(&generated_dir).await?;
    fs::write(
        generated_dir.join("deployments.json"),
        r#"{ "network": "", "deployed_at": "", "contracts": {} }"#,
    )
    .await?;

    status(
        "[clean] Done. Run `stacksdapp generate` to regenerate bindings."
            .green()
            .to_string(),
    );
    if shell::is_json() {
        shell::emit_json(&json!({
            "ok": true,
            "command": "clean",
            "removed": removed,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{emit_command_ok, resolve_default_network};
    use serde_json::json;
    use std::fs;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cli_flag_overrides_config_default_network() {
        assert_eq!(resolve_default_network(Some("mainnet")).unwrap(), "mainnet");
    }

    #[test]
    fn config_default_network_is_used_when_flag_missing() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("stacksdapp.toml"),
            "[defaults]\nnetwork = \"testnet\"\n",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let resolved = resolve_default_network(None).unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        assert_eq!(resolved, "testnet");
    }

    #[test]
    fn invalid_config_default_network_is_rejected() {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("stacksdapp.toml"),
            "[defaults]\nnetwork = \"staging\"\n",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let resolved = resolve_default_network(None);
        std::env::set_current_dir(&cwd).unwrap();
        assert!(resolved.is_err());
    }

    #[test]
    fn emit_command_ok_is_noop_without_json_mode() {
        emit_command_ok("new", json!({ "name": "demo", "git": true }));
    }
}
