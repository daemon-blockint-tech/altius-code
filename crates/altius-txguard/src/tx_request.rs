use altius_svm_detect::Cluster;

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
            TxKind::Upgrade | TxKind::SetAuthority | TxKind::CloseAccount | TxKind::Transfer { .. }
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
            TxKind::Invoke { instruction_name } => instruction_name.clone(),
        }
    }
}

/// A candidate transaction that has not been signed and must not be until
/// it has passed every stage of [`crate::pipeline::TxGuard::submit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRequest {
    /// Human-readable summary shown in approval prompts and audit logs.
    pub description: String,
    pub cluster: Cluster,
    pub kind: TxKind,
    /// Opaque unsigned transaction bytes. A full implementation would
    /// carry a serialized Solana `Transaction`/`VersionedTransaction`
    /// here; this crate treats it as an opaque payload to sign so the
    /// guardrail pipeline can be built and tested ahead of wiring in
    /// `solana-sdk` transaction construction (tracked as follow-up work
    /// in `altius-svm-tools`).
    pub unsigned_transaction: Vec<u8>,
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
    }
}
