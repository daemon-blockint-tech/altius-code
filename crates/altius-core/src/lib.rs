//! Shared primitives for the Altius multi-agent fleet.
//!
//! This crate is intentionally free of SVM / signing dependencies so protocol,
//! graph, and agent layers can depend on it without pulling in Solana crates.

mod budget;
mod error;
mod ids;
mod redact;

pub use budget::Budget;
pub use error::{AltiusError, Result};
pub use ids::{AgentId, CorrelationId, RunId, StepId};
pub use redact::redact_secrets;
