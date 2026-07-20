//! Terminal styling for `stacksdapp new` — Scaffold Stacks mockup layout.

use colored::Colorize;

const BOX_INNER: usize = 48;
/// Visible width of the left "next steps" column (plain text, no ANSI).
const COL_LEFT: usize = 50;

/// FIGlet-style **STACKSDAPP** wordmark (standard font).
const WORDMARK_STACKSDAPP: &[&str] = &[
    r" ____ _____  _    ____ _  ______    ____    _    ____  ____  ",
    r"/ ___|_   _|/ \  / ___| |/ / ___|  |  _ \  / \  |  _ \|  _ \ ",
    r"\___ \ | | / _ \| |   | ' /\___ \  | | | |/ _ \ | |_) | |_) |",
    r" ___) || |/ ___ \ |___| . \ ___) | | |_| / ___ \|  __/|  __/ ",
    r"|____/ |_/_/   \_\____|_|\_\____/  |____/_/   \_\_|   |_|    ",
];

fn lavender(s: &str) -> colored::ColoredString {
    s.truecolor(167, 139, 250)
}

fn soft_grey(s: &str) -> colored::ColoredString {
    s.truecolor(156, 163, 175)
}

fn soft_blue(s: &str) -> colored::ColoredString {
    s.truecolor(147, 197, 253)
}

fn mint_check() -> colored::ColoredString {
    "✔".truecolor(52, 211, 153).bold()
}

fn dim_check() -> colored::ColoredString {
    "✓".truecolor(107, 114, 128)
}

fn border(ch: &str) -> colored::ColoredString {
    ch.truecolor(75, 85, 99)
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

fn mint_cmd(s: &str) -> colored::ColoredString {
    s.truecolor(52, 211, 153).bold()
}

/// Banner + config overview box shown at the start of `stacksdapp new`.
pub fn print_new_project_banner() {
    if stacksdapp_shell::is_quiet() {
        return;
    }
    println!();
    for line in WORDMARK_STACKSDAPP {
        println!("{}", lavender(line).bold());
    }
    println!();
    println!(
        "{}",
        "Scaffold Stacks dApps with Clarity, Next.js & Stacks".white()
    );
    println!();
    print_overview_box();
    println!();
}

fn print_overview_box() {
    let rows = [
        ("Framework", "Scaffold Stacks CLI"),
        ("Contracts", "Clarity"),
        ("Frontend", "Next.js"),
        ("Network", "Stacks"),
    ];
    println!(
        "{}{}{}",
        border("╭"),
        border(&"─".repeat(BOX_INNER)),
        border("╮")
    );
    for (label, value) in rows {
        let plain = format!(" ✔  {label}: {value}");
        let pad = BOX_INNER.saturating_sub(plain.chars().count());
        println!(
            "{} {}  {}: {}{} {}",
            border("│"),
            mint_check(),
            soft_grey(label),
            value.white(),
            spaces(pad),
            border("│")
        );
    }
    println!(
        "{}{}{}",
        border("╰"),
        border(&"─".repeat(BOX_INNER)),
        border("╯")
    );
}

pub fn print_creating_line(name: &str) {
    if stacksdapp_shell::is_quiet() {
        return;
    }
    println!(
        "{} Creating new project: {}",
        mint_check(),
        name.truecolor(52, 211, 153).bold()
    );
}

pub fn step_done_string(label: &str, detail: &str) -> String {
    format!("  {}  {} {}", dim_check(), label.white(), detail.white())
}

pub fn note_line(text: &str) -> String {
    format!("     {}", soft_grey(text))
}

pub fn print_success_block(name: &str) {
    if stacksdapp_shell::is_quiet() {
        return;
    }
    println!(
        "  {}  {} Project {} is ready.",
        dim_check(),
        "Done!".truecolor(52, 211, 153).bold(),
        name.truecolor(52, 211, 153).bold()
    );
    println!();
    println!(
        "{}",
        soft_grey("──────────────────────────────────────────────────────────────")
    );
    println!();
}

/// Dual-column recommended next steps + documentation footer.
pub fn print_next_steps(name: &str) {
    if stacksdapp_shell::is_quiet() {
        return;
    }

    println!("💡 {}", lavender("RECOMMENDED NEXT STEPS").bold());
    println!();

    let left_h = "Option 1: Deploy to Testnet (Recommended)";
    print!("  ");
    print!("{}", soft_blue("Option 1: Deploy to Testnet").bold());
    print!(" {}", soft_grey("(Recommended)"));
    print!("{}", spaces(COL_LEFT.saturating_sub(left_h.len())));
    print!(" {} ", soft_grey("│"));
    println!(
        "💻 {}",
        soft_blue("Option 2: Local Devnet (Alternative)").bold()
    );
    println!();

    struct Step {
        plain: String,
        is_cmd: bool,
        detail: Option<&'static str>,
    }

    let left: [Step; 5] = [
        Step {
            plain: format!("cd {name}"),
            is_cmd: false,
            detail: None,
        },
        Step {
            plain: "Get testnet STX".into(),
            is_cmd: false,
            detail: Some("https://explorer.hiro.so/sandbox/faucet?chain=testnet"),
        },
        Step {
            plain: "Edit contracts/settings/Testnet.toml".into(),
            is_cmd: false,
            detail: Some(r#"accounts.deployer.mnemonic = "...""#),
        },
        Step {
            plain: "stacksdapp deploy --network testnet".into(),
            is_cmd: true,
            detail: None,
        },
        Step {
            plain: "stacksdapp dev --network testnet".into(),
            is_cmd: true,
            detail: None,
        },
    ];

    let right: [Step; 3] = [
        Step {
            plain: format!("cd {name} && start Docker Desktop"),
            is_cmd: false,
            detail: None,
        },
        Step {
            plain: "stacksdapp dev".into(),
            is_cmd: true,
            detail: Some("# local chain + Next.js"),
        },
        Step {
            plain: "stacksdapp deploy --network devnet".into(),
            is_cmd: true,
            detail: Some("# second terminal"),
        },
    ];

    let rows = left.len().max(right.len());
    for i in 0..rows {
        let n = i + 1;

        print!("  ");
        let left_w = if let Some(step) = left.get(i) {
            let num = format!("{n}. ");
            print!("{}", soft_blue(&num).bold());
            if step.is_cmd {
                print!("{}", mint_cmd(&step.plain));
            } else {
                print!("{}", step.plain.white());
            }
            num.len() + step.plain.chars().count()
        } else {
            0
        };
        print!("{}", spaces(COL_LEFT.saturating_sub(left_w)));
        print!(" {} ", soft_grey("│"));

        if let Some(step) = right.get(i) {
            let num = format!("{n}. ");
            print!("{}", soft_blue(&num).bold());
            if step.is_cmd {
                print!("{}", mint_cmd(&step.plain));
            } else {
                print!("{}", step.plain.white());
            }
            if let Some(note) = step.detail {
                if note.starts_with('#') {
                    print!("  {}", soft_grey(note));
                }
            }
        }
        println!();

        // Detail under left (URL / mnemonic). Skip divider if too wide.
        if let Some(step) = left.get(i) {
            if let Some(detail) = step.detail {
                let detail_plain = format!("   {detail}");
                if detail_plain.chars().count() > COL_LEFT {
                    println!("     {}", soft_grey(detail));
                } else {
                    print!("  ");
                    print!("   {}", soft_grey(detail));
                    print!(
                        "{}",
                        spaces(COL_LEFT.saturating_sub(detail_plain.chars().count()))
                    );
                    print!(" {} ", soft_grey("│"));
                    println!();
                }
            }
        }
    }

    println!();
    footer_docs_box();
}

fn footer_docs_box() {
    let url = "https://scaffoldstacks.mintlify.app/";
    let label = "Documentation";
    let plain = format!(" ⓘ  {label}  {url}");
    let inner = plain.chars().count().max(42);
    println!(
        "{}{}{}",
        border("╭"),
        border(&"─".repeat(inner)),
        border("╮")
    );
    let pad = inner.saturating_sub(plain.chars().count());
    println!(
        "{} {}  {}  {}{} {}",
        border("│"),
        soft_blue("ⓘ"),
        soft_grey(label),
        lavender(url),
        spaces(pad),
        border("│")
    );
    println!(
        "{}{}{}",
        border("╰"),
        border(&"─".repeat(inner)),
        border("╯")
    );
    println!();
}
