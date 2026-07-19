//! Well-known Solana program IDs for human-readable diff output.

/// Return a short label for a mainnet program id when recognized.
pub fn label_program_id(id: &str) -> Option<&'static str> {
    match id {
        "11111111111111111111111111111111" => Some("System Program"),
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" => Some("Token Program"),
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" => Some("Token-2022 Program"),
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" => Some("Associated Token Program"),
        "BPFLoaderUpgradeab1e11111111111111111111111" => Some("BPF Loader Upgradeable"),
        "ComputeBudget111111111111111111111111111111" => Some("Compute Budget Program"),
        "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr" => Some("Memo Program"),
        "Stake11111111111111111111111111111111111111" => Some("Stake Program"),
        "Vote111111111111111111111111111111111111111" => Some("Vote Program"),
        "So11111111111111111111111111111111111111112" => Some("Native SOL mint"),
        _ => None,
    }
}

/// Format a pubkey with an optional known-program suffix.
pub fn format_pubkey_label(pubkey: &str) -> String {
    match label_program_id(pubkey) {
        Some(label) => format!("{pubkey} ({label})"),
        None => pubkey.to_owned(),
    }
}

/// Format lamports as SOL when the magnitude is useful to a human reviewer.
pub fn format_lamports(delta: i128) -> String {
    const LAMPORTS_PER_SOL: i128 = 1_000_000_000;
    let sign = if delta.is_negative() { "-" } else { "+" };
    let abs = delta.unsigned_abs();
    if abs == 0 {
        return "0 lamports".into();
    }
    let whole = abs / LAMPORTS_PER_SOL as u128;
    let frac = abs % LAMPORTS_PER_SOL as u128;
    format!("{sign}{abs} lamports ({sign}{whole}.{frac:09} SOL)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_system_program() {
        assert_eq!(
            label_program_id("11111111111111111111111111111111"),
            Some("System Program")
        );
    }

    #[test]
    fn formats_sol_amounts() {
        assert!(format_lamports(-500_000_000).contains("0.500000000 SOL"));
    }
}
