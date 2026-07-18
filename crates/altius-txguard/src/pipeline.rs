use altius_signer::SignerClient;

use crate::approval::{ApprovalChannel, ApprovalDecision};
use crate::audit_log::{AuditEntry, AuditLogger};
use crate::diff::DiffReport;
use crate::error::GuardError;
use crate::policy::{PolicyConfig, PolicyDecision};
use crate::simulate::Simulator;
use crate::tx_request::TxRequest;

/// What happened to a transaction that made it all the way through
/// [`TxGuard::submit`] and was approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxOutcome {
    /// Approved and signed by the wired [`SignerClient`].
    Signed { signature: altius_signer::Signature },
    /// Approved, but no signer was configured on this `TxGuard` — useful
    /// for dry runs and tests that only want to exercise policy,
    /// simulation, and approval.
    ApprovedNoSigner,
}

/// Composes the five mandatory stages from Phase 0 spec §6 — policy,
/// simulation, diff, approval, audit log — into the single entry point
/// through which every on-chain transaction must pass. There is no other
/// way to reach the [`SignerClient`] this type wraps: `submit` is the
/// only method that can produce a signature.
pub struct TxGuard<Sim: Simulator, App: ApprovalChannel> {
    policy: PolicyConfig,
    simulator: Sim,
    approval: App,
    audit: AuditLogger,
    signer: Option<SignerClient>,
}

impl<Sim: Simulator, App: ApprovalChannel> TxGuard<Sim, App> {
    pub fn new(policy: PolicyConfig, simulator: Sim, approval: App, audit: AuditLogger) -> Self {
        TxGuard {
            policy,
            simulator,
            approval,
            audit,
            signer: None,
        }
    }

    pub fn with_signer(mut self, signer: SignerClient) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn audit_log_path(&self) -> &std::path::Path {
        self.audit.path()
    }

    /// Runs `tx` through policy, mandatory simulation, diff reporting,
    /// approval, and audit logging, in that order. Every call appends
    /// exactly one audit entry, whether the outcome is a rejection, a
    /// denial, or a signed transaction — see Phase 0 spec §6 stage 5.
    pub fn submit(&mut self, tx: TxRequest) -> Result<TxOutcome, GuardError> {
        // Stage 1: policy.
        let policy_decision = self.policy.evaluate(&tx);
        if let PolicyDecision::Reject(reason) = &policy_decision {
            self.audit.append(AuditEntry::new(
                tx.description.clone(),
                tx.cluster.to_string(),
                tx.kind.name(),
                format!("Reject: {reason}"),
                None,
                "rejected before simulation".to_string(),
                None,
            ))?;
            return Err(GuardError::PolicyRejected(reason.clone()));
        }
        let requires_manual = matches!(policy_decision, PolicyDecision::RequireApproval);

        // Stage 2: simulation is mandatory — there is no branch that
        // skips this call.
        let simulation = self.simulator.simulate(&tx)?;
        if !simulation.success {
            let reason = simulation
                .error
                .clone()
                .unwrap_or_else(|| "simulation reported failure with no error message".to_string());
            self.audit.append(AuditEntry::new(
                tx.description.clone(),
                tx.cluster.to_string(),
                tx.kind.name(),
                format!("{policy_decision:?}"),
                Some(false),
                format!("rejected: simulation failed: {reason}"),
                None,
            ))?;
            return Err(GuardError::SimulationFailed(reason));
        }

        // Stage 3: diff report, built from the simulation output only.
        let diff = DiffReport::from_simulation(&simulation);

        // Stage 4: approval.
        let decision = self
            .approval
            .request_approval(&tx, &diff, requires_manual)?;
        let approved = matches!(decision, ApprovalDecision::Approved);
        let approval_summary = match &decision {
            ApprovalDecision::Approved => "approved".to_string(),
            ApprovalDecision::Denied { reason } => format!("denied: {reason}"),
        };

        let signature = if approved {
            match &self.signer {
                Some(signer) => Some(signer.sign(&tx.unsigned_transaction)?),
                None => None,
            }
        } else {
            None
        };

        // Stage 5: audit log — always written, before returning either
        // outcome.
        self.audit.append(AuditEntry::new(
            tx.description.clone(),
            tx.cluster.to_string(),
            tx.kind.name(),
            format!("{policy_decision:?}"),
            Some(true),
            approval_summary.clone(),
            signature.as_ref().map(|s| s.to_string()),
        ))?;

        if !approved {
            return Err(GuardError::ApprovalDenied(approval_summary));
        }

        Ok(match signature {
            Some(signature) => TxOutcome::Signed { signature },
            None => TxOutcome::ApprovedNoSigner,
        })
    }
}
