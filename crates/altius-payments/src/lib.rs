//! x402 / machine-payment support for the Altius fleet (Phase C).
//!
//! Flow: an agent's HTTP call returns `402 Payment Required` with an x402
//! JSON challenge → [`PaymentChallenge::parse`] → pick a requirement Altius
//! can settle ([`PaymentChallenge::select_solana_requirement`]) → build a
//! [`altius_txguard::TxRequest`] with `TxKind::Payment` → settle **only**
//! through [`settle_via_guard`], which runs the full TxGuard pipeline
//! (policy, mandatory simulation, diff, approval, audit log, isolated
//! signer) → retry the HTTP request with the [`PaymentProof`] `X-PAYMENT`
//! header.
//!
//! Security invariants:
//!
//! - This crate never holds keys and has no direct path to a signer; the
//!   only signing route is `TxGuard::submit`.
//! - `TxKind::Payment` is irreversible and in the default
//!   `deny_instructions`, so payments always at least require approval —
//!   headless `FailClosed` / `AutoApprove` configurations deny them.
//! - Challenge bodies are untrusted remote input and are bounds-checked.
//! - No HTTP client lives here; callers own the request/retry loop so this
//!   crate stays fully unit-testable without a network.

mod challenge;
mod error;
mod settle;

pub use challenge::{
    network_cluster, PaymentChallenge, PaymentRequirements, NATIVE_SOL_MINT, SCHEME_EXACT,
    SUPPORTED_X402_VERSION,
};
pub use error::{PaymentError, PaymentResult};
pub use settle::{build_payment_request, settle_via_guard, PaymentProof, ProofPayload};
