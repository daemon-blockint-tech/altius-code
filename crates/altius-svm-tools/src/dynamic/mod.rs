//! Local-only dynamic validation interface for SVM programs.
//!
//! Feature-gated behind `dynamic`. Never talks to mainnet. Optional Trident
//! interoperability is limited to an installed local executable — Trident is
//! never vendored.

use altius_findings::ValidationState;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DynamicError {
    #[error("dynamic validation is disabled (enable the `dynamic` feature)")]
    Disabled,
    #[error("dynamic harness error: {0}")]
    Harness(String),
    #[error("external tool unavailable: {0}")]
    ToolUnavailable(String),
}

/// A single invariant or sequence step the harness can attempt locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicCase {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicCaseResult {
    pub name: String,
    pub validation: ValidationState,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DynamicReport {
    pub results: Vec<DynamicCaseResult>,
}

/// Trait for local dynamic scanners (stateful sequence / invariant harness).
pub trait DynamicScanner: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, cases: &[DynamicCase]) -> Result<DynamicReport, DynamicError>;
}

/// Altius-owned placeholder harness: marks cases `Unverified` unless a local
/// probe succeeds. No network, no signing.
#[derive(Debug, Default)]
pub struct LocalSequenceHarness;

impl DynamicScanner for LocalSequenceHarness {
    fn name(&self) -> &'static str {
        "altius-local-sequence"
    }

    fn run(&self, cases: &[DynamicCase]) -> Result<DynamicReport, DynamicError> {
        let mut results = Vec::new();
        for case in cases {
            // Without a compiled fixture + local SVM runtime attachment, we
            // deliberately stay Unverified rather than claiming reproduction.
            results.push(DynamicCaseResult {
                name: case.name.clone(),
                validation: ValidationState::Unverified,
                detail: "local harness scaffold: no PoC binary attached".into(),
            });
        }
        Ok(DynamicReport { results })
    }
}

/// Optional adapter that shells out to an installed `trident` binary.
/// Disabled unless the caller opts in; never downloads Trident.
#[derive(Debug, Clone)]
pub struct OptionalTridentAdapter {
    pub binary: String,
}

impl OptionalTridentAdapter {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl DynamicScanner for OptionalTridentAdapter {
    fn name(&self) -> &'static str {
        "trident-optional"
    }

    fn run(&self, _cases: &[DynamicCase]) -> Result<DynamicReport, DynamicError> {
        match std::process::Command::new(&self.binary)
            .arg("--version")
            .output()
        {
            Ok(out) if out.status.success() => Ok(DynamicReport {
                results: vec![DynamicCaseResult {
                    name: "trident-probe".into(),
                    validation: ValidationState::Unverified,
                    detail: format!(
                        "trident present ({}); PoC execution not auto-run",
                        String::from_utf8_lossy(&out.stdout).trim()
                    ),
                }],
            }),
            Ok(out) => Err(DynamicError::ToolUnavailable(format!(
                "trident exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ))),
            Err(error) => Err(DynamicError::ToolUnavailable(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_harness_marks_unverified() {
        let harness = LocalSequenceHarness;
        let report = harness
            .run(&[DynamicCase {
                name: "close-revival".into(),
                description: "attempt account revival sequence".into(),
            }])
            .unwrap();
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].validation, ValidationState::Unverified);
    }
}
