use altius_svm_detect::Cluster;
use serde::Deserialize;

use crate::error::GuardError;
use crate::tx_request::{TxKind, TxRequest};

/// How mainnet-beta transactions are handled. Note there is no `Auto`
/// variant: that is deliberate, not an oversight. Per Phase 0 spec §6,
/// mainnet approval is a hard rule that configuration cannot disable, and
/// the simplest way to guarantee that is to make the unwanted state
/// unrepresentable — `mainnet = "auto"` in `altius.toml` fails to parse
/// with an "unknown variant" error rather than silently doing something
/// unsafe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MainnetPolicy {
    Forbid,
    #[default]
    RequireApproval,
}

/// Parsed `[svm.policy]` section of a project's `altius.toml`. Every
/// field has a safe default so an absent config file (or a partial one)
/// never accidentally widens what is allowed.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub allowed_clusters: Vec<Cluster>,
    pub mainnet: MainnetPolicy,
    pub max_lamports_out: u64,
    pub deny_instructions: Vec<String>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        PolicyConfig {
            allowed_clusters: vec![Cluster::Localnet, Cluster::Devnet],
            mainnet: MainnetPolicy::RequireApproval,
            max_lamports_out: 100_000_000,
            deny_instructions: vec![
                "SetAuthority".to_string(),
                "Upgrade".to_string(),
                "CloseAccount".to_string(),
            ],
        }
    }
}

/// Outcome of evaluating a [`TxRequest`] against a [`PolicyConfig`],
/// before any simulation has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Nothing in policy objects; proceed to simulation. Does not by
    /// itself mean the transaction will be auto-approved later — that is
    /// decided independently at the approval stage.
    Continue,
    /// Policy allows the transaction to proceed to simulation, but a
    /// human must approve it regardless of what cluster/kind rules would
    /// otherwise allow.
    RequireApproval,
    /// Policy forbids the transaction outright; it must not reach
    /// simulation or the signer.
    Reject(String),
}

impl PolicyConfig {
    pub fn from_toml_str(s: &str) -> Result<PolicyConfig, GuardError> {
        Ok(toml::from_str(s)?)
    }

    pub fn evaluate(&self, tx: &TxRequest) -> PolicyDecision {
        if !self.allowed_clusters.contains(&tx.cluster) {
            return PolicyDecision::Reject(format!(
                "cluster {} is not in allowed_clusters {:?}",
                tx.cluster, self.allowed_clusters
            ));
        }

        // Hard rule: mainnet always at least requires approval, and can
        // be forbidden outright, but can never be auto-approved — there
        // is no code path here that returns `Continue` for mainnet.
        if tx.cluster.is_mainnet() {
            return match self.mainnet {
                MainnetPolicy::Forbid => PolicyDecision::Reject(
                    "mainnet-beta transactions are forbidden by policy".into(),
                ),
                MainnetPolicy::RequireApproval => PolicyDecision::RequireApproval,
            };
        }

        if self.deny_instructions.iter().any(|d| *d == tx.kind.name()) {
            return PolicyDecision::RequireApproval;
        }

        if let TxKind::Transfer { lamports } = tx.kind {
            if lamports > self.max_lamports_out {
                return PolicyDecision::Reject(format!(
                    "transfer of {lamports} lamports exceeds max_lamports_out {}",
                    self.max_lamports_out
                ));
            }
        }

        PolicyDecision::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(cluster: Cluster, kind: TxKind) -> TxRequest {
        TxRequest {
            description: "test tx".into(),
            cluster,
            kind,
            unsigned_transaction: vec![],
        }
    }

    #[test]
    fn rejects_mainnet_auto_at_parse_time() {
        let err = PolicyConfig::from_toml_str("mainnet = \"auto\"").unwrap_err();
        assert!(matches!(err, GuardError::PolicyConfigParse(_)));
    }

    #[test]
    fn default_policy_requires_approval_for_mainnet_even_for_a_plain_invoke() {
        let policy = PolicyConfig {
            allowed_clusters: vec![Cluster::MainnetBeta],
            ..PolicyConfig::default()
        };
        let request = tx(
            Cluster::MainnetBeta,
            TxKind::Invoke {
                instruction_name: "swap".into(),
            },
        );
        assert_eq!(policy.evaluate(&request), PolicyDecision::RequireApproval);
    }

    #[test]
    fn forbid_mainnet_rejects_before_simulation() {
        let policy = PolicyConfig {
            allowed_clusters: vec![Cluster::MainnetBeta],
            mainnet: MainnetPolicy::Forbid,
            ..PolicyConfig::default()
        };
        let request = tx(Cluster::MainnetBeta, TxKind::Deploy);
        assert!(matches!(
            policy.evaluate(&request),
            PolicyDecision::Reject(_)
        ));
    }

    #[test]
    fn cluster_not_in_allow_list_is_rejected() {
        let policy = PolicyConfig::default(); // devnet/localnet only
        let request = tx(Cluster::Testnet, TxKind::Deploy);
        assert!(matches!(
            policy.evaluate(&request),
            PolicyDecision::Reject(_)
        ));
    }

    #[test]
    fn deny_instructions_forces_approval_on_devnet() {
        let policy = PolicyConfig::default();
        let request = tx(Cluster::Devnet, TxKind::SetAuthority);
        assert_eq!(policy.evaluate(&request), PolicyDecision::RequireApproval);
    }

    #[test]
    fn oversized_transfer_is_rejected_on_devnet() {
        let policy = PolicyConfig {
            max_lamports_out: 100,
            ..PolicyConfig::default()
        };
        let request = tx(Cluster::Devnet, TxKind::Transfer { lamports: 101 });
        assert!(matches!(
            policy.evaluate(&request),
            PolicyDecision::Reject(_)
        ));
    }

    #[test]
    fn ordinary_devnet_invoke_continues() {
        let policy = PolicyConfig::default();
        let request = tx(
            Cluster::Devnet,
            TxKind::Invoke {
                instruction_name: "initialize".into(),
            },
        );
        assert_eq!(policy.evaluate(&request), PolicyDecision::Continue);
    }
}
