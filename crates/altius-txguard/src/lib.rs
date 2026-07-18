//! The transaction guardrail: every on-chain transaction Altius Code
//! could submit is required to pass through [`pipeline::TxGuard::submit`],
//! which enforces five stages in order — policy, mandatory simulation, a
//! human-readable diff, approval, and a tamper-evident audit log — before
//! anything reaches a signer.
//!
//! See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §6 in the repo root
//! for the design this crate implements.

mod approval;
mod audit_log;
mod diff;
mod error;
mod pipeline;
mod policy;
mod simulate;
pub mod testing;
mod tx_request;

pub use approval::{ApprovalChannel, ApprovalDecision, AutoApprove, FailClosed};
pub use audit_log::{verify_chain, AuditEntry, AuditLogger};
pub use diff::DiffReport;
pub use error::GuardError;
pub use pipeline::{TxGuard, TxOutcome};
pub use policy::{MainnetPolicy, PolicyConfig, PolicyDecision};
pub use simulate::{AccountDelta, SimulationOutcome, Simulator};
pub use tx_request::{TxKind, TxRequest};
