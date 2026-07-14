//! Foundry-style display controls shared across the stacksdapp CLI and libraries.
//!
//! Init once from `main` via [`init`]. Commands and crates then use [`status`],
//! [`warn`], [`error`], [`debug`], and [`emit_json`].

pub mod project;

pub use project::{
    default_config_toml, enter_scaffold_root, find_init_root, find_scaffold_root, load_config,
    project_root, resolve_scaffold_root, StacksdappConfig, CONFIG_FILE,
};

use colored::control;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static SHELL: OnceLock<Shell> = OnceLock::new();
static JSON_EMITTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

#[derive(Debug, Clone)]
pub struct Shell {
    pub verbosity: u8,
    pub quiet: bool,
    pub format: Format,
    pub color: ColorMode,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            verbosity: 0,
            quiet: false,
            format: Format::Human,
            color: ColorMode::Auto,
        }
    }
}

/// Initialize global shell settings. Safe to call once; later calls are ignored.
pub fn init(shell: Shell) {
    apply_color(shell.color);
    let _ = SHELL.set(shell);
}

fn apply_color(mode: ColorMode) {
    match mode {
        ColorMode::Always => control::set_override(true),
        ColorMode::Never => control::set_override(false),
        ColorMode::Auto => {
            // Respect TTY: colored crate already defaults to auto when unset.
            if !io::stdout().is_terminal() && !io::stderr().is_terminal() {
                control::set_override(false);
            } else {
                control::unset_override();
            }
        }
    }
}

pub fn get() -> &'static Shell {
    SHELL.get_or_init(Shell::default)
}

pub fn verbosity() -> u8 {
    get().verbosity
}

pub fn is_quiet() -> bool {
    get().quiet || get().format == Format::Json
}

pub fn is_json() -> bool {
    get().format == Format::Json
}

/// Human status line on stdout (suppressed when quiet/json).
pub fn status(msg: impl AsRef<str>) {
    if is_quiet() {
        return;
    }
    println!("{}", msg.as_ref());
}

/// Human warning on stderr (suppressed when quiet/json).
pub fn warn(msg: impl AsRef<str>) {
    if is_quiet() {
        return;
    }
    eprintln!("{}", msg.as_ref());
}

/// Errors always print unless JSON mode (then prefer [`emit_json`] / [`emit_error_json`]).
pub fn error(msg: impl AsRef<str>) {
    if is_json() {
        return;
    }
    eprintln!("{}", msg.as_ref());
}

/// Verbose detail when `-v` / `-vv` … is set (and not quiet/json).
pub fn debug(level: u8, msg: impl AsRef<str>) {
    if is_quiet() || verbosity() < level {
        return;
    }
    eprintln!("{}", msg.as_ref());
}

/// Emit a JSON value on stdout when `--json` is active.
pub fn emit_json<T: Serialize>(value: &T) {
    if !is_json() {
        return;
    }
    match serde_json::to_string(value) {
        Ok(s) => {
            println!("{s}");
            let _ = io::stdout().flush();
            JSON_EMITTED.store(true, Ordering::SeqCst);
        }
        Err(e) => eprintln!("{{\"ok\":false,\"error\":\"json serialize failed: {e}\"}}"),
    }
}

/// True if a command already wrote a JSON payload this process.
pub fn json_already_emitted() -> bool {
    JSON_EMITTED.load(Ordering::SeqCst)
}

/// Emit a JSON error object (for command failures under `--json`).
pub fn emit_error_json(command: &str, message: &str) {
    if !is_json() || json_already_emitted() {
        return;
    }
    let payload = serde_json::json!({
        "ok": false,
        "command": command,
        "error": message,
    });
    emit_json(&payload);
}

/// Pretty-print JSON when not in machine mode helpers need human fallback.
pub fn println_human(msg: impl AsRef<str>) {
    status(msg);
}
