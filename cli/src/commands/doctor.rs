use anyhow::Result;
use colored::Colorize;
use serde_json::json;
use stacksdapp_shell::{
    self as shell, find_scaffold_root, inspect_settings_file, status, MnemonicCheck,
};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::error::CliError;

struct Check {
    name: &'static str,
    result: CheckResult,
}

enum CheckResult {
    Ok(String),
    Warn(String),
    Fail(String),
}

impl CheckResult {
    fn status_str(&self) -> &'static str {
        match self {
            Self::Ok(_) => "ok",
            Self::Warn(_) => "warn",
            Self::Fail(_) => "fail",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::Ok(s) | Self::Warn(s) | Self::Fail(s) => s,
        }
    }
}

/// Run prerequisite checks.
///
/// Exit semantics (for CI / preflight):
/// - Fail → always non-zero
/// - Warn → zero by default; non-zero when `strict` is true
pub async fn run(strict: bool) -> Result<()> {
    shell::debug(1, "doctor: probing rustc, node, clarinet, docker, git");

    let checks = vec![
        check_rust().await,
        check_node().await,
        check_clarinet().await,
        check_docker().await,
        check_git().await,
        check_git_hooks().await,
        check_deploy_mnemonics().await,
        check_devnet_epochs().await,
        check_stacksdapp().await,
    ];

    let mut fail_count = 0usize;
    let mut warn_count = 0usize;

    for check in &checks {
        match &check.result {
            CheckResult::Ok(_) => {}
            CheckResult::Warn(_) => warn_count += 1,
            CheckResult::Fail(_) => fail_count += 1,
        }
    }

    let ok = fail_count == 0 && (!strict || warn_count == 0);

    let exit_code = if !ok { 3 } else { 0 };
    if shell::is_json() {
        let checks_json: Vec<_> = checks
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "status": c.result.status_str(),
                    "detail": c.result.detail(),
                })
            })
            .collect();
        shell::emit_json(&json!({
            "ok": ok,
            "command": "doctor",
            "strict": strict,
            "fail_count": fail_count,
            "warn_count": warn_count,
            "code": if ok { "ok" } else { "prerequisite" },
            "exit_code": exit_code,
            "checks": checks_json,
        }));
    } else {
        status(format!(
            "\n{}\n",
            "stacksdapp doctor — checking prerequisites".bold()
        ));

        for check in &checks {
            match &check.result {
                CheckResult::Ok(msg) => {
                    status(format!(
                        "  {}  {} {}",
                        "✔".green().bold(),
                        check.name.white(),
                        msg.dimmed()
                    ));
                }
                CheckResult::Warn(msg) => {
                    status(format!(
                        "  {}  {} {}",
                        "⚠".yellow().bold(),
                        check.name.white(),
                        msg.yellow()
                    ));
                }
                CheckResult::Fail(msg) => {
                    status(format!(
                        "  {}  {} {}",
                        "✗".red().bold(),
                        check.name.white().bold(),
                        msg.red()
                    ));
                }
            }
        }

        status("");

        if fail_count == 0 && warn_count == 0 {
            status(
                "  All checks passed. You're ready to build on Stacks!"
                    .green()
                    .bold()
                    .to_string(),
            );
            status("");
            return Ok(());
        }

        if fail_count > 0 {
            status(
                "  Some checks failed. Fix the issues above before running stacksdapp new."
                    .red()
                    .bold()
                    .to_string(),
            );
            status("");
        } else {
            status(
                "  Some checks warned. Review the issues above before running stacksdapp new."
                    .yellow()
                    .to_string(),
            );
            if strict {
                status(
                    "  (--strict: treating warnings as failures)"
                        .dimmed()
                        .to_string(),
                );
            }
            status("");
        }
    }

    if fail_count > 0 {
        return Err(CliError::Prerequisite(format!(
            "doctor failed: {fail_count} failing check(s){}",
            if warn_count > 0 {
                format!(", {warn_count} warning(s)")
            } else {
                String::new()
            }
        ))
        .into());
    }

    if warn_count > 0 && strict {
        return Err(CliError::Prerequisite(format!(
            "doctor failed: {warn_count} warning(s) under --strict"
        ))
        .into());
    }

    Ok(())
}

// ── Individual checks ─────────────────────────────────────────────────────────

async fn check_rust() -> Check {
    match version_output("rustc", &["--version"]).await {
        Some(v) => {
            // "rustc 1.78.0 (9b00956e5 2024-04-29)"
            let version = v
                .trim_start_matches("rustc ")
                .split_whitespace()
                .next()
                .unwrap_or("?")
                .to_string();
            if meets_semver(&version, 1, 75) {
                Check {
                    name: "Rust",
                    result: CheckResult::Ok(version),
                }
            } else {
                Check {
                    name: "Rust",
                    result: CheckResult::Warn(format!(
                        "{version} — Rust 1.75+ required. Run: rustup update"
                    )),
                }
            }
        }
        None => Check {
            name: "Rust",
            result: CheckResult::Fail("not found. Install from https://rustup.rs".into()),
        },
    }
}

async fn check_node() -> Check {
    match version_output("node", &["--version"]).await {
        Some(v) => {
            // "v20.11.0"
            let version = v.trim().trim_start_matches('v').to_string();
            let major: u32 = version
                .split('.')
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            if major >= 20 {
                Check {
                    name: "Node.js",
                    result: CheckResult::Ok(version),
                }
            } else {
                Check {
                    name: "Node.js",
                    result: CheckResult::Fail(format!(
                        "{version} — Node.js 20+ required. Install from https://nodejs.org"
                    )),
                }
            }
        }
        None => Check {
            name: "Node.js",
            result: CheckResult::Fail(
                "not found — Node.js 20+ required. Install from https://nodejs.org".into(),
            ),
        },
    }
}

async fn check_clarinet() -> Check {
    match version_output("clarinet", &["--version"]).await {
        Some(v) => {
            let version = v
                .trim()
                .trim_start_matches("clarinet ")
                .split_whitespace()
                .next()
                .unwrap_or("?")
                .to_string();
            let major: u32 = version
                .split('.')
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            if major < 3 {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Warn(format!(
                        "{version} — Clarinet 3.21+ required. \
                         Run: brew upgrade clarinet  OR  cargo install clarinet --locked"
                    )),
                }
            } else if meets_semver(&version, 3, 23) {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Ok(version),
                }
            } else if meets_semver(&version, 3, 21) {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Warn(format!(
                        "{version} — Clarinet 3.23+ recommended (Clarity 6 devnet / epoch 4.0 at burn 163). \
                         Run: brew upgrade clarinet"
                    )),
                }
            } else {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Warn(format!(
                        "{version} — Clarinet 3.21+ required. \
                         Run: brew upgrade clarinet  OR  cargo install clarinet --locked"
                    )),
                }
            }
        }
        None => Check {
            name: "Clarinet",
            result: CheckResult::Fail(
                "not found. Install: brew install clarinet  OR  cargo install clarinet".into(),
            ),
        },
    }
}

async fn check_docker() -> Check {
    // Probe for the binary by asking for its version — if it errors with
    // NotFound the binary isn't installed; any other outcome means it exists.
    let bin_exists = Command::new("docker")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    let found = !matches!(
        &bin_exists,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    );

    if !found {
        return Check {
            name: "Docker",
            result: CheckResult::Warn(
                "not found — only required for local devnet. Install from https://docker.com"
                    .into(),
            ),
        };
    }

    // Binary exists — check if the daemon is actually running
    let running = Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if running {
        let version = version_output("docker", &["--version"])
            .await
            .map(|v| {
                v.trim()
                    .trim_start_matches("Docker version ")
                    .split(',')
                    .next()
                    .unwrap_or("?")
                    .to_string()
            })
            .unwrap_or_else(|| "?".into());

        Check {
            name: "Docker",
            result: CheckResult::Ok(version),
        }
    } else {
        Check {
            name: "Docker",
            result: CheckResult::Warn(
                "not running — Start Docker Desktop first (required for devnet only)".into(),
            ),
        }
    }
}

async fn check_git() -> Check {
    match version_output("git", &["--version"]).await {
        Some(v) => {
            // "git version 2.44.0"
            let version = v.trim().trim_start_matches("git version ").to_string();
            Check {
                name: "git",
                result: CheckResult::Ok(version),
            }
        }
        None => Check {
            name: "git",
            result: CheckResult::Warn(
                "not found — optional but recommended. Install from https://git-scm.com".into(),
            ),
        },
    }
}

async fn check_devnet_epochs() -> Check {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(_) => {
            return Check {
                name: "Devnet epochs",
                result: CheckResult::Ok("could not read working directory".into()),
            };
        }
    };

    let Some(root) = find_scaffold_root(&cwd) else {
        return Check {
            name: "Devnet epochs",
            result: CheckResult::Ok("not in a scaffold project".into()),
        };
    };

    let path = root.join("contracts/settings/Devnet.toml");
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return Check {
            name: "Devnet epochs",
            result: CheckResult::Ok("no Devnet.toml".into()),
        };
    };

    if raw.contains("[epochs]") {
        return Check {
            name: "Devnet epochs",
            result: CheckResult::Warn(
                "Devnet.toml contains [epochs] — this overrides Clarinet 3.23+ defaults and can block epoch 4.0 / Clarity 6. Remove [epochs] or add epoch_4_0, then run stacksdapp clean.".into(),
            ),
        };
    }

    Check {
        name: "Devnet epochs",
        result: CheckResult::Ok("using Clarinet default epoch schedule".into()),
    }
}

async fn check_stacksdapp() -> Check {
    // Read the version baked into this binary at compile time
    let version = env!("CARGO_PKG_VERSION").to_string();
    Check {
        name: "stacksdapp",
        result: CheckResult::Ok(version),
    }
}

async fn check_deploy_mnemonics() -> Check {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(_) => {
            return Check {
                name: "Deploy mnemonics",
                result: CheckResult::Ok("could not read working directory".into()),
            };
        }
    };

    let Some(root) = find_scaffold_root(&cwd) else {
        return Check {
            name: "Deploy mnemonics",
            result: CheckResult::Ok("not in a scaffold project".into()),
        };
    };

    let mut warnings = Vec::new();
    let mut strict_failures = Vec::new();

    for network in ["testnet", "mainnet"] {
        let Some((path, check)) = inspect_settings_file(network, &root, true) else {
            continue;
        };
        let network_label = stacksdapp_shell::settings_relative_path(network);
        match check {
            MnemonicCheck::NotConfigured => {}
            MnemonicCheck::Ok => {}
            MnemonicCheck::PublicDevnet => {
                warnings.push(format!(
                    "{path}: public devnet mnemonic — use a fresh wallet ({network_label})"
                ));
            }
            MnemonicCheck::InvalidFormat(detail) => {
                strict_failures.push(format!("{path}: {detail}"));
            }
        }
    }

    if !strict_failures.is_empty() {
        return Check {
            name: "Deploy mnemonics",
            result: CheckResult::Warn(strict_failures.join("; ")),
        };
    }

    if !warnings.is_empty() {
        return Check {
            name: "Deploy mnemonics",
            result: CheckResult::Warn(warnings.join("; ")),
        };
    }

    Check {
        name: "Deploy mnemonics",
        result: CheckResult::Ok("testnet/mainnet settings look safe".into()),
    }
}

async fn check_git_hooks() -> Check {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(_) => {
            return Check {
                name: "Git hooks",
                result: CheckResult::Ok("could not read working directory".into()),
            };
        }
    };

    let Some(root) = find_scaffold_root(&cwd) else {
        return Check {
            name: "Git hooks",
            result: CheckResult::Ok("not in a scaffold project".into()),
        };
    };

    let hook_file = root.join(".githooks/pre-commit");
    if !hook_file.is_file() {
        return Check {
            name: "Git hooks",
            result: CheckResult::Warn(
                "mnemonic guard hook missing (.githooks/pre-commit). \
                 Run stacksdapp upgrade or recreate hooks from CONTRIBUTING.md"
                    .into(),
            ),
        };
    }

    if !root.join(".git").exists() {
        return Check {
            name: "Git hooks",
            result: CheckResult::Ok("scaffold project is not a git repository".into()),
        };
    }

    match git_config_hooks_path(&root).await {
        None => Check {
            name: "Git hooks",
            result: CheckResult::Warn(
                "core.hooksPath not set — mnemonic guard inactive. \
                 Run: npm run setup-hooks  OR  git config core.hooksPath .githooks"
                    .into(),
            ),
        },
        Some(path) if hooks_path_is_githooks(&path, &root) => Check {
            name: "Git hooks",
            result: CheckResult::Ok(".githooks".into()),
        },
        Some(path) => Check {
            name: "Git hooks",
            result: CheckResult::Warn(format!(
                "core.hooksPath is \"{path}\" (expected .githooks) — mnemonic guard may be inactive. \
                 Run: git config core.hooksPath .githooks"
            )),
        },
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// True when `git config core.hooksPath` points at this project's `.githooks` directory.
fn hooks_path_is_githooks(configured: &str, root: &Path) -> bool {
    let configured = configured.trim();
    if configured == ".githooks" {
        return true;
    }

    let expected = root.join(".githooks");
    if let (Ok(expected_canon), Ok(configured_canon)) = (
        expected.canonicalize(),
        Path::new(configured).canonicalize(),
    ) {
        return expected_canon == configured_canon;
    }

    // When canonicalize fails (path does not exist yet), compare normalized absolute-style paths.
    if let Ok(root_canon) = root.canonicalize() {
        let expected = root_canon.join(".githooks");
        let expected_display = expected.display().to_string().replace('\\', "/");
        let configured_norm = configured.replace('\\', "/");
        return configured_norm == expected_display;
    }

    false
}

async fn git_config_hooks_path(root: &Path) -> Option<String> {
    Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run a command and return its trimmed stdout, or None if it failed / not found.
async fn version_output(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Returns true if `version` (e.g. "1.78.0") is >= major.minor.
fn meets_semver(version: &str, req_major: u32, req_minor: u32) -> bool {
    let mut parts = version.split('.');
    let major: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let minor: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    (major, minor) >= (req_major, req_minor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn hooks_path_accepts_relative_githooks() {
        let root = PathBuf::from("/tmp/my-app");
        assert!(hooks_path_is_githooks(".githooks", &root));
    }

    #[test]
    fn hooks_path_rejects_unrelated_path() {
        let root = PathBuf::from("/tmp/my-app");
        assert!(!hooks_path_is_githooks(".husky", &root));
        assert!(!hooks_path_is_githooks("../other/.githooks", &root));
    }
}
