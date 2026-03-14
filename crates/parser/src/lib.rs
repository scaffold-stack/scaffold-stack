use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAbi {
    pub contract_id: String,
    pub contract_name: String,
    pub functions: Vec<AbiFunction>,
    pub variables: Vec<AbiVariable>,
    pub maps: Vec<AbiMap>,
    pub fungible_tokens: Vec<String>,
    pub non_fungible_tokens: Vec<AbiNft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiFunction {
    pub name: String,
    pub access: FunctionAccess,
    pub args: Vec<AbiArg>,
    pub outputs: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FunctionAccess {
    Public,
    ReadOnly,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiArg {
    pub name: String,
    pub r#type: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AbiType {
    Simple(String),
    // SDK emits "string-ascii" (hyphen) — rename to match
    StringAscii {
        #[serde(rename = "string-ascii")]
        string_ascii: StringLen,
    },
    // SDK emits "string-utf8" (hyphen) — rename to match
    StringUtf8 {
        #[serde(rename = "string-utf8")]
        string_utf8: StringLen,
    },
    // SDK emits { "buffer": { "length": N } }
    Buffer { buffer: StringLen },
    // Legacy { "buff": N }
    Buff { buff: u32 },
    List { list: ListDef },
    Tuple { tuple: Vec<TupleEntry> },
    Optional { optional: Box<AbiType> },
    Response { response: ResponseDef },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringLen {
    pub length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDef {
    pub r#type: Box<AbiType>,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TupleEntry {
    pub name: String,
    pub r#type: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseDef {
    pub ok: Box<AbiType>,
    pub error: Box<AbiType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiVariable {
    pub name: String,
    pub access: String,
    pub r#type: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiMap {
    pub name: String,
    pub key: AbiType,
    pub value: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiNft {
    pub name: String,
    pub r#type: AbiType,
}

pub async fn parse_project(contracts_dir: &Path) -> Result<Vec<ContractAbi>> {
    use tokio::process::Command;

    let clarinet_toml = contracts_dir.join("Clarinet.toml");
    if !clarinet_toml.exists() {
        return Err(anyhow!(
            "No scaffold-stacks project found. Run from the directory created by stacks-dapp new"
        ));
    }

    // The export-abi script lives in frontend/scripts/ but must be run
    // with CWD = contracts/ so that initSimnet() finds Clarinet.toml and
    // settings/Devnet.toml in the current directory — exactly where they are.
    let project_root = contracts_dir
        .parent()
        .ok_or_else(|| anyhow!("Invalid contracts path"))?;
    let script = project_root
        .join("frontend")
        .join("scripts")
        .join("export-abi.mjs");

    if !script.exists() {
        return Err(anyhow!(
            "ABI export script not found at {}. Re-scaffold or add frontend/scripts/export-abi.mjs.",
            script.display()
        ));
    }

    // Resolve the script path to an absolute path before changing CWD.
    let script_abs = script
        .canonicalize()
        .map_err(|e| anyhow!("Cannot resolve script path {}: {e}", script.display()))?;

    // Run from contracts/ so initSimnet() resolves Clarinet.toml + settings/Devnet.toml correctly.
    let output = Command::new("node")
        .arg(&script_abs)
        .current_dir(contracts_dir) // <-- KEY FIX: CWD must be contracts/
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("Node.js is required to export ABIs. Install from nodejs.org")
            } else {
                anyhow!("Failed to run export-abi script: {e}")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Failed to export contract ABIs. Run clarinet check to validate contracts.\n{}",
            if stderr.is_empty() {
                "Script exited non-zero.".to_string()
            } else {
                stderr.trim().to_string()
            }
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Print stderr always so the developer sees what the script actually said
    if !stderr.trim().is_empty() {
        eprintln!("[export-abi] {}", stderr.trim());
    }

    // initSimnet() writes status lines like "Updated deployment plan file"
    // to stdout before the JSON array. Find the first '[' and slice from there.
    let json_start = stdout.find('[').ok_or_else(|| anyhow!(
        "export-abi.mjs produced no JSON. Run: cd contracts && node ../frontend/scripts/export-abi.mjs\nOutput: {}",
        &stdout[..stdout.len().min(300)]
    ))?;
    let json = stdout[json_start..].trim();

    parse_abi_list(json)
}

/// Parse a JSON array of ContractAbi (e.g. from export-abi.mjs stdout).
pub fn parse_abi_list(json: &str) -> Result<Vec<ContractAbi>> {
    serde_json::from_str(json).map_err(|e| {
        anyhow!(
            "Failed to parse ABI JSON: {e}.
             First 200 chars of output: {}",
            &json[..json.len().min(200)]
        )
    })
}

/// Parse a single ABI JSON string (for testing).
pub fn parse_abi(json: &str) -> Result<ContractAbi> {
    let abi = serde_json::from_str(json)?;
    Ok(abi)
}

/// Map an AbiType into a TypeScript type string.
pub fn abi_type_to_ts(t: &AbiType) -> String {
    match t {
        AbiType::Simple(s) => match s.as_str() {
            "uint128" | "int128" => "bigint".to_string(),
            "bool" => "boolean".to_string(),
            "principal" => "string".to_string(),
            _ => "unknown".to_string(),
        },
        AbiType::StringAscii { .. } | AbiType::StringUtf8 { .. } => "string".to_string(),
        AbiType::Buffer { .. } | AbiType::Buff { .. } => "Uint8Array".to_string(),
        AbiType::List { list } => {
            let inner = abi_type_to_ts(&list.r#type);
            format!("Array<{inner}>")
        }
        AbiType::Tuple { tuple } => {
            let fields: Vec<String> = tuple
                .iter()
                .map(|e| format!("{}: {}", e.name, abi_type_to_ts(&e.r#type)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        AbiType::Optional { optional } => format!("{} | null", abi_type_to_ts(optional)),
        AbiType::Response { response } => {
            let ok = abi_type_to_ts(&response.ok);
            let err = abi_type_to_ts(&response.error);
            format!("{{ ok: {ok} }} | {{ error: {err} }}")
        }
    }
}