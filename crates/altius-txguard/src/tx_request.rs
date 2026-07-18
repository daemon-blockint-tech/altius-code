use altius_svm_detect::Cluster;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

/// What an on-chain transaction is trying to do, at a level of detail
/// coarse enough for policy and approval decisions without needing to
/// decode a real Solana instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxKind {
    Deploy,
    Upgrade,
    SetAuthority,
    CloseAccount,
    Transfer { lamports: u64 },
    /// An x402 / machine-payment settlement leaving the wallet. Kept
    /// separate from `Transfer` so policy can gate paid API calls
    /// independently of plain fund movement (Phase C, altius-payments).
    Payment { lamports: u64 },
    Invoke { instruction_name: String },
}

impl TxKind {
    /// Actions that cannot be meaningfully undone once landed: changing
    /// who controls a program, closing an account, or moving funds. A
    /// fresh `Deploy` is comparatively cheap to correct (redeploy, or
    /// upgrade again), so it is not included here.
    pub fn is_irreversible(&self) -> bool {
        matches!(
            self,
            TxKind::Upgrade
                | TxKind::SetAuthority
                | TxKind::CloseAccount
                | TxKind::Transfer { .. }
                | TxKind::Payment { .. }
        )
    }

    /// Stable name used to match against a policy's `deny_instructions`
    /// list (see `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §6).
    pub fn name(&self) -> String {
        match self {
            TxKind::Deploy => "Deploy".to_string(),
            TxKind::Upgrade => "Upgrade".to_string(),
            TxKind::SetAuthority => "SetAuthority".to_string(),
            TxKind::CloseAccount => "CloseAccount".to_string(),
            TxKind::Transfer { .. } => "Transfer".to_string(),
            TxKind::Payment { .. } => "Payment".to_string(),
            TxKind::Invoke { instruction_name } => instruction_name.clone(),
        }
    }
}

/// A candidate transaction that has not been signed and must not be until
/// it has passed every stage of [`crate::pipeline::TxGuard::submit`].
///
/// `message` is real, real Solana `solana_message::Message` — the exact
/// bytes `message.serialize()` produces are what gets signed. Some
/// transactions need more than one signer (for example, deploying a new
/// program requires the fresh program/buffer keypairs to sign alongside
/// the wallet): those are provided in `extra_signatures`, obtained by
/// signing locally with keypairs this process holds in memory only for
/// the lifetime of building the request. They are never the wallet key —
/// the wallet's signature is the one thing `TxGuard::submit` obtains
/// through the isolated `altius-signer`.
#[derive(Debug, Clone, PartialEq)]
pub struct TxRequest {
    /// Human-readable summary shown in approval prompts and audit logs.
    pub description: String,
    pub cluster: Cluster,
    pub kind: TxKind,
    pub message: Message,
    pub extra_signatures: Vec<(Pubkey, Signature)>,
}

impl TxRequest {
    /// Convenience constructor for the common case of no extra signers.
    pub fn new(
        description: impl Into<String>,
        cluster: Cluster,
        kind: TxKind,
        message: Message,
    ) -> TxRequest {
        TxRequest {
            description: description.into(),
            cluster,
            kind,
            message,
            extra_signatures: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_and_invoke_are_not_irreversible() {
        assert!(!TxKind::Deploy.is_irreversible());
        assert!(!TxKind::Invoke {
            instruction_name: "swap".into()
        }
        .is_irreversible());
    }

    #[test]
    fn authority_and_fund_movement_are_irreversible() {
        assert!(TxKind::Upgrade.is_irreversible());
        assert!(TxKind::SetAuthority.is_irreversible());
        assert!(TxKind::CloseAccount.is_irreversible());
        assert!(TxKind::Transfer { lamports: 1 }.is_irreversible());
        assert!(TxKind::Payment { lamports: 1 }.is_irreversible());
    }

    #[test]
    fn payment_has_a_stable_policy_name() {
        assert_eq!(TxKind::Payment { lamports: 5 }.name(), "Payment");
    }
}
