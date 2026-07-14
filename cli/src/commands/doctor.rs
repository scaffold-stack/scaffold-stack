use anyhow::Result;
use colored::Colorize;
use serde_json::json;
use stacksdapp_shell::{self as shell, status};
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
            } else if meets_semver(&version, 3, 21) {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Ok(version),
                }
            } else {
                Check {
                    name: "Clarinet",
                    result: CheckResult::Warn(format!(
                        "{version} — Clarinet 3.21+ recommended (templates target 3.21). \
                         Run: brew upgrade clarinet"
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

    let found = match &bin_exists {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        _ => true,
    };

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
