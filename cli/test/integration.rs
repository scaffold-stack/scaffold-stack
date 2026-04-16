
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a `Command` for the `stacksdapp` binary under test.
fn cli() -> Command {
    Command::cargo_bin("stacksdapp").expect("binary stacksdapp not found")
}

/// Returns true if both `node` (>=20) and `clarinet` are on PATH.
fn has_toolchain() -> bool {
    which::which("node").is_ok() && which::which("clarinet").is_ok()
}

/// Scaffold a new project inside `dir` with the given name.
/// Uses `--no-git` so tests don't depend on global git config.
/// The generous timeout accounts for `npm install` in CI environments.
fn scaffold_project(dir: &Path, name: &str) {
    cli()
        .current_dir(dir)
        .args(["new", name, "--no-git"])
        .timeout(std::time::Duration::from_secs(600))
        .assert()
        .success();
}

// =============================================================================
// Tier 1: CLI parsing — no filesystem side-effects
// =============================================================================

#[test]
fn cli_no_args_shows_help() {
    cli()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn cli_help_flag() {
    cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Scaffold-Stacks CLI"));
}

#[test]
fn cli_version_flag() {
    cli()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("stacksdapp"));
}

#[test]
fn cli_unknown_subcommand() {
    cli()
        .arg("foobar")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// =============================================================================
// Tier 2: `new` command — scaffold project and verify file structure
// =============================================================================

#[test]
fn new_creates_project_structure() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "test-app");

    let root = tmp.path().join("test-app");
    assert!(root.exists(), "project root should exist");

    // Top-level files
    assert!(root.join("package.json").exists(), "root package.json");
    assert!(root.join(".gitignore").exists(), "root .gitignore");

    // Contracts workspace
    let contracts = root.join("contracts");
    assert!(contracts.join("Clarinet.toml").exists(), "Clarinet.toml");
    assert!(
        contracts.join("package.json").exists(),
        "contracts/package.json"
    );
    assert!(
        contracts.join("vitest.config.ts").exists(),
        "vitest.config.ts"
    );
    assert!(contracts.join("tsconfig.json").exists(), "tsconfig.json");
    assert!(
        contracts.join("contracts/counter.clar").exists(),
        "counter.clar"
    );
    assert!(
        contracts.join("tests/counter.test.ts").exists(),
        "counter.test.ts"
    );

    // Settings files
    assert!(
        contracts.join("settings/Devnet.toml").exists(),
        "Devnet.toml"
    );
    assert!(
        contracts.join("settings/Testnet.toml").exists(),
        "Testnet.toml"
    );
    assert!(
        contracts.join("settings/Mainnet.toml").exists(),
        "Mainnet.toml"
    );

    // Frontend workspace
    let frontend = root.join("frontend");
    assert!(
        frontend.join("package.json").exists(),
        "frontend/package.json"
    );
    assert!(
        frontend.join("src/app/page.tsx").exists(),
        "frontend page.tsx"
    );
    assert!(
        frontend.join("scripts/export-abi.mjs").exists(),
        "export-abi.mjs"
    );
    assert!(
        frontend.join("node_modules").exists(),
        "frontend/node_modules installed"
    );

    // Contracts node_modules installed
    assert!(
        contracts.join("node_modules").exists(),
        "contracts/node_modules installed"
    );
}

#[test]
fn new_fails_if_directory_exists() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    // Create the directory first
    fs::create_dir(tmp.path().join("dup-project")).unwrap();

    cli()
        .current_dir(tmp.path())
        .args(["new", "dup-project", "--no-git"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn new_clarinet_toml_has_counter_contract() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "toml-check");

    let toml = fs::read_to_string(tmp.path().join("toml-check/contracts/Clarinet.toml")).unwrap();
    assert!(
        toml.contains("[contracts.counter]"),
        "Clarinet.toml should define counter contract"
    );
    assert!(
        toml.contains("contracts/counter.clar"),
        "Clarinet.toml should reference counter.clar path"
    );
}

#[test]
fn new_counter_contract_has_expected_functions() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "counter-fns");

    let source = fs::read_to_string(
        tmp.path()
            .join("counter-fns/contracts/contracts/counter.clar"),
    )
    .unwrap();

    assert!(
        source.contains("(define-public (increment)"),
        "increment fn"
    );
    assert!(
        source.contains("(define-public (decrement)"),
        "decrement fn"
    );
    assert!(source.contains("(define-public (reset)"), "reset fn");
    assert!(
        source.contains("(define-read-only (get-count)"),
        "get-count fn"
    );
}

// =============================================================================
// Tier 3: `add` command — add contracts to existing project
// =============================================================================

#[test]
fn add_blank_contract() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "add-test");

    let project = tmp.path().join("add-test");

    cli()
        .current_dir(&project)
        .args(["add", "greeter"])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    // Contract file created
    assert!(
        project.join("contracts/contracts/greeter.clar").exists(),
        "greeter.clar should be created"
    );

    // Test file created
    assert!(
        project.join("contracts/tests/greeter.test.ts").exists(),
        "greeter.test.ts should be created"
    );

    // Clarinet.toml updated
    let toml = fs::read_to_string(project.join("contracts/Clarinet.toml")).unwrap();
    assert!(
        toml.contains("[contracts.greeter]"),
        "Clarinet.toml should define greeter"
    );
}

#[test]
fn add_sip010_template() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "sip010-test");

    let project = tmp.path().join("sip010-test");

    cli()
        .current_dir(&project)
        .args(["add", "my-token", "--template", "sip010"])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let source = fs::read_to_string(project.join("contracts/contracts/my-token.clar")).unwrap();

    assert!(
        source.contains("define-fungible-token"),
        "SIP-010 should define a fungible token"
    );
    assert!(
        source.contains("(define-public (mint"),
        "SIP-010 should have mint function"
    );
    assert!(
        source.contains("(define-public (transfer"),
        "SIP-010 should have transfer function"
    );
}

#[test]
fn add_sip009_template() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "sip009-test");

    let project = tmp.path().join("sip009-test");

    cli()
        .current_dir(&project)
        .args(["add", "my-nft", "--template", "sip009"])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let source = fs::read_to_string(project.join("contracts/contracts/my-nft.clar")).unwrap();

    assert!(
        source.contains("define-non-fungible-token"),
        "SIP-009 should define a non-fungible token"
    );
    assert!(
        source.contains("(define-public (mint"),
        "SIP-009 should have mint function"
    );
}

#[test]
fn add_duplicate_contract_fails() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "dup-contract");

    let project = tmp.path().join("dup-contract");

    // counter already exists from scaffold
    cli()
        .current_dir(&project)
        .args(["add", "counter"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// =============================================================================
// Tier 4: `generate` command — requires node + clarinet
// =============================================================================

#[test]
fn generate_creates_typescript_bindings() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "gen-test");

    let project = tmp.path().join("gen-test");

    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let generated = project.join("frontend/src/generated");
    assert!(generated.join("contracts.ts").exists(), "contracts.ts");
    assert!(generated.join("hooks.ts").exists(), "hooks.ts");
    assert!(
        generated.join("DebugContracts.tsx").exists(),
        "DebugContracts.tsx"
    );
    assert!(
        generated.join("deployments.json").exists(),
        "deployments.json"
    );
}

#[test]
fn generate_contracts_ts_contains_counter_functions() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "gen-fns");

    let project = tmp.path().join("gen-fns");

    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let contracts_ts =
        fs::read_to_string(project.join("frontend/src/generated/contracts.ts")).unwrap();

    // Should contain camelCase wrappers for the counter contract functions
    assert!(
        contracts_ts.contains("counter_increment") || contracts_ts.contains("counter_Increment"),
        "contracts.ts should contain increment wrapper"
    );
    assert!(
        contracts_ts.contains("counter_getCount") || contracts_ts.contains("counter_get_count"),
        "contracts.ts should contain getCount wrapper"
    );
}

#[test]
fn generate_hooks_ts_has_react_hooks() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "gen-hooks");

    let project = tmp.path().join("gen-hooks");

    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let hooks_ts = fs::read_to_string(project.join("frontend/src/generated/hooks.ts")).unwrap();

    assert!(
        hooks_ts.contains("useState"),
        "hooks.ts should import useState"
    );
    assert!(
        hooks_ts.contains("useCallback"),
        "hooks.ts should import useCallback"
    );
}

#[test]
fn generate_is_idempotent() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "gen-idem");

    let project = tmp.path().join("gen-idem");

    // First generate
    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let contracts_ts_path = project.join("frontend/src/generated/contracts.ts");
    let first_content = fs::read_to_string(&contracts_ts_path).unwrap();

    // Second generate — should produce identical output
    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date"));

    let second_content = fs::read_to_string(&contracts_ts_path).unwrap();
    assert_eq!(
        first_content, second_content,
        "generate should be idempotent"
    );
}

// =============================================================================
// Tier 5: `clean` command
// =============================================================================

#[test]
fn clean_removes_generated_files() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "clean-test");

    let project = tmp.path().join("clean-test");

    // First generate so there are files to clean
    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let generated = project.join("frontend/src/generated");
    assert!(
        generated.join("contracts.ts").exists(),
        "contracts.ts should exist before clean"
    );

    // Run clean
    cli()
        .current_dir(&project)
        .arg("clean")
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    // Generated TypeScript files should be removed
    assert!(
        !generated.join("contracts.ts").exists(),
        "contracts.ts should be removed after clean"
    );
    assert!(
        !generated.join("hooks.ts").exists(),
        "hooks.ts should be removed after clean"
    );

    // But deployments.json should be recreated as empty
    assert!(
        generated.join("deployments.json").exists(),
        "deployments.json should be recreated after clean"
    );

    let deployments = fs::read_to_string(generated.join("deployments.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&deployments).unwrap();
    assert!(
        json["contracts"].as_object().unwrap().is_empty(),
        "deployments.json contracts should be empty after clean"
    );
}

// =============================================================================
// Tier 6: Full flow — new → generate → add → generate
// =============================================================================

#[test]
fn full_flow_new_generate_add_generate() {
    if !has_toolchain() {
        eprintln!("SKIP: node/clarinet not found");
        return;
    }

    let tmp = TempDir::new().unwrap();
    scaffold_project(tmp.path(), "full-flow");

    let project = tmp.path().join("full-flow");

    // Step 1: Generate bindings for the starter counter contract
    cli()
        .current_dir(&project)
        .arg("generate")
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let generated = project.join("frontend/src/generated");
    let contracts_v1 = fs::read_to_string(generated.join("contracts.ts")).unwrap();
    assert!(
        contracts_v1.contains("counter"),
        "v1 contracts.ts should reference counter"
    );

    // Step 2: Add a new contract
    cli()
        .current_dir(&project)
        .args(["add", "vault"])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    // Step 3: Verify the regenerated bindings include both contracts
    let contracts_v2 = fs::read_to_string(generated.join("contracts.ts")).unwrap();
    assert!(
        contracts_v2.contains("counter"),
        "v2 contracts.ts should still reference counter"
    );
    assert!(
        contracts_v2.contains("vault"),
        "v2 contracts.ts should now reference vault"
    );

    // Hooks should reference both contracts too
    let hooks_v2 = fs::read_to_string(generated.join("hooks.ts")).unwrap();
    assert!(
        hooks_v2.contains("counter") && hooks_v2.contains("vault"),
        "hooks.ts should reference both contracts"
    );

    // DebugContracts should reference both
    let debug_ui = fs::read_to_string(generated.join("DebugContracts.tsx")).unwrap();
    assert!(
        debug_ui.contains("counter") && debug_ui.contains("vault"),
        "DebugContracts.tsx should reference both contracts"
    );
}

// =============================================================================
// Tier 7: `generate` outside project should fail gracefully
// =============================================================================

#[test]
fn generate_outside_project_fails() {
    let tmp = TempDir::new().unwrap();

    cli()
        .current_dir(tmp.path())
        .arg("generate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No scaffold-stacks project found"));
}

#[test]
fn add_outside_project_fails() {
    let tmp = TempDir::new().unwrap();

    cli()
        .current_dir(tmp.path())
        .args(["add", "foo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No scaffold-stacks project found"));
}

#[test]
fn clean_outside_project_succeeds_gracefully() {
    // clean should not crash even if there are no generated dirs
    let tmp = TempDir::new().unwrap();

    cli()
        .current_dir(tmp.path())
        .arg("clean")
        .assert()
        .success();
}

// =============================================================================
// Tier 8: `doctor` command
// =============================================================================

#[test]
fn doctor_runs_without_crash() {
    cli()
        .arg("doctor")
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();
}