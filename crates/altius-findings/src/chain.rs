use serde::{Deserialize, Serialize};

/// High-level family of execution environments Altius can scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainFamily {
    Solana,
    Evm,
    Algorand,
    Cairo,
    Cosmos,
    Ton,
    Unknown,
}

impl ChainFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solana => "solana",
            Self::Evm => "evm",
            Self::Algorand => "algorand",
            Self::Cairo => "cairo",
            Self::Cosmos => "cosmos",
            Self::Ton => "ton",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for ChainFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ChainFamily {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "solana" | "svm" => Ok(Self::Solana),
            "evm" | "solidity" => Ok(Self::Evm),
            "algorand" => Ok(Self::Algorand),
            "cairo" | "starknet" => Ok(Self::Cairo),
            "cosmos" | "cosmwasm" => Ok(Self::Cosmos),
            "ton" => Ok(Self::Ton),
            "unknown" | "auto" => Ok(Self::Unknown),
            _ => Err(()),
        }
    }
}
