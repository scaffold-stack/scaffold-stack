use anyhow::Result;
use sha2::{Digest, Sha256};
use stacksdapp_parser::ContractAbi;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tera::{Filter, Tera, Value};

const CONTRACTS_TS_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/contracts.ts.tera"
));
const HOOKS_TS_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/hooks.ts.tera"
));
const DEBUG_UI_TSX_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/debug_ui.tsx.tera"
));

// ── Custom Tera filters ───────────────────────────────────────────────────────

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for (i, ch) in s.chars().enumerate() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else if i == 0 {
            result.extend(ch.to_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

fn to_upper_camel_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

struct CamelFilter;
impl Filter for CamelFilter {
    fn filter(&self, value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
        match value.as_str() {
            Some(s) => Ok(Value::String(to_camel_case(s))),
            None => Err(tera::Error::msg("camel filter: expected string")),
        }
    }
}

struct UpperCamelFilter;
impl Filter for UpperCamelFilter {
    fn filter(&self, value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
        match value.as_str() {
            Some(s) => Ok(Value::String(to_upper_camel_case(s))),
            None => Err(tera::Error::msg("upper_camel filter: expected string")),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn generate_all() -> Result<()> {
    generate_all_impl(false).await
}

/// Same as [`generate_all`] but suppresses progress logs (for nested CLI steps).
pub async fn generate_all_quiet() -> Result<()> {
    generate_all_impl(true).await
}

async fn generate_all_impl(quiet: bool) -> Result<()> {
    let project_root = std::env::current_dir()?;
    let contracts_dir = project_root.join("contracts");
    if !contracts_dir.join("Clarinet.toml").exists()
        || !project_root.join("frontend/package.json").exists()
    {
        anyhow::bail!(
            "No scaffold-stacks project found. Run from the directory created by stacksdapp new"
        );
    }

    let log = |msg: String| {
        if !quiet {
            println!("{msg}");
        }
    };

    let frontend_dir = project_root.join("frontend");
    if !frontend_dir.join("node_modules").exists() {
        log("[generate] Installing frontend dependencies...".into());
        let subcommand = if frontend_dir.join("package-lock.json").exists() {
            "ci"
        } else {
            "install"
        };
        let status = tokio::process::Command::new("npm")
            .arg(subcommand)
            .args([
                "--no-audit",
                "--no-fund",
                "--prefer-offline",
                "--progress=false",
                "--loglevel=error",
            ])
            .current_dir(&frontend_dir)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("npm install in frontend/ failed.");
        }
    }

    log("[generate] Parsing contract ABIs...".into());
    let abis = stacksdapp_parser::parse_project(&contracts_dir).await?;

    let out_dir = project_root.join("frontend/src/generated");
    tokio::fs::create_dir_all(&out_dir).await?;

    let deployments_path = out_dir.join("deployments.json");
    if !deployments_path.exists() {
        tokio::fs::write(
            &deployments_path,
            r#"{ "network": "", "deployed_at": "", "contracts": {} }"#,
        )
        .await?;
        log("[generate] Created empty deployments.json (run stacksdapp deploy to populate)".into());
    }

    if abis.is_empty() {
        let written = render_with_quiet(&abis, &out_dir, quiet)?;
        if written == 0 {
            log("[generate] No user contracts found in Clarinet.toml — generated stubs already up to date.".into());
        } else {
            log("[generate] No user contracts found in Clarinet.toml — wrote empty generated stubs.".into());
        }
        return Ok(());
    }

    log(format!(
        "[generate] Found {} contract(s): {}",
        abis.len(),
        abis.iter()
            .map(|a| a.contract_name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let written = render_with_quiet(&abis, &out_dir, quiet)?;

    if written == 0 {
        log("[generate] All files already up to date.".into());
    } else {
        log(format!("[generate] Done — {written} file(s) written."));
    }

    let network = std::env::var("NEXT_PUBLIC_NETWORK").unwrap_or_else(|_| "<network>".into());
    let stale = find_stale_deployments(&abis, &out_dir);
    if !stale.is_empty() && !quiet {
        warn_redeploy_required(&stale, &network);
    }

    Ok(())
}

/// Render all templates. Returns the number of files actually written.
pub fn render(abis: &[ContractAbi], out_dir: &Path) -> Result<usize> {
    render_with_quiet(abis, out_dir, false)
}

fn render_with_quiet(abis: &[ContractAbi], out_dir: &Path, quiet: bool) -> Result<usize> {
    let mut tera = Tera::default();
    tera.register_filter("camel", CamelFilter);
    tera.register_filter("upper_camel", UpperCamelFilter);

    tera.add_raw_template("contracts.ts.tera", CONTRACTS_TS_TEMPLATE)?;
    tera.add_raw_template("hooks.ts.tera", HOOKS_TS_TEMPLATE)?;
    tera.add_raw_template("debug_ui.tsx.tera", DEBUG_UI_TSX_TEMPLATE)?;

    // Serialize ABIs and enrich each function arg with a `type_str` field —
    // a simple lowercase Clarity type string (e.g. "uint128", "bool", "principal",
    // "string-ascii", "string-utf8", "buff") used by the debug UI to build
    // typed inputs and call toClarityValue() correctly.
    let contracts_json: Vec<serde_json::Value> = abis
        .iter()
        .map(|c| {
            let mut val = serde_json::to_value(c).expect("ContractAbi serialization failed");
            if let Some(fns) = val["functions"].as_array_mut() {
                for f in fns.iter_mut() {
                    if let Some(args) = f["args"].as_array_mut() {
                        for arg in args.iter_mut() {
                            let type_str = clarity_type_str(&arg["type"]);
                            arg["type_str"] = serde_json::Value::String(type_str);
                        }
                    }
                }
            }
            val
        })
        .collect();

    let ctx = tera::Context::from_serialize(serde_json::json!({
        "contracts": contracts_json
    }))?;

    let mut written = 0;
    written += write_if_changed(
        out_dir.join("contracts.ts"),
        &tera.render("contracts.ts.tera", &ctx)?,
        quiet,
    )?;
    written += write_if_changed(
        out_dir.join("hooks.ts"),
        &tera.render("hooks.ts.tera", &ctx)?,
        quiet,
    )?;
    written += write_if_changed(
        out_dir.join("DebugContracts.tsx"),
        &tera.render("debug_ui.tsx.tera", &ctx)?,
        quiet,
    )?;

    Ok(written)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a serialized AbiType JSON value into a simple Clarity type string
/// for use in the debug UI. e.g. uint128 → "uint128", string-ascii → "string-ascii"
fn clarity_type_str(t: &serde_json::Value) -> String {
    match t {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            if map.contains_key("string-ascii") {
                return "string-ascii".into();
            }
            if map.contains_key("string-utf8") {
                return "string-utf8".into();
            }
            if map.contains_key("buffer") {
                return "buff".into();
            }
            if map.contains_key("buff") {
                return "buff".into();
            }
            if map.contains_key("list") {
                return "list".into();
            }
            if map.contains_key("tuple") {
                return "tuple".into();
            }
            if map.contains_key("optional") {
                return "optional".into();
            }
            if map.contains_key("response") {
                return "response".into();
            }
            "unknown".into()
        }
        _ => "unknown".into(),
    }
}

fn hash_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().to_vec()
}

fn find_stale_deployments(abis: &[ContractAbi], out_dir: &Path) -> Vec<String> {
    let deployments_path = out_dir.join("deployments.json");
    let Ok(raw) = std::fs::read_to_string(&deployments_path) else {
        return vec![]; // no deployments file — nothing to compare
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return vec![];
    };
    stale_contract_names(abis, &json)
}

/// Contracts that were previously deployed under a *different* on-chain name
/// (e.g. version bump `counter` → `counter-v2`). Undeployed / empty deployments
/// are not stale — they just have not been deployed yet.
fn stale_contract_names(abis: &[ContractAbi], json: &serde_json::Value) -> Vec<String> {
    let network = json["network"].as_str().unwrap_or("").trim();
    let Some(deployed) = json["contracts"].as_object() else {
        return vec![];
    };
    // Fresh scaffold / post-clean: not "out of sync".
    if network.is_empty() || deployed.is_empty() {
        return vec![];
    }

    abis.iter()
        .filter_map(|abi| {
            let entry = deployed.get(&abi.contract_name)?;
            let deployed_id = entry["contract_id"].as_str().unwrap_or("").trim();
            if deployed_id.is_empty() {
                return None;
            }
            if deployment_id_matches(deployed_id, &abi.contract_name) {
                None
            } else {
                Some(abi.contract_name.clone())
            }
        })
        .collect()
}

/// `ST….counter` or bare `counter` matches local name `counter`.
fn deployment_id_matches(deployed_id: &str, contract_name: &str) -> bool {
    match deployed_id.rsplit_once('.') {
        Some((_, name)) => name == contract_name,
        None => deployed_id == contract_name,
    }
}

/// Write file only if content changed. Returns 1 if written, 0 if skipped.
fn write_if_changed(path: PathBuf, contents: &str, quiet: bool) -> Result<usize> {
    let new_bytes = contents.as_bytes();
    let new_hash = hash_bytes(new_bytes);

    if let Ok(existing) = fs::read(&path) {
        if hash_bytes(&existing) == new_hash {
            return Ok(0);
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(&path)?;
    file.write_all(new_bytes)?;
    if !quiet {
        println!("[generated] {}", path.display());
    }
    Ok(1)
}

/// Print a prominent redeployment warning.
fn warn_redeploy_required(stale: &[String], network: &str) {
    let names = stale.join(", ");
    eprintln!("\n{}", "━".repeat(60));
    eprintln!("  ⚠  REDEPLOYMENT REQUIRED");
    eprintln!("{}", "━".repeat(60));
    eprintln!("  On-chain contract ids no longer match local names:");
    eprintln!("  {}", names);
    eprintln!();
    eprintln!("  Clarity contracts are immutable. Redeploy so bindings");
    eprintln!("  point at the current contract ids:");
    eprintln!();
    eprintln!("    stacksdapp deploy --network {network}");
    eprintln!("    where network is either devnet/testnet/mainnet");
    eprintln!();
    eprintln!("  Until then, calls to renamed/versioned contracts will fail.");
    eprintln!("{}\n", "━".repeat(60));
}

#[cfg(test)]
mod tests {
    use super::*;
    use stacksdapp_parser::ContractAbi;

    fn abi(name: &str) -> ContractAbi {
        ContractAbi {
            contract_id: format!(".{}", name),
            contract_name: name.to_string(),
            functions: vec![],
            variables: vec![],
            maps: vec![],
            fungible_tokens: vec![],
            non_fungible_tokens: vec![],
        }
    }

    #[test]
    fn empty_deployments_are_not_stale() {
        let json = serde_json::json!({
            "network": "",
            "deployed_at": "",
            "contracts": {}
        });
        let stale = stale_contract_names(&[abi("counter")], &json);
        assert!(stale.is_empty(), "fresh project must not warn: {stale:?}");
    }

    #[test]
    fn matching_deployment_is_not_stale() {
        let json = serde_json::json!({
            "network": "devnet",
            "deployed_at": "2026-01-01T00:00:00Z",
            "contracts": {
                "counter": {
                    "contract_id": "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.counter",
                    "tx_id": "0xabc",
                    "block_height": 1
                }
            }
        });
        let stale = stale_contract_names(&[abi("counter")], &json);
        assert!(stale.is_empty(), "in-sync deploy must not warn: {stale:?}");
    }

    #[test]
    fn renamed_deployment_is_stale() {
        let json = serde_json::json!({
            "network": "devnet",
            "deployed_at": "2026-01-01T00:00:00Z",
            "contracts": {
                "counter": {
                    "contract_id": "ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM.counter-v2",
                    "tx_id": "0xabc",
                    "block_height": 1
                }
            }
        });
        let stale = stale_contract_names(&[abi("counter")], &json);
        assert_eq!(stale, vec!["counter".to_string()]);
    }

    #[test]
    fn undeployed_sibling_is_not_stale() {
        // New local contract while others are deployed — not a rename mismatch.
        let json = serde_json::json!({
            "network": "devnet",
            "contracts": {
                "counter": {
                    "contract_id": "ST1.counter",
                    "tx_id": "0x1",
                    "block_height": 1
                }
            }
        });
        let stale = stale_contract_names(&[abi("counter"), abi("hello-token")], &json);
        assert!(
            stale.is_empty(),
            "missing entry is undeployed, not stale: {stale:?}"
        );
    }

    #[test]
    fn deployment_id_match_is_exact_suffix() {
        assert!(deployment_id_matches("ST1.counter", "counter"));
        assert!(!deployment_id_matches("ST1.my-counter", "counter"));
        assert!(deployment_id_matches("counter", "counter"));
    }

    #[test]
    fn render_writes_empty_generated_stubs() {
        let tmp = tempfile::tempdir().unwrap();
        let written = render(&[], tmp.path()).unwrap();
        assert_eq!(written, 3);
        assert!(tmp.path().join("contracts.ts").is_file());
        assert!(tmp.path().join("hooks.ts").is_file());
        assert!(tmp.path().join("DebugContracts.tsx").is_file());
    }
}
