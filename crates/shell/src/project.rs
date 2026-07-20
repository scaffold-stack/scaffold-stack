//! Project root discovery

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PROJECT_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub const CONFIG_FILE: &str = "stacksdapp.toml";

/// Optional project config loaded from `stacksdapp.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StacksdappConfig {
    pub project: Option<ProjectSection>,
    pub defaults: Option<DefaultsSection>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ProjectSection {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DefaultsSection {
    /// Default network for deploy/dev when not passed on the CLI (reserved for future use).
    pub network: Option<String>,
}

/// Walk upward from `start` looking for a scaffold-stacks project root.
///
/// A directory qualifies if it contains either:
/// - `stacksdapp.toml`, or
/// - `contracts/Clarinet.toml` (scaffold-stacks layout)
pub fn find_scaffold_root(start: &Path) -> Option<PathBuf> {
    walk_up(start, is_scaffold_root)
}

/// Walk upward for scaffold root **or** a standard Clarinet root (`Clarinet.toml`).
/// Used by `stacksdapp init`.
pub fn find_init_root(start: &Path) -> Option<PathBuf> {
    walk_up(start, |dir| {
        is_scaffold_root(dir) || dir.join("Clarinet.toml").is_file()
    })
}

fn is_scaffold_root(dir: &Path) -> bool {
    dir.join(CONFIG_FILE).is_file() || dir.join("contracts").join("Clarinet.toml").is_file()
}

fn walk_up(start: &Path, predicate: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut dir = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };

    // Best-effort canonicalize; keep walking even if it fails (e.g. missing path).
    if let Ok(canon) = dir.canonicalize() {
        dir = canon;
    }

    loop {
        if predicate(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolve project root: explicit override, else walk-up from cwd.
pub fn resolve_scaffold_root(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        let root = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let root = root
            .canonicalize()
            .map_err(|e| format!("Invalid --root '{}': {e}", path.display()))?;
        if !is_scaffold_root(&root) {
            return Err(format!(
                "Directory '{}' is not a stacksdapp project (expected {CONFIG_FILE} or contracts/Clarinet.toml).",
                root.display()
            ));
        }
        return Ok(root);
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    find_scaffold_root(&cwd).ok_or_else(|| {
        format!(
            "No stacksdapp project found above '{}'.\n\
             Looked for {CONFIG_FILE} or contracts/Clarinet.toml.\n\
             Run `stacksdapp new <name>`, `stacksdapp init`, or pass --root <path>.",
            cwd.display()
        )
    })
}

/// `chdir` into the project root and remember it for [`project_root`].
pub fn enter_scaffold_root(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let root = resolve_scaffold_root(explicit)?;
    std::env::set_current_dir(&root)
        .map_err(|e| format!("Failed to enter project root '{}': {e}", root.display()))?;
    let _ = PROJECT_ROOT.set(root.clone());
    Ok(root)
}

/// Last root entered via [`enter_scaffold_root`], if any.
pub fn project_root() -> Option<&'static Path> {
    PROJECT_ROOT.get().map(|p| p.as_path())
}

/// Default network for deploy/dev when not passed on the CLI.
pub fn validate_network(network: &str) -> Result<(), String> {
    match network {
        "devnet" | "testnet" | "mainnet" => Ok(()),
        other => Err(format!(
            "Invalid network '{other}'. Expected one of: devnet | testnet | mainnet"
        )),
    }
}

/// Load `stacksdapp.toml` from `root` if present (missing file → default config).
pub fn load_config(root: &Path) -> Result<StacksdappConfig, String> {
    let path = root.join(CONFIG_FILE);
    if !path.is_file() {
        return Ok(StacksdappConfig::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    toml::from_str(&raw).map_err(|e| format!("Failed to parse {}: {e}", path.display()))
}

/// Default contents for a new `stacksdapp.toml`.
pub fn default_config_toml(project_name: &str) -> String {
    format!(
        r#"# stacksdapp project marker — enables root walk-up from subdirectories.
# https://github.com/scaffold-stack/scaffold-stack

[project]
name = "{project_name}"

# [defaults]
# network = "devnet"
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_root_via_clarinet_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(nested.join("x")).unwrap();
        fs::create_dir_all(tmp.path().join("contracts")).unwrap();
        fs::write(
            tmp.path().join("contracts/Clarinet.toml"),
            "[project]\nname=\"t\"\n",
        )
        .unwrap();

        let found = find_scaffold_root(&nested.join("x")).unwrap();
        assert_eq!(found, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn prefers_stacksdapp_toml_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("frontend").join("src");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "[project]\nname=\"demo\"\n").unwrap();

        let found = find_scaffold_root(&sub).unwrap();
        assert_eq!(found, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("empty");
        fs::create_dir_all(&sub).unwrap();
        assert!(find_scaffold_root(&sub).is_none());
    }

    #[test]
    fn load_config_parses_name() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(CONFIG_FILE),
            "[project]\nname = \"my-dapp\"\n\n[defaults]\nnetwork = \"testnet\"\n",
        )
        .unwrap();
        let cfg = load_config(tmp.path()).unwrap();
        assert_eq!(cfg.project.unwrap().name.as_deref(), Some("my-dapp"));
        assert_eq!(cfg.defaults.unwrap().network.as_deref(), Some("testnet"));
    }

    #[test]
    fn init_root_finds_standard_clarinet() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("contracts");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.path().join("Clarinet.toml"), "[project]\nname=\"c\"\n").unwrap();
        let found = find_init_root(&sub).unwrap();
        assert_eq!(found, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn validate_network_accepts_known_values() {
        assert!(validate_network("devnet").is_ok());
        assert!(validate_network("testnet").is_ok());
        assert!(validate_network("mainnet").is_ok());
    }

    #[test]
    fn validate_network_rejects_unknown_values() {
        assert!(validate_network("staging").is_err());
        assert!(validate_network("").is_err());
    }
}
