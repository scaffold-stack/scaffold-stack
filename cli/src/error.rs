//! Stable CLI exit codes for scripting (Foundry-style distinguishability).
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Success |
//! | 1    | Generic / unexpected error |
//! | 2    | Project not found or invalid `--root` |
//! | 3    | Missing / failing prerequisite tool (`doctor`, clarinet, node, …) |
//! | 4    | User aborted (confirmations) |
//! | 5    | Invalid / argument validation |
//! | 6    | Contract type-check failed |
//! | 7    | Tests failed |
//! | 8    | Deploy failed |
//! | 10   | Generate / codegen failed |

use thiserror::Error;

/// Typed CLI failures with stable process exit codes.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Project(String),

    #[error("{0}")]
    Prerequisite(String),

    #[error("{0}")]
    Aborted(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Check(String),

    #[error("{0}")]
    Test(String),

    #[error("{0}")]
    Deploy(String),

    #[error("{0}")]
    Generate(String),

    #[error("{0}")]
    Other(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Project(_) => 2,
            Self::Prerequisite(_) => 3,
            Self::Aborted(_) => 4,
            Self::Validation(_) => 5,
            Self::Check(_) => 6,
            Self::Test(_) => 7,
            Self::Deploy(_) => 8,
            Self::Generate(_) => 10,
            Self::Other(_) => 1,
        }
    }

    pub fn code_name(&self) -> &'static str {
        match self {
            Self::Project(_) => "project",
            Self::Prerequisite(_) => "prerequisite",
            Self::Aborted(_) => "aborted",
            Self::Validation(_) => "validation",
            Self::Check(_) => "check",
            Self::Test(_) => "test",
            Self::Deploy(_) => "deploy",
            Self::Generate(_) => "generate",
            Self::Other(_) => "error",
        }
    }
}

/// Map an `anyhow` error (including nested [`CliError`]) to a stable exit code.
pub fn exit_code_for(err: &anyhow::Error) -> i32 {
    if let Some(cli) = err.downcast_ref::<CliError>() {
        return cli.exit_code();
    }
    classify_message(&format!("{err:#}"))
}

pub fn code_name_for(err: &anyhow::Error) -> &'static str {
    if let Some(cli) = err.downcast_ref::<CliError>() {
        return cli.code_name();
    }
    match classify_message(&format!("{err:#}")) {
        2 => "project",
        3 => "prerequisite",
        4 => "aborted",
        5 => "validation",
        6 => "check",
        7 => "test",
        8 => "deploy",
        10 => "generate",
        _ => "error",
    }
}

fn classify_message(msg: &str) -> i32 {
    let m = msg.to_ascii_lowercase();

    if m.contains("no stacksdapp project")
        || m.contains("invalid --root")
        || m.contains("is not a stacksdapp project")
        || m.contains("is not a clarinet/stacksdapp project")
        || m.contains("no scaffold-stacks project")
        || m.contains("no clarinet project detected")
    {
        return 2;
    }

    if m.contains("doctor failed")
        || m.contains("clarinet is required")
        || m.contains("node.js")
        || m.contains("nodejs")
        || m.contains("not found. install")
        || m.contains("rust 1.75")
    {
        return 3;
    }

    if m.contains("aborted")
        || m.contains("refusing to clean")
        || m.contains("refusing to deploy")
        || m.contains("confirmation") && (m.contains("aborted") || m.contains("refusing"))
    {
        return 4;
    }

    if m.contains("invalid project name")
        || m.contains("invalid contract name")
        || m.contains("cannot contain path")
        || m.contains("cannot be an absolute path")
        || m.contains("must be a single directory")
        || m.contains("must be at most")
        || m.contains("cannot be empty")
        || m.contains("cannot be '.' or '..'")
    {
        return 5;
    }

    if m.contains("type-check failed") || m.contains("clarity type-check") {
        return 6;
    }

    if m.contains("tests failed") || m.contains("test failed") {
        return 7;
    }

    if m.contains("deploy")
        && (m.contains("failed")
            || m.contains("broadcast")
            || m.contains("mnemonic")
            || m.contains("deployment plan"))
    {
        return 8;
    }

    if m.contains("[generate]")
        || m.contains("failed to export")
        || m.contains("abi") && m.contains("fail")
    {
        return 10;
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_exit_codes() {
        assert_eq!(CliError::Project("x".into()).exit_code(), 2);
        assert_eq!(CliError::Prerequisite("x".into()).exit_code(), 3);
        assert_eq!(CliError::Aborted("x".into()).exit_code(), 4);
        assert_eq!(CliError::Validation("x".into()).exit_code(), 5);
        assert_eq!(CliError::Check("x".into()).exit_code(), 6);
        assert_eq!(CliError::Test("x".into()).exit_code(), 7);
        assert_eq!(CliError::Deploy("x".into()).exit_code(), 8);
        assert_eq!(CliError::Generate("x".into()).exit_code(), 10);
    }

    #[test]
    fn classifies_common_messages() {
        assert_eq!(
            classify_message("No stacksdapp project found above '/tmp'."),
            2
        );
        assert_eq!(
            classify_message("doctor failed: 1 warning(s) under --strict"),
            3
        );
        assert_eq!(classify_message("Clean aborted."), 4);
        assert_eq!(
            classify_message("Invalid project name 'bad name'. Use a letter..."),
            5
        );
        assert_eq!(classify_message("Clarity type-check failed."), 6);
        assert_eq!(classify_message("Contract tests failed."), 7);
        assert_eq!(
            classify_message("Direct devnet deployment failed for counter."),
            8
        );
    }
}
