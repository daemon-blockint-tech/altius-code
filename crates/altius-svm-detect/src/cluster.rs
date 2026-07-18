use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A Solana cluster a program can target.
///
/// `MainnetBeta` is spelled out deliberately (rather than folded into a
/// generic `Cluster::Custom(String)`) so that policy code in
/// `altius-txguard` can match on it exhaustively instead of on strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cluster {
    Localnet,
    Devnet,
    Testnet,
    MainnetBeta,
}

impl Cluster {
    /// Every cluster this crate knows how to name.
    pub const ALL: [Cluster; 4] = [
        Cluster::Localnet,
        Cluster::Devnet,
        Cluster::Testnet,
        Cluster::MainnetBeta,
    ];

    pub fn is_mainnet(self) -> bool {
        matches!(self, Cluster::MainnetBeta)
    }
}

impl Default for Cluster {
    /// Absent any explicit configuration, Altius never assumes mainnet.
    fn default() -> Self {
        Cluster::Localnet
    }
}

impl fmt::Display for Cluster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Cluster::Localnet => "localnet",
            Cluster::Devnet => "devnet",
            Cluster::Testnet => "testnet",
            Cluster::MainnetBeta => "mainnet-beta",
        };
        f.write_str(name)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unrecognized cluster: {0:?}")]
pub struct ParseClusterError(pub String);

impl FromStr for Cluster {
    type Err = ParseClusterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "localnet" | "local" => Ok(Cluster::Localnet),
            "devnet" => Ok(Cluster::Devnet),
            "testnet" => Ok(Cluster::Testnet),
            "mainnet-beta" | "mainnet" => Ok(Cluster::MainnetBeta),
            other => Err(ParseClusterError(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_anchor_toml_spellings() {
        assert_eq!(Cluster::from_str("localnet"), Ok(Cluster::Localnet));
        assert_eq!(Cluster::from_str("mainnet"), Ok(Cluster::MainnetBeta));
        assert_eq!(Cluster::from_str("mainnet-beta"), Ok(Cluster::MainnetBeta));
    }

    #[test]
    fn rejects_unknown_cluster() {
        assert!(Cluster::from_str("betanet").is_err());
    }

    #[test]
    fn default_is_never_mainnet() {
        assert_eq!(Cluster::default(), Cluster::Localnet);
        assert!(!Cluster::default().is_mainnet());
    }
}
