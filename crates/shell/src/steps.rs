//! Shared spinner steps and banner chrome for Foundry-style CLI output.

use colored::Colorize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
            finished: false,
        };
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let label_c = label.to_string();

    print!(
        "\r{} {}",
        SPINNER[0].truecolor(167, 139, 250),
        label.truecolor(156, 163, 175)
    );
    let _ = io::stdout().flush();

    let handle = thread::spawn(move || {
        let mut i = 0usize;
        while !stop_c.load(Ordering::Relaxed) {
            print!(
                "\r{} {}",
                SPINNER[i % SPINNER.len()].truecolor(167, 139, 250),
                label_c.truecolor(156, 163, 175)
            );
            let _ = io::stdout().flush();
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
