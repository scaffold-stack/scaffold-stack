use anyhow::Result;
use colored::Colorize;
use std::process::Stdio;
use tokio::process::Command;

struct Check {
    name: &'static str,
    result: CheckResult,
}

enum CheckResult {
    Ok(String),
    Warn(String),
    Fail(String),
}

pub async fn run() -> Result<()> {
    println!(
        "\n{}\n",
        "stacksdapp doctor — checking prerequisites".bold()
    );

    let checks = vec![
        check_rust().await,
        check_node().await,
        check_clarinet().await,
        check_docker().await,
        check_git().await,
        check_stacksdapp().await,
    ];

    let mut all_ok = true;

    for check in &checks {
        match &check.result {
            CheckResult::Ok(msg) => {
                println!(
                    "  {}  {} {}",
                    "✔".green().bold(),
                    check.name.white(),
                    msg.dimmed()
                );
            }
            CheckResult::Warn(msg) => {
                println!(
                    "  {}  {} {}",
                    "⚠".yellow().bold(),
                    check.name.white(),
                    msg.yellow()
                );
                all_ok = false;
            }
            CheckResult::Fail(msg) => {
                println!(
                    "  {}  {} {}",
                    "✗".red().bold(),
                    check.name.white().bold(),
                    msg.red()
                );
                all_ok = false;
            }
        }
    }

    println!();

    if all_ok {
        println!(
            "{}",
            "  All checks passed. You're ready to build on Stacks!"
                .green()
                .bold()
        );
    } else {
        println!(
            "{}",
            "  Some checks failed. Fix the issues above before running stacksdapp new.".yellow()
        );
    }

    println!();
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
            // "clarinet 3.14.1" or just "3.14.1"
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
            if major >= 3 {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Ok(version),
                }
            } else {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Warn(format!(
                        "{version} — Clarinet 3.x required. \
                         Run: brew install clarinet  OR  cargo install clarinet"
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

    let found = !matches!(&bin_exists, Err(e) if e.kind() == std::io::ErrorKind::NotFound);

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

async fn check_stacksdapp() -> Check {
    // Read the version baked into this binary at compile time
    let version = env!("CARGO_PKG_VERSION").to_string();
    Check {
        name: "stacksdapp",
        result: CheckResult::Ok(version),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
