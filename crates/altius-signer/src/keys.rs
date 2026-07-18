use std::fmt;

/// An Ed25519 public key, formatted the way Solana tooling expects
/// (base58), so program ids and account addresses read the same way here
/// as they do in `solana` CLI output or an explorer.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pubkey(pub [u8; 32]);

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&bs58::encode(self.0).into_string())
    }
}

impl fmt::Debug for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pubkey({self})")
    }
}

/// An Ed25519 signature over a message signed by the corresponding
/// [`Pubkey`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature(pub [u8; 64]);

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&bs58::encode(self.0).into_string())
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubkey_display_round_trips_through_base58() {
        let bytes = [7u8; 32];
        let pubkey = Pubkey(bytes);
        let decoded = bs58::decode(pubkey.to_string()).into_vec().unwrap();
        assert_eq!(decoded, bytes.to_vec());
    }
}
