//! Canonical security finding models for the Altius multi-chain fleet.
//!
//! This crate is intentionally free of chain-tool and signing dependencies so
//! scanners, agents, MCP, eval, and persistence can share one finding shape.

mod chain;
mod finding;
mod fingerprint;
mod report;
mod severity;

pub use chain::ChainFamily;
pub use finding::{
    EvidenceSpan, Finding, FindingLocation, RemediationRef, SourceProvenance, ValidationState,
};
pub use fingerprint::{fingerprint_finding, normalize_path};
pub use report::ScanReport;
pub use severity::{Confidence, Severity};
