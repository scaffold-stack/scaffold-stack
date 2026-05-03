//! Terminal styling for `stacksdapp new` — Foundry-inspired: ASCII wordmark, boxed tagline, semantic colors.

use colored::{ColoredString, Colorize};

const INNER: usize = 49;

/// FIGlet-style wordmark: **stacks** + **dapp** (standard font).
const WORDMARK_STACKSDAPP: &[&str] = &[
    r" ____ _____  _    ____ _  ______    ____    _    ____  ____  ",
    r"/ ___|_   _|/ \  / ___| |/ / ___|  |  _ \  / \  |  _ \|  _ \ ",
    r"\___ \ | | / _ \| |   | ' /\___ \  | | | |/ _ \ | |_) | |_) |",
    r" ___) || |/ ___ \ |___| . \ ___) | | |_| / ___ \|  __/|  __/ ",
    r"|____/ |_/_/   \_\____|_|\_\____/  |____/_/   \_\_|   |_|    ",
];

pub fn print_new_project_banner() {
    println!();
    for (i, line) in WORDMARK_STACKSDAPP.iter().enumerate() {
        match i {
            0 | 4 => println!("{}", line.truecolor(110, 118, 135)),
            _ => println!("{}", line.bold().truecolor(255, 204, 92)),
        }
    }

    let top = format!(
        "    {} {} {}",
        "╭".dimmed(),
        "─".repeat(INNER).dimmed(),
        "╮".dimmed()
    );
    let bot = format!(
        "    {} {} {}",
        "╰".dimmed(),
        "─".repeat(INNER).dimmed(),
        "╯".dimmed()
    );
    let title_row = format!("{:^width$}", "Scaffold Stacks", width = INNER);
    let tag_row = format!(
        "{:^width$}",
        "Clarity · Next.js · Stacks",
        width = INNER
    );
    println!();
    println!("{top}");
    println!(
        "    {} {} {}",
        "│".dimmed(),
        title_row.bold().yellow(),
        "│".dimmed()
    );
    println!(
        "    {} {} {}",
        "│".dimmed(),
        tag_row.truecolor(175, 180, 195),
        "│".dimmed()
    );
    println!("{bot}");
    println!("    {}", "━".repeat(53).dimmed());
    println!();
}

pub fn print_creating_line(name: &str) {
    println!(
        "    {} {}",
        "Creating".bold().white(),
        format!("{name}/").bold().cyan()
    );
    println!();
}

pub fn step_done_string(label: &str, detail: &str) -> String {
    format!(
        "  {}  {}   {}",
        "✔".green().bold(),
        label.bold().white(),
        detail
    )
}

pub fn dim_rule(len: usize) -> ColoredString {
    "━".repeat(len).dimmed()
}

pub fn print_success_block(name: &str) {
    println!(
        "    {}  {}",
        "✔ Done!".green().bold(),
        format!("Project {} is ready.", name.bold().cyan())
    );
    println!("    {}", dim_rule(53));
    println!();
}

pub fn section_recommended() {
    println!(
        "    {}  {}",
        "Recommended".bold().yellow(),
        "Deploy to testnet".bold().white()
    );
    println!(
        "    {}",
        "              Hiro API · no local chain — use the faucet for STX".dimmed()
    );
    println!();
}

pub fn section_alternative() {
    println!("    {}", "─".repeat(53).dimmed());
    println!();
    println!(
        "    {}  {}",
        "Alternative".bold().truecolor(130, 175, 255),
        "Local devnet".bold().white()
    );
    println!(
        "    {}",
        "              Docker + Clarinet — mirroring production closer".dimmed()
    );
    println!();
}

pub fn footer_repo_link() {
    println!("    {}", "─".repeat(53).dimmed());
    println!(
        "    {}",
        "https://github.com/scaffold-stack/scaffold-stack".dimmed()
    );
    println!();
}
