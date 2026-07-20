//! Shared spinner steps and banner chrome for Foundry-style CLI output.

use colored::Colorize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Serializes human stdout so child-process logs cannot fight the live spinner.
fn stdout_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// How many live spinners are currently active (for `\r` line clearing).
static ACTIVE_SPINNERS: AtomicUsize = AtomicUsize::new(0);

/// Print a human line without corrupting an active spinner row.
pub fn println_human_safe(line: impl AsRef<str>) {
    if crate::is_quiet() {
        return;
    }
    let _guard = stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
    if ACTIVE_SPINNERS.load(Ordering::SeqCst) > 0 {
        print!("\r\x1b[2K");
    }
    println!("{}", line.as_ref());
    let _ = io::stdout().flush();
}

/// In-place spinner that occupies the checkmark column until [`LiveStep::finish`].
pub struct LiveStep {
    label: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    finished: bool,
}

impl LiveStep {
    pub fn finish(mut self) {
        self.complete(true);
    }

    pub fn fail(mut self) {
        self.complete(false);
    }

    fn complete(&mut self, ok: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let _guard = stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
        ACTIVE_SPINNERS.fetch_sub(1, Ordering::SeqCst);
        print!("\r\x1b[2K");
        if ok {
            println!(
                "{} {}",
                "✓".truecolor(52, 211, 153).bold(),
                self.label.white()
            );
        } else {
            println!(
                "{} {}",
                "✗".truecolor(239, 68, 68).bold(),
                self.label.white()
            );
        }
        let _ = io::stdout().flush();
    }
}

impl Drop for LiveStep {
    fn drop(&mut self) {
        if !self.finished {
            self.complete(false);
        }
    }
}

/// Print a centered banner between grey rules.
pub fn print_banner(title: &str) {
    if crate::is_quiet() {
        return;
    }
    println!();
    println!("{}", "━".repeat(46).truecolor(75, 85, 99));
    println!("{:^46}", title.bold().white());
    println!("{}", "━".repeat(46).truecolor(75, 85, 99));
    println!();
}

pub fn kv(key: &str, value: &str) {
    if crate::is_quiet() {
        return;
    }
    println!("{:<12} {}", key.truecolor(156, 163, 175), value.white());
}

pub fn rule() {
    if crate::is_quiet() {
        return;
    }
    println!("{}", "─".repeat(46).truecolor(75, 85, 99));
}

pub fn begin_step(label: &str) -> LiveStep {
    if crate::is_quiet() {
        return LiveStep {
            label: label.to_string(),
            stop: Arc::new(AtomicBool::new(true)),
            handle: None,
            finished: true,
        };
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let label_c = label.to_string();

    ACTIVE_SPINNERS.fetch_add(1, Ordering::SeqCst);
    {
        let _guard = stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
        print!(
            "\r\x1b[2K{} {}",
            SPINNER[0].truecolor(167, 139, 250),
            label.truecolor(156, 163, 175)
        );
        let _ = io::stdout().flush();
    }

    let handle = thread::spawn(move || {
        let mut i = 0usize;
        while !stop_c.load(Ordering::Relaxed) {
            {
                let _guard = stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
                print!(
                    "\r\x1b[2K{} {}",
                    SPINNER[i % SPINNER.len()].truecolor(167, 139, 250),
                    label_c.truecolor(156, 163, 175)
                );
                let _ = io::stdout().flush();
            }
            i = i.wrapping_add(1);
            thread::sleep(Duration::from_millis(80));
        }
    });

    LiveStep {
        label: label.to_string(),
        stop,
        handle: Some(handle),
        finished: false,
    }
}

pub fn step_ok(label: &str) {
    if crate::is_quiet() {
        return;
    }
    println!("{} {}", "✓".truecolor(52, 211, 153).bold(), label.white());
}

pub fn mint(s: &str) -> colored::ColoredString {
    s.truecolor(52, 211, 153)
}

pub fn grey(s: &str) -> colored::ColoredString {
    s.truecolor(156, 163, 175)
}

pub fn lavender(s: &str) -> colored::ColoredString {
    s.truecolor(167, 139, 250)
}

#[cfg(test)]
mod tests {
    use super::begin_step;
    use crate::{init, Format, Shell};

    #[test]
    fn quiet_begin_step_does_not_print_completion() {
        init(Shell {
            verbosity: 0,
            quiet: true,
            format: Format::Human,
            color: crate::ColorMode::Never,
        });

        let step = begin_step("Environment configured");
        step.finish();
    }
}
