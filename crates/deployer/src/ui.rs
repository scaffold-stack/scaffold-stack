//! Clean Foundry-style deploy terminal UI.

use colored::Colorize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn human_output_enabled() -> bool {
    !stacksdapp_shell::is_quiet()
}

fn println_human(line: impl std::fmt::Display) {
    if human_output_enabled() {
        println!("{line}");
    }
}

pub struct DeployUi {
    start: Instant,
    network: String,
    rpc: String,
    project: String,
    bar_finalized: AtomicBool,
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
        if !human_output_enabled() {
            return;
        }
        // Clear spinner line, then print final status.
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

impl DeployUi {
    pub fn start(network: &str, rpc: &str) -> Self {
        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| ".".into());

        let title = match network {
            "mainnet" => "Deploying to Stacks Mainnet 🚀",
            "devnet" => "Deploying to Local Devnet 🚀",
            _ => "Deploying to Stacks Testnet 🚀",
        };

        if human_output_enabled() {
            println!();
            println!("{}", "━".repeat(46).truecolor(75, 85, 99));
            println!("{:^46}", title.bold().white());
            println!("{}", "━".repeat(46).truecolor(75, 85, 99));
            println!();
            kv("Network", network);
            kv("RPC", rpc);
            kv("Project", &project);
            println!();
        }

        Self {
            start: Instant::now(),
            network: network.to_string(),
            rpc: rpc.to_string(),
            project,
            bar_finalized: AtomicBool::new(false),
        }
    }

    /// Start a live spinner on the current line; call [`LiveStep::finish`] when done.
    pub fn begin_step(&self, label: &str) -> LiveStep {
        if stacksdapp_shell::is_quiet() {
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

        // Seed the line immediately so the screen isn't blank.
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

    pub fn step_ok(&self, label: &str) {
        if !human_output_enabled() {
            return;
        }
        println!("{} {}", "✓".truecolor(52, 211, 153).bold(), label.white());
    }

    pub fn step_detail(&self, text: &str) {
        if !human_output_enabled() {
            return;
        }
        println!(
            "  {} {}",
            "↳".truecolor(156, 163, 175),
            text.truecolor(156, 163, 175)
        );
    }

    pub fn print_summary(&self, deployer: &str, contracts: &[String], fee_micro: u64) {
        if !human_output_enabled() {
            return;
        }
        println!();
        println!("{}", "─".repeat(46).truecolor(75, 85, 99));
        println!();
        println!("{}", "Deployment Summary".bold().white());
        println!();
        kv("Deployer", &short_addr(deployer));
        kv("Contracts", &contracts.len().to_string());
        if fee_micro > 0 {
            kv("Fee", &format!("{:.6} STX", fee_micro as f64 / 1_000_000.0));
        }
        println!();
        for name in contracts {
            println!("  {}", name.truecolor(52, 211, 153));
        }
        println!();
        println!("{}", "─".repeat(46).truecolor(75, 85, 99));
        println!();
    }

    pub fn confirm_continue(&self, yes: bool) -> anyhow::Result<bool> {
        use std::io::IsTerminal;

        if yes {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Refusing to deploy to {} without confirmation in a non-interactive terminal.\n\
                 Re-run with: stacksdapp deploy --network {} --yes",
                self.network,
                self.network
            );
        }
        if self.network == "mainnet" {
            eprintln!(
                "{}",
                "Mainnet broadcast — real funds will be spent."
                    .yellow()
                    .bold()
            );
        }
        print!("{} ", "Continue? (Y/n)".bold().white());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let t = input.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("y") || t.eq_ignore_ascii_case("yes") {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn broadcasting_start(&self) {
        if !human_output_enabled() {
            return;
        }
        self.bar_finalized.store(false, Ordering::SeqCst);
        println!();
        println!("{}", "Broadcasting...".bold().white());
        println!();
    }

    /// Update the single in-place progress bar. Finalizes (newline) only once at 100%.
    pub fn render_bar(&self, done: usize, total: usize) {
        if !human_output_enabled() {
            if done >= total {
                self.bar_finalized.store(true, Ordering::SeqCst);
            }
            return;
        }
        if self.bar_finalized.load(Ordering::SeqCst) {
            return;
        }
        let total = total.max(1);
        let pct = ((done * 100) / total).min(100);
        let width = 32usize;
        let filled = (pct * width) / 100;
        let bar: String = "█".repeat(filled) + &"░".repeat(width.saturating_sub(filled));
        print!("\r[{}] {pct}%   ", bar.truecolor(52, 211, 153));
        let _ = io::stdout().flush();
        if done >= total {
            self.bar_finalized.store(true, Ordering::SeqCst);
            println!();
            println!();
        }
    }

    pub fn contract_broadcast_ok(&self, name: &str, txid: &str) {
        if !human_output_enabled() {
            return;
        }
        println!("{} {}", "✓".truecolor(52, 211, 153).bold(), name.white());
        println!(
            "  {:<8} {}",
            "txid".truecolor(156, 163, 175),
            short_txid(txid).truecolor(156, 163, 175)
        );
        println!();
    }

    pub fn waiting_confirmation(&self) {
        println_human("Waiting for node confirmation...");
        if human_output_enabled() {
            println!();
        }
    }

    pub fn success(
        &self,
        entries: &[(String, String, String)], // name, full_contract_id, full_txid
    ) {
        if !human_output_enabled() {
            return;
        }
        println!(
            "{} {}",
            "✓".truecolor(52, 211, 153).bold(),
            "Deployment complete.".white()
        );
        println!();
        println!("{}", "━".repeat(46).truecolor(75, 85, 99));
        println!("{:^46}", "Success 🎉".bold().truecolor(52, 211, 153));
        println!("{}", "━".repeat(46).truecolor(75, 85, 99));
        println!();

        println!("{}", "Contract".bold().white());
        println!();
        for (name, id, _) in entries {
            let _ = name;
            println!("{}", id.truecolor(52, 211, 153));
        }
        println!();

        println!("{}", "Transaction".bold().white());
        println!();
        for (_, _, txid) in entries {
            if txid.is_empty() {
                println!("{}", "(pending)".truecolor(156, 163, 175));
            } else {
                println!("{}", txid.white());
            }
        }
        println!();

        println!("{}", "Generated".bold().white());
        println!();
        for f in [
            "frontend/src/generated/contracts.ts",
            "frontend/src/generated/hooks.ts",
            "frontend/src/generated/deployments.json",
        ] {
            println!(
                "{} {}",
                "✓".truecolor(52, 211, 153),
                f.truecolor(156, 163, 175)
            );
        }
        println!();

        println!("{}", "Next".bold().white());
        println!();
        println!(
            "{}",
            format!("stacksdapp dev --network {}", self.network)
                .truecolor(52, 211, 153)
                .bold()
        );
        println!();

        let chain = match self.network.as_str() {
            "mainnet" => "mainnet",
            "devnet" => "devnet",
            _ => "testnet",
        };
        let explorer_urls: Vec<String> = entries
            .iter()
            .filter(|(_, _, txid)| !txid.is_empty())
            .map(|(_, _, txid)| {
                format!(
                    "https://explorer.hiro.so/txid/{}?chain={chain}",
                    txid.trim_start_matches("0x")
                )
            })
            .collect();

        if !explorer_urls.is_empty() {
            println!("{}", "Explorer".bold().white());
            println!();
            for url in explorer_urls {
                println!("{}", url.truecolor(167, 139, 250));
            }
            println!();
            println!(
                "{}",
                "Note: the explorer link may take 10–15 seconds to show the transaction while it indexes."
                    .truecolor(156, 163, 175)
            );
            println!();
        }

        let secs = self.start.elapsed().as_secs_f64();
        println!("{} {:.1}s", "Done in".truecolor(156, 163, 175), secs);
        println!();
        let _ = (&self.rpc, &self.project);
    }

    pub fn dry_run_done(&self, contracts: &[String], fee_micro: u64) {
        if !human_output_enabled() {
            return;
        }
        println!();
        println!("{}", "Dry run complete — nothing broadcast.".bold().white());
        if fee_micro > 0 {
            println!("Estimated fee: {:.6} STX", fee_micro as f64 / 1_000_000.0);
        }
        println!("Contracts: {}", contracts.join(", "));
        println!(
            "{}",
            "Re-run without --dry-run to apply.".truecolor(156, 163, 175)
        );
        println!();
    }
}

fn kv(key: &str, value: &str) {
    if !human_output_enabled() {
        return;
    }
    println!("{:<12} {}", soft_grey(key), value.white());
}

fn soft_grey(s: &str) -> colored::ColoredString {
    s.truecolor(156, 163, 175)
}

pub fn short_addr(addr: &str) -> String {
    if addr.len() <= 14 {
        return addr.to_string();
    }
    format!("{}...{}", &addr[..8], &addr[addr.len().saturating_sub(6)..])
}

pub fn short_txid(txid: &str) -> String {
    let t = txid.trim_start_matches("0x");
    if t.len() <= 16 {
        return txid.to_string();
    }
    format!("{}...{}", &t[..8], &t[t.len().saturating_sub(7)..])
}

#[cfg(test)]
mod tests {
    use super::{human_output_enabled, DeployUi};
    use stacksdapp_shell::{init, ColorMode, Format, Shell};

    #[test]
    fn deploy_ui_respects_quiet_mode() {
        init(Shell {
            verbosity: 0,
            quiet: true,
            format: Format::Human,
            color: ColorMode::Never,
        });
        assert!(!human_output_enabled());
        let ui = DeployUi::start("devnet", "http://localhost:3999");
        ui.step_detail("hidden");
        ui.print_summary("ST1PQ", &["counter".into()], 1000);
        ui.dry_run_done(&["counter".into()], 1000);
        ui.success(&[("counter".into(), "ST1PQ.counter".into(), "0xabc".into())]);
        ui.begin_step("quiet step").finish();
    }
}
