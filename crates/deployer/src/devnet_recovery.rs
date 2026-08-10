//! Local Clarinet devnet recovery when Bitcoin burn advances but Stacks tip stalls
//! (PoX / Nakamoto tenure desync around reward-cycle boundaries).

use anyhow::{anyhow, Result};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalCoreInfo {
    pub stacks_tip_height: u64,
    pub burn_block_height: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryLevel {
    /// Restart stacks-signer + stacks-node only.
    StacksOnly,
    /// Restart bitcoin-node, then stacks-signer + stacks-node.
    WithBitcoin,
    /// Restart all devnet containers (bitcoin, postgres, stacks-node, signer).
    Full,
}

impl RecoveryLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::StacksOnly => "stacks-node + signer",
            Self::WithBitcoin => "bitcoin-node + stacks-node + signer",
            Self::Full => "full devnet stack",
        }
    }
}

/// True when Stacks tip is unchanged while burn height increased (classic devnet stall).
pub fn is_devnet_chain_stalled(previous: &LocalCoreInfo, current: &LocalCoreInfo) -> bool {
    current.stacks_tip_height == previous.stacks_tip_height
        && current.burn_block_height > previous.burn_block_height
}

pub fn devnet_project_name() -> Option<String> {
    let raw = std::fs::read_to_string("contracts/Clarinet.toml").ok()?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name = ") {
            return trimmed
                .trim_start_matches("name = ")
                .trim()
                .trim_matches('"')
                .to_string()
                .into();
        }
    }
    None
}

pub async fn fetch_local_core_info_optional() -> Option<LocalCoreInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let response = client
        .get("http://localhost:20443/v2/info")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().await.ok()?;
    Some(LocalCoreInfo {
        stacks_tip_height: json.get("stacks_tip_height")?.as_u64()?,
        burn_block_height: json.get("burn_block_height")?.as_u64()?,
    })
}

fn docker_restart_container(name: &str) -> bool {
    let id = docker_container_id(name);
    let Some(id) = id else {
        return false;
    };
    std::process::Command::new("docker")
        .args(["restart", &id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn docker_container_id(name: &str) -> Option<String> {
    std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", &format!("name={name}")])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().lines().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
}

fn docker_stop_container(name: &str) -> bool {
    let Some(id) = docker_container_id(name) else {
        return false;
    };
    std::process::Command::new("docker")
        .args(["stop", &id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn docker_start_container(name: &str) -> bool {
    let id = std::process::Command::new("docker")
        .args(["ps", "-aq", "--filter", &format!("name={name}")])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().lines().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty());
    let Some(id) = id else {
        return false;
    };
    std::process::Command::new("docker")
        .args(["start", &id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Stop all devnet containers for a project (used before Clarinet respawn).
pub fn stop_devnet_containers(project: &str) -> bool {
    let names = [
        format!("stacks-signer-0.{project}.devnet"),
        format!("stacks-node.{project}.devnet"),
        format!("postgres.{project}.devnet"),
        format!("bitcoin-node.{project}.devnet"),
    ];
    let mut any = false;
    for name in &names {
        if docker_stop_container(name) {
            any = true;
        }
    }
    any
}
/// Restart devnet Docker containers for `project`. Returns true if anything restarted.
pub fn try_recover_devnet_chain(project: &str, level: RecoveryLevel) -> bool {
    let stacks = [
        format!("stacks-signer-0.{project}.devnet"),
        format!("stacks-node.{project}.devnet"),
    ];
    let bitcoin = format!("bitcoin-node.{project}.devnet");
    let postgres = format!("postgres.{project}.devnet");

    let mut any = false;
    match level {
        RecoveryLevel::StacksOnly => {
            for name in &stacks {
                if docker_restart_container(name) {
                    any = true;
                }
            }
        }
        RecoveryLevel::WithBitcoin => {
            if docker_restart_container(&bitcoin) {
                any = true;
            }
            std::thread::sleep(Duration::from_secs(5));
            for name in &stacks {
                if docker_restart_container(name) {
                    any = true;
                }
            }
        }
        RecoveryLevel::Full => {
            // Stop/start (not restart) resets container processes while Clarinet's
            // bitcoin controller reconnects — more reliable than restart alone.
            for name in [&stacks[0], &stacks[1], &postgres, &bitcoin] {
                docker_stop_container(name);
            }
            std::thread::sleep(Duration::from_secs(3));
            for name in [&bitcoin, &postgres, &stacks[1], &stacks[0]] {
                if docker_start_container(name) {
                    any = true;
                }
            }
        }
    }
    any
}

async fn wait_for_stacks_tip_above(min_exclusive: u64, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Some(info) = fetch_local_core_info_optional().await {
            if info.stacks_tip_height > min_exclusive {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    false
}

/// Block until Stacks tip advances, attempting escalating Docker recovery on stall.
/// Devnet-only — callers must gate on `network == "devnet"`.
pub async fn ensure_devnet_chain_mining(context: &str) -> Result<()> {
    let project = devnet_project_name().ok_or_else(|| {
        anyhow!(
            "Cannot recover devnet: missing contracts/Clarinet.toml project name.\n\
             Run deploy from a scaffold project root with `stacksdapp dev` running."
        )
    })?;

    let first = fetch_local_core_info_optional()
        .await
        .ok_or_else(|| anyhow!("Local stacks-node at http://localhost:20443 is not responding."))?;

    // Fast path: tip advancing normally (typical deploy within ~2 min of dev ready).
    tokio::time::sleep(Duration::from_secs(8)).await;
    if let Some(second) = fetch_local_core_info_optional().await {
        if second.stacks_tip_height > first.stacks_tip_height {
            return Ok(());
        }
        if !is_devnet_chain_stalled(&first, &second)
            && wait_for_stacks_tip_above(first.stacks_tip_height, Duration::from_secs(45)).await
        {
            return Ok(());
        }
        // Stalled sample — fall through to recovery using latest observation.
        return recover_devnet_chain_mining(&project, context, &second).await;
    }

    recover_devnet_chain_mining(&project, context, &first).await
}

async fn recover_devnet_chain_mining(
    project: &str,
    context: &str,
    stalled_at: &LocalCoreInfo,
) -> Result<()> {
    stacksdapp_shell::println_human_safe(format!(
        "[deploy] Devnet chain stalled (Stacks #{}, burn {}) before {context}.",
        stalled_at.stacks_tip_height, stalled_at.burn_block_height
    ));

    let levels = [
        RecoveryLevel::StacksOnly,
        RecoveryLevel::WithBitcoin,
        RecoveryLevel::Full,
    ];
    for (idx, level) in levels.iter().enumerate() {
        stacksdapp_shell::println_human_safe(format!(
            "[deploy] Recovering stalled chain (attempt {}/3): {} for {project}...",
            idx + 1,
            level.label()
        ));
        try_recover_devnet_chain(project, *level);
        tokio::time::sleep(Duration::from_secs(12)).await;
        if wait_for_stacks_tip_above(stalled_at.stacks_tip_height, Duration::from_secs(120)).await {
            stacksdapp_shell::println_human_safe(
                "[deploy] Devnet chain recovered — Stacks tip is advancing again.",
            );
            return Ok(());
        }
    }

    Err(anyhow!(
        "Devnet chain stalled at Stacks #{} (burn {}) and recovery did not restore block production.\n\
         If `stacksdapp dev` is running, wait for \"[devnet] Clarinet devnet restarted\" then retry.\n\
         Otherwise run `stacksdapp clean --force`, restart with `stacksdapp dev --auto-deploy`, and deploy while the tip advances.\n\
         If this repeats, try `bitcoin_controller_block_time = 60_000` in contracts/settings/Devnet.toml.",
        stalled_at.stacks_tip_height,
        stalled_at.burn_block_height
    ))
}

/// Poll during contract confirmation; trigger recovery if burn advances without new Stacks blocks.
pub async fn recover_devnet_if_stalled_during_wait(
    previous: &LocalCoreInfo,
    project: Option<&str>,
) -> bool {
    let Some(current) = fetch_local_core_info_optional().await else {
        return false;
    };
    if !is_devnet_chain_stalled(previous, &current) {
        return false;
    }
    let Some(name) = project.map(str::to_string).or_else(devnet_project_name) else {
        return false;
    };
    stacksdapp_shell::println_human_safe(format!(
        "[deploy] Chain stalled while waiting (Stacks #{}, burn {}) — restarting devnet containers...",
        current.stacks_tip_height, current.burn_block_height
    ));
    try_recover_devnet_chain(&name, RecoveryLevel::WithBitcoin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_stall_when_burn_advances_one_block() {
        let prev = LocalCoreInfo {
            stacks_tip_height: 71,
            burn_block_height: 176,
        };
        let curr = LocalCoreInfo {
            stacks_tip_height: 71,
            burn_block_height: 177,
        };
        assert!(is_devnet_chain_stalled(&prev, &curr));
    }

    #[test]
    fn not_stalled_when_stacks_advances() {
        let prev = LocalCoreInfo {
            stacks_tip_height: 71,
            burn_block_height: 176,
        };
        let curr = LocalCoreInfo {
            stacks_tip_height: 72,
            burn_block_height: 177,
        };
        assert!(!is_devnet_chain_stalled(&prev, &curr));
    }

    #[test]
    fn not_stalled_when_both_unchanged() {
        let prev = LocalCoreInfo {
            stacks_tip_height: 71,
            burn_block_height: 176,
        };
        let curr = LocalCoreInfo {
            stacks_tip_height: 71,
            burn_block_height: 176,
        };
        assert!(!is_devnet_chain_stalled(&prev, &curr));
    }
}
