use crate::error::GuardError;
use crate::tx_request::TxRequest;

/// How a single account's state changed (or would change) across the
/// simulated transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDelta {
    pub pubkey: String,
    pub lamports_before: u64,
    pub lamports_after: u64,
    pub owner_before: String,
    pub owner_after: String,
    pub created: bool,
    pub closed: bool,
}

/// Result of running a [`Simulator`] over a [`TxRequest`]. A real
/// implementation would come from `simulateTransaction` RPC calls (and,
/// for mainnet, a local fork replay per Phase 0 spec §6 stage 2); this
/// crate only defines the shape simulators must produce, so the rest of
/// the guardrail pipeline can be built and tested against it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationOutcome {
    pub success: bool,
    pub logs: Vec<String>,
    pub compute_units_consumed: u64,
    pub compute_unit_limit: u64,
    pub account_deltas: Vec<AccountDelta>,
    pub error: Option<String>,
}

/// Produces a [`SimulationOutcome`] for a [`TxRequest`] without ever
/// signing or submitting it. Every transaction must go through a
/// `Simulator` before it can reach the approval stage — see
/// [`crate::pipeline::TxGuard::submit`].
pub trait Simulator {
    fn simulate(&self, tx: &TxRequest) -> Result<SimulationOutcome, GuardError>;
}
