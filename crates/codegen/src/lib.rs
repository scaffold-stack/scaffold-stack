use anyhow::Result;
use parser::ContractAbi;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tera::{Filter, Tera, Value};

const CONTRACTS_TS_TEMPLATE: &str = include_str!("../../../templates/contracts.ts.tera");
const HOOKS_TS_TEMPLATE: &str = include_str!("../../../templates/hooks.ts.tera");
const DEBUG_UI_TSX_TEMPLATE: &str = include_str!("../../../templates/debug_ui.tsx.tera");

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
    let project_root = std::env::current_dir()?;
    let contracts_dir = project_root.join("contracts");
    if !contracts_dir.join("Clarinet.toml").exists()
        || !project_root.join("frontend/package.json").exists()
    {
        anyhow::bail!(
            "No scaffold-stacks project found. Run from the directory created by stacks-dapp new"
        );
    }

    let frontend_dir = project_root.join("frontend");
    if !frontend_dir.join("node_modules").exists() {
        println!("Installing frontend dependencies (npm install)...");
        let status = tokio::process::Command::new("npm")
            .arg("install")
            .current_dir(&frontend_dir)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("npm install in frontend/ failed.");
        }
    }

    println!("[generate] Parsing contract ABIs...");
    let abis = parser::parse_project(&contracts_dir).await?;

    if abis.is_empty() {
        println!("[generate] No user contracts found in Clarinet.toml — nothing to generate.");
        return Ok(());
    }

    println!("[generate] Found {} contract(s): {}", abis.len(),
        abis.iter().map(|a| a.contract_name.as_str()).collect::<Vec<_>>().join(", "));

    let out_dir = project_root.join("frontend/src/generated");
    tokio::fs::create_dir_all(&out_dir).await?;

    // Write empty deployments.json if it doesn't exist yet so that
    // contracts.ts can always require() it without crashing at import time.
    // The real content is written by `stacks-dapp deploy`.
    let deployments_path = out_dir.join("deployments.json");
    if !deployments_path.exists() {
        tokio::fs::write(
            &deployments_path,
            r#"{ "network": "", "deployed_at": "", "contracts": {} }"#,
        ).await?;
        println!("[generate] Created empty deployments.json (run stacks-dapp deploy to populate)");
    }

    let written = render(&abis, &out_dir)?;

    if written == 0 {
        println!("[generate] All files already up to date.");
    } else {
        println!("[generate] Done — {written} file(s) written.");
    }

    Ok(())
}

/// Render all templates. Returns the number of files actually written.
pub fn render(abis: &[ContractAbi], out_dir: &Path) -> Result<usize> {
    let mut tera = Tera::default();
    tera.register_filter("camel", CamelFilter);
    tera.register_filter("upper_camel", UpperCamelFilter);

    tera.add_raw_template("contracts.ts.tera", CONTRACTS_TS_TEMPLATE)?;
    tera.add_raw_template("hooks.ts.tera", HOOKS_TS_TEMPLATE)?;
    tera.add_raw_template("debug_ui.tsx.tera", DEBUG_UI_TSX_TEMPLATE)?;

    let contracts_json: Vec<serde_json::Value> = abis
        .iter()
        .map(|c| serde_json::to_value(c).expect("ContractAbi serialization failed"))
        .collect();

    let ctx = tera::Context::from_serialize(serde_json::json!({
        "contracts": contracts_json
    }))?;

    let mut written = 0;
    written += write_if_changed(out_dir.join("contracts.ts"), &tera.render("contracts.ts.tera", &ctx)?)?;
    written += write_if_changed(out_dir.join("hooks.ts"), &tera.render("hooks.ts.tera", &ctx)?)?;
    written += write_if_changed(out_dir.join("DebugContracts.tsx"), &tera.render("debug_ui.tsx.tera", &ctx)?)?;

    Ok(written)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hash_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().to_vec()
}

/// Write file only if content changed. Returns 1 if written, 0 if skipped.
fn write_if_changed(path: PathBuf, contents: &str) -> Result<usize> {
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
    println!("[generated] {}", path.display());
    Ok(1)
}