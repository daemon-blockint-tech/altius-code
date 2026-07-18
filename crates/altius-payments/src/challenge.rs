//! Parsing of x402 HTTP 402 "Payment Required" challenge bodies.
//!
//! The wire format follows the x402 spec (<https://github.com/coinbase/x402>,
//! adopted by the Solana Foundation x402 work): a 402 response carries a JSON
//! body with an `x402Version`, an optional human-readable `error`, and an
//! `accepts` array of payment requirements the resource server will settle.
//!
//! Everything here treats the challenge as untrusted remote input: fields are
//! bounds-checked and unknown schemes/networks/assets are surfaced as typed
//! errors rather than guessed at.

use altius_svm_detect::Cluster;
use serde::{Deserialize, Serialize};

use crate::error::{PaymentError, PaymentResult};

/// The only x402 version this crate speaks.
pub const SUPPORTED_X402_VERSION: u64 = 1;

/// The only settlement scheme supported so far: pay the exact amount up
/// front, then retry the request with proof.
pub const SCHEME_EXACT: &str = "exact";

/// Pseudo-mint address x402 uses for native SOL.
pub const NATIVE_SOL_MINT: &str = "So11111111111111111111111111111111111111112";

const MAX_ACCEPTS: usize = 32;
const MAX_FIELD_LEN: usize = 2_048;
const MAX_CHALLENGE_BYTES: usize = 256 * 1024;

/// One entry of the `accepts` array: a way the resource server is willing
/// to be paid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    /// Settlement scheme, e.g. `"exact"`.
    pub scheme: String,
    /// Network identifier, e.g. `"solana"` or `"solana-devnet"`.
    pub network: String,
    /// Amount in the asset's base units, as a decimal string.
    pub max_amount_required: String,
    /// URL of the resource being paid for.
    pub resource: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mime_type: String,
    /// Recipient address on `network`.
    pub pay_to: String,
    #[serde(default)]
    pub max_timeout_seconds: u64,
    /// Asset identifier (token mint). Empty or the native SOL pseudo-mint
    /// means native SOL.
    #[serde(default)]
    pub asset: String,
    /// Scheme-specific extension data, passed through opaquely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl PaymentRequirements {
    /// The Solana cluster this requirement settles on, if the network is a
    /// Solana network this build understands.
    pub fn cluster(&self) -> PaymentResult<Cluster> {
        network_cluster(&self.network)
    }

    /// Whether the asset is native SOL (the only asset supported so far;
    /// SPL-token settlement is an intentional stub for later).
    pub fn is_native_sol(&self) -> bool {
        self.asset.is_empty() || self.asset == "native" || self.asset == NATIVE_SOL_MINT
    }

    /// The required amount in lamports.
    pub fn lamports(&self) -> PaymentResult<u64> {
        self.max_amount_required
            .parse::<u64>()
            .map_err(|_| PaymentError::InvalidAmount(self.max_amount_required.clone()))
    }

    fn validate(&self) -> PaymentResult<()> {
        for (name, value) in [
            ("scheme", &self.scheme),
            ("network", &self.network),
            ("maxAmountRequired", &self.max_amount_required),
            ("resource", &self.resource),
            ("payTo", &self.pay_to),
            ("asset", &self.asset),
        ] {
            if value.len() > MAX_FIELD_LEN {
                return Err(PaymentError::InvalidChallenge(format!(
                    "{name} exceeds {MAX_FIELD_LEN} bytes"
                )));
            }
        }
        if self.scheme.is_empty() || self.network.is_empty() || self.pay_to.is_empty() {
            return Err(PaymentError::InvalidChallenge(
                "scheme, network, and payTo must be non-empty".into(),
            ));
        }
        Ok(())
    }
}

/// A parsed 402 challenge body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentChallenge {
    pub x402_version: u64,
    #[serde(default)]
    pub error: Option<String>,
    pub accepts: Vec<PaymentRequirements>,
}

impl PaymentChallenge {
    /// Parse and validate an HTTP 402 response body.
    pub fn parse(body: &str) -> PaymentResult<Self> {
        if body.len() > MAX_CHALLENGE_BYTES {
            return Err(PaymentError::InvalidChallenge(format!(
                "body exceeds {MAX_CHALLENGE_BYTES} bytes"
            )));
        }
        let challenge: PaymentChallenge = serde_json::from_str(body)
            .map_err(|error| PaymentError::InvalidChallenge(error.to_string()))?;
        if challenge.x402_version != SUPPORTED_X402_VERSION {
            return Err(PaymentError::UnsupportedVersion(challenge.x402_version));
        }
        if challenge.accepts.is_empty() {
            return Err(PaymentError::InvalidChallenge(
                "accepts array is empty".into(),
            ));
        }
        if challenge.accepts.len() > MAX_ACCEPTS {
            return Err(PaymentError::InvalidChallenge(format!(
                "accepts array exceeds {MAX_ACCEPTS} entries"
            )));
        }
        for requirement in &challenge.accepts {
            requirement.validate()?;
        }
        Ok(challenge)
    }

    /// Pick the first requirement Altius can settle: `exact` scheme, a
    /// known Solana network, and native SOL as the asset.
    pub fn select_solana_requirement(&self) -> PaymentResult<&PaymentRequirements> {
        self.accepts
            .iter()
            .find(|req| {
                req.scheme == SCHEME_EXACT
                    && network_cluster(&req.network).is_ok()
                    && req.is_native_sol()
            })
            .ok_or_else(|| {
                PaymentError::NoSupportedRequirement(format!(
                    "offered: {}",
                    self.accepts
                        .iter()
                        .map(|r| format!("{}/{}", r.scheme, r.network))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

/// Map an x402 network identifier onto an Altius [`Cluster`].
pub fn network_cluster(network: &str) -> PaymentResult<Cluster> {
    match network {
        "solana" | "solana-mainnet" => Ok(Cluster::MainnetBeta),
        "solana-devnet" => Ok(Cluster::Devnet),
        "solana-testnet" => Ok(Cluster::Testnet),
        "solana-localnet" => Ok(Cluster::Localnet),
        other => Err(PaymentError::UnsupportedNetwork(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn fixture(network: &str, asset: &str) -> String {
        format!(
            r#"{{
                "x402Version": 1,
                "error": "payment required",
                "accepts": [{{
                    "scheme": "exact",
                    "network": "{network}",
                    "maxAmountRequired": "10000",
                    "resource": "https://api.example.com/v1/report",
                    "description": "one report",
                    "mimeType": "application/json",
                    "payTo": "7VHUFJHWu2CuExkJcJrzhQPJ2oygupTWkL2A2For4BmE",
                    "maxTimeoutSeconds": 60,
                    "asset": "{asset}"
                }}]
            }}"#
        )
    }

    #[test]
    fn parses_a_solana_devnet_challenge() {
        let challenge = PaymentChallenge::parse(&fixture("solana-devnet", "")).unwrap();
        let req = challenge.select_solana_requirement().unwrap();
        assert_eq!(req.cluster().unwrap(), Cluster::Devnet);
        assert_eq!(req.lamports().unwrap(), 10_000);
        assert!(req.is_native_sol());
    }

    #[test]
    fn native_sol_pseudo_mint_counts_as_native() {
        let challenge = PaymentChallenge::parse(&fixture("solana", NATIVE_SOL_MINT)).unwrap();
        let req = challenge.select_solana_requirement().unwrap();
        assert_eq!(req.cluster().unwrap(), Cluster::MainnetBeta);
    }

    #[test]
    fn rejects_unknown_version() {
        let body = r#"{"x402Version": 2, "accepts": [{"scheme":"exact","network":"solana","maxAmountRequired":"1","resource":"r","payTo":"p"}]}"#;
        assert!(matches!(
            PaymentChallenge::parse(body),
            Err(PaymentError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn rejects_empty_accepts() {
        let body = r#"{"x402Version": 1, "accepts": []}"#;
        assert!(matches!(
            PaymentChallenge::parse(body),
            Err(PaymentError::InvalidChallenge(_))
        ));
    }

    #[test]
    fn spl_token_requirements_are_not_selected() {
        let challenge = PaymentChallenge::parse(&fixture(
            "solana-devnet",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        ))
        .unwrap();
        assert!(matches!(
            challenge.select_solana_requirement(),
            Err(PaymentError::NoSupportedRequirement(_))
        ));
    }

    #[test]
    fn unknown_network_is_not_selected() {
        let challenge = PaymentChallenge::parse(&fixture("base-sepolia", "")).unwrap();
        assert!(matches!(
            challenge.select_solana_requirement(),
            Err(PaymentError::NoSupportedRequirement(_))
        ));
        assert!(matches!(
            network_cluster("base-sepolia"),
            Err(PaymentError::UnsupportedNetwork(_))
        ));
    }

    #[test]
    fn non_numeric_amount_is_rejected_lazily() {
        let body = fixture("solana-devnet", "").replace("\"10000\"", "\"1e9\"");
        let challenge = PaymentChallenge::parse(&body).unwrap();
        let req = challenge.select_solana_requirement().unwrap();
        assert!(matches!(
            req.lamports(),
            Err(PaymentError::InvalidAmount(_))
        ));
    }

    #[test]
    fn rejects_oversized_challenge_before_json_parsing() {
        let body = " ".repeat(MAX_CHALLENGE_BYTES + 1);
        assert!(matches!(
            PaymentChallenge::parse(&body),
            Err(PaymentError::InvalidChallenge(_))
        ));
    }

    proptest! {
        #[test]
        fn decimal_lamport_amounts_round_trip(amount in any::<u64>()) {
            let mut requirement = PaymentChallenge::parse(&fixture("solana-devnet", ""))
                .unwrap()
                .select_solana_requirement()
                .unwrap()
                .clone();
            requirement.max_amount_required = amount.to_string();
            prop_assert_eq!(requirement.lamports().unwrap(), amount);
        }

        #[test]
        fn accepts_count_cap_is_enforced(extra in 1usize..32) {
            let requirement = serde_json::json!({
                "scheme": "exact",
                "network": "solana-devnet",
                "maxAmountRequired": "1",
                "resource": "https://example.invalid",
                "payTo": "7VHUFJHWu2CuExkJcJrzhQPJ2oygupTWkL2A2For4BmE"
            });
            let body = serde_json::json!({
                "x402Version": 1,
                "accepts": vec![requirement; MAX_ACCEPTS + extra]
            })
            .to_string();
            prop_assert!(matches!(
                PaymentChallenge::parse(&body),
                Err(PaymentError::InvalidChallenge(_))
            ));
        }
    }
}
