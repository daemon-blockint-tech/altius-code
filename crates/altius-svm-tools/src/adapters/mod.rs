//! Optional local tool adapters (clippy, cargo-audit).
//!
//! These are explicitly invoked scanners. When the tool is unavailable they
//! return a structured "unavailable" result — never silent success.

mod cargo_audit;
mod clippy;

pub use cargo_audit::{run_cargo_audit, CargoAuditResult};
pub use clippy::{run_clippy, ClippyResult};

/// Shared outcome when an external tool is missing or fails to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterStatus {
    Ok,
    Unavailable { tool: String, detail: String },
    Failed { tool: String, detail: String },
}
