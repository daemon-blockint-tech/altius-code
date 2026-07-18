use crate::diff::DiffReport;
use crate::error::GuardError;
use crate::tx_request::TxRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// Asked, at stage 4 of the pipeline, whether a transaction that has
/// already passed policy and simulation may proceed to signing.
pub trait ApprovalChannel {
    fn request_approval(
        &self,
        tx: &TxRequest,
        diff: &DiffReport,
        requires_manual: bool,
    ) -> Result<ApprovalDecision, GuardError>;
}

/// Approves automatically — but only for transactions that are neither
/// targeting mainnet-beta nor irreversible, and only when policy hasn't
/// separately flagged the transaction via `requires_manual`. The refusal
/// lives inside this implementation on purpose: wiring `AutoApprove`
/// everywhere by mistake still cannot rubber-stamp a mainnet or
/// irreversible action, because this type itself declines to. See Phase
/// 0 spec §6 stage 4.
pub struct AutoApprove;

impl ApprovalChannel for AutoApprove {
    fn request_approval(
        &self,
        tx: &TxRequest,
        _diff: &DiffReport,
        requires_manual: bool,
    ) -> Result<ApprovalDecision, GuardError> {
        if requires_manual || tx.cluster.is_mainnet() || tx.kind.is_irreversible() {
            return Ok(ApprovalDecision::Denied {
                reason: "AutoApprove refuses mainnet, irreversible, or policy-flagged \
                         transactions; a human approval channel is required"
                    .to_string(),
            });
        }
        Ok(ApprovalDecision::Approved)
    }
}

/// The headless-mode default. A headless run has no interactive channel
/// to prompt a human, so per Phase 0 spec §6 stage 4 it fails closed —
/// denying the transaction — rather than blocking indefinitely or, worse,
/// approving without anyone actually looking at it.
pub struct FailClosed;

impl ApprovalChannel for FailClosed {
    fn request_approval(
        &self,
        _tx: &TxRequest,
        _diff: &DiffReport,
        _requires_manual: bool,
    ) -> Result<ApprovalDecision, GuardError> {
        Ok(ApprovalDecision::Denied {
            reason: "no interactive approval channel is configured (headless fail-closed default)"
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_request::TxKind;
    use altius_svm_detect::Cluster;

    fn tx(cluster: Cluster, kind: TxKind) -> TxRequest {
        TxRequest {
            description: "test".into(),
            cluster,
            kind,
            unsigned_transaction: vec![],
        }
    }

    fn diff() -> DiffReport {
        DiffReport {
            lamport_deltas: vec![],
            accounts_created: vec![],
            accounts_closed: vec![],
            owner_changes: vec![],
            compute_units_consumed: 0,
            compute_unit_limit: 200_000,
        }
    }

    #[test]
    fn auto_approve_allows_benign_devnet_invoke() {
        let request = tx(
            Cluster::Devnet,
            TxKind::Invoke {
                instruction_name: "ping".into(),
            },
        );
        let decision = AutoApprove
            .request_approval(&request, &diff(), false)
            .unwrap();
        assert_eq!(decision, ApprovalDecision::Approved);
    }

    #[test]
    fn auto_approve_refuses_mainnet_even_if_misconfigured() {
        let request = tx(Cluster::MainnetBeta, TxKind::Deploy);
        let decision = AutoApprove
            .request_approval(&request, &diff(), false)
            .unwrap();
        assert!(matches!(decision, ApprovalDecision::Denied { .. }));
    }

    #[test]
    fn auto_approve_refuses_irreversible_kinds() {
        let request = tx(Cluster::Devnet, TxKind::CloseAccount);
        let decision = AutoApprove
            .request_approval(&request, &diff(), false)
            .unwrap();
        assert!(matches!(decision, ApprovalDecision::Denied { .. }));
    }

    #[test]
    fn auto_approve_refuses_when_policy_flags_manual() {
        let request = tx(
            Cluster::Devnet,
            TxKind::Invoke {
                instruction_name: "ping".into(),
            },
        );
        let decision = AutoApprove
            .request_approval(&request, &diff(), true)
            .unwrap();
        assert!(matches!(decision, ApprovalDecision::Denied { .. }));
    }

    #[test]
    fn fail_closed_denies_everything() {
        let request = tx(Cluster::Localnet, TxKind::Deploy);
        let decision = FailClosed
            .request_approval(&request, &diff(), false)
            .unwrap();
        assert!(matches!(decision, ApprovalDecision::Denied { .. }));
    }
}
