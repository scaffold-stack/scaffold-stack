//! Deployer mnemonic inspection for testnet/mainnet safety checks.

use std::path::Path;

/// Public devnet seeds shipped in scaffold templates — must never be used on testnet/mainnet.
pub const PUBLIC_DEVNET_MNEMONICS: &[&str] = &[
    "twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw",
    "sell invite acquire kitten bamboo drastic jelly vivid peace spawn twice guilt pave pen trash pretty park cube fragile unaware remain midnight betray rebuild",
    "hold excess usual excess ring elephant install account glad dry fragile donkey gaze humble truck breeze nation gasp vacuum limb head keep delay hospital",
    "cycle puppy glare enroll cost improve round trend wrist mushroom scorpion tower claim oppose clever elephant dinosaur eight problem before frozen dune wagon high",
    "board list obtain sugar hour worth raven scout denial thunder horse logic fury scorpion fold genuine phrase wealth news aim below celery when cabin",
    "hurry aunt blame peanut heavy update captain human rice crime juice adult scale device promote vast project quiz unit note reform update climb purchase",
    "area desk dutch sign gold cricket dawn toward giggle vibrant indoor bench warfare wagon number tiny universe sand talk dilemma pottery bone trap buddy",
    "prevent gallery kind limb income control noise together echo rival record wedding sense uncover school version force bleak nuclear include danger skirt enact arrow",
    "female adjust gallery certain visit token during great side clown fitness like hurt clip knife warm bench start reunion globe detail dream depend fortune",
    "shadow private easily thought say logic fault paddle word top book during ignore notable orange flight clock image wealth health outside kitten belt reform",
];

const VALID_WORD_COUNTS: [usize; 5] = [12, 15, 18, 21, 24];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDeployerMnemonic {
    pub mnemonic: String,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemonicCheck {
    /// Placeholder or empty — not configured yet.
    NotConfigured,
    Ok,
    PublicDevnet,
    InvalidFormat(String),
}

/// Normalize whitespace for stable comparison.
pub fn normalize_mnemonic(mnemonic: &str) -> String {
    mnemonic.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_mnemonic_placeholder(mnemonic: &str) -> bool {
    mnemonic.is_empty() || mnemonic.contains('<') || mnemonic.contains('>')
}

pub fn is_public_devnet_mnemonic(mnemonic: &str) -> bool {
    let normalized = normalize_mnemonic(mnemonic);
    PUBLIC_DEVNET_MNEMONICS
        .iter()
        .any(|seed| normalize_mnemonic(seed) == normalized)
}

/// Word-count + lowercase a-z charset (no BIP39 checksum — Clarinet may accept edge cases).
pub fn validate_mnemonic_word_format(mnemonic: &str) -> Result<(), String> {
    let words: Vec<&str> = mnemonic.split_whitespace().collect();
    let count = words.len();
    if !VALID_WORD_COUNTS.contains(&count) {
        return Err(format!(
            "mnemonic has {count} words (expected 12, 15, 18, 21, or 24)"
        ));
    }
    for word in words {
        if word.is_empty() || !word.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(format!(
                "mnemonic words must be lowercase a-z (invalid token: \"{word}\")"
            ));
        }
    }
    Ok(())
}

pub fn parse_deployer_mnemonic(toml_raw: &str) -> Option<ParsedDeployerMnemonic> {
    let mut in_deployer = false;
    for (idx, line) in toml_raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[accounts.deployer]" {
            in_deployer = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deployer = false;
        }
        if in_deployer && trimmed.starts_with("mnemonic") {
            if let Some((_, val)) = trimmed.split_once('=') {
                let mnemonic = val.trim().trim_matches('"').to_string();
                return Some(ParsedDeployerMnemonic {
                    mnemonic,
                    line_number: idx + 1,
                });
            }
        }
    }
    None
}

pub fn check_deployer_mnemonic(mnemonic: &str, strict_format: bool) -> MnemonicCheck {
    if is_mnemonic_placeholder(mnemonic) {
        return MnemonicCheck::NotConfigured;
    }
    if is_public_devnet_mnemonic(mnemonic) {
        return MnemonicCheck::PublicDevnet;
    }
    if strict_format {
        if let Err(detail) = validate_mnemonic_word_format(mnemonic) {
            return MnemonicCheck::InvalidFormat(detail);
        }
    }
    MnemonicCheck::Ok
}

pub fn settings_relative_path(network: &str) -> String {
    format!("contracts/settings/{}.toml", capitalize(network))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Inspect a network settings file on disk (for doctor).
pub fn inspect_settings_file(network: &str, root: &Path, strict_format: bool) -> Option<(String, MnemonicCheck)> {
    let path = root.join(settings_relative_path(network));
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed = parse_deployer_mnemonic(&raw)?;
    let check = check_deployer_mnemonic(&parsed.mnemonic, strict_format);
    Some((path.display().to_string(), check))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_public_devnet_deployer_seed() {
        assert!(is_public_devnet_mnemonic(
            PUBLIC_DEVNET_MNEMONICS[0]
        ));
    }

    #[test]
    fn rejects_placeholder_mnemonic() {
        assert!(is_mnemonic_placeholder("<YOUR PRIVATE TESTNET MNEMONIC HERE>"));
    }

    #[test]
    fn validates_word_count_and_charset() {
        assert!(validate_mnemonic_word_format(PUBLIC_DEVNET_MNEMONICS[0]).is_ok());
        assert!(validate_mnemonic_word_format("one two three").is_err());
        assert!(validate_mnemonic_word_format("One two three four five six seven eight nine ten eleven twelve").is_err());
    }

    #[test]
    fn parse_finds_line_number() {
        let toml = r#"[accounts.deployer]
mnemonic = "twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw"
"#;
        let parsed = parse_deployer_mnemonic(toml).unwrap();
        assert_eq!(parsed.line_number, 2);
        assert!(is_public_devnet_mnemonic(&parsed.mnemonic));
    }
}
