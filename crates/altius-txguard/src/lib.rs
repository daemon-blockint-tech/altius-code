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
mod known_programs;
mod pipeline;
mod policy;
mod rpc_simulator;
mod simulate;
pub mod test_validator;
pub mod testing;
mod tx_assembly;
mod tx_request;

pub use approval::{ApprovalChannel, ApprovalDecision, AutoApprove, FailClosed};
pub use audit_log::{verify_chain, AuditEntry, AuditLogger};
pub use diff::DiffReport;
pub use error::GuardError;
pub use pipeline::{TxGuard, TxOutcome};
pub use policy::{MainnetPolicy, PolicyConfig, PolicyDecision};
pub use rpc_simulator::RpcSimulator;
pub use simulate::{AccountDelta, SimulationOutcome, Simulator};
pub use test_validator::TestValidator;
pub use tx_assembly::{assemble_for_simulation, assemble_signed_transaction, assemble_transaction};
pub use tx_request::{TxKind, TxRequest};
