use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::chain::ChainFamily;
use crate::fingerprint::fingerprint_finding;
use crate::severity::{Confidence, Severity};

/// Source of a finding (native Altius rule, optional external tool adapter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenance {
    pub kind: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SourceProvenance {
    pub fn native(tool: impl Into<String>) -> Self {
        Self {
            kind: "native".into(),
            tool: tool.into(),
            tool_version: None,
            rule_source_url: None,
            note: None,
        }
    }
}

/// Local-only PoC / dynamic validation state. Never implies mainnet execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ValidationState {
    #[default]
    Unverified,
    ReproducedLocal,
    Rejected,
}

/// Byte/line span used as evidence for a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingLocation {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_column: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Pointer to a remediation pattern (native check, Vipers-style assert, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationRef {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Canonical security finding shared by all Altius scanners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub chain: ChainFamily,
    pub pattern_id: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub title: String,
    pub description: String,
    pub location: FindingLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology_class: Option<String>,
    pub tool: String,
    pub provenance: SourceProvenance,
    pub validation: ValidationState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remediation_refs: Vec<RemediationRef>,
    /// Stable identity; recomputed by [`Finding::with_fingerprint`] if empty.
    pub fingerprint: String,
}

impl Finding {
    pub fn with_fingerprint(mut self) -> Self {
        if self.fingerprint.is_empty() {
            self.fingerprint = fingerprint_finding(&self);
        }
        self
    }

    /// Convert a legacy SVM lint finding into the canonical model.
    ///
    /// `lint_severity_error` is true when the lint used `Severity::Error`.
    pub fn from_lint(
        rule_id: impl Into<String>,
        lint_severity_error: bool,
        message: impl Into<String>,
        file: impl Into<String>,
    ) -> Self {
        let rule_id = rule_id.into();
        let message = message.into();
        let file = file.into();
        let severity = Severity::from_lint_severity(lint_severity_error);
        let title = rule_id.replace('-', " ");
        Self {
            id: format!(
                "lint:{rule_id}:{}",
                crate::fingerprint::normalize_path(&file)
            ),
            chain: ChainFamily::Solana,
            pattern_id: rule_id.clone(),
            severity,
            confidence: Confidence::Medium,
            title,
            description: message.clone(),
            location: FindingLocation {
                file: file.clone(),
                start_line: None,
                end_line: None,
                start_column: None,
                end_column: None,
                snippet: None,
            },
            evidence: vec![EvidenceSpan {
                file,
                start_line: None,
                end_line: None,
                snippet: Some(message),
                note: Some("legacy lint finding".into()),
            }],
            attack_scenario: None,
            recommendation: None,
            ontology_class: ontology_class_for_rule(&rule_id),
            tool: "altius-svm-tools".into(),
            provenance: SourceProvenance::native("altius-svm-tools"),
            validation: ValidationState::Unverified,
            remediation_refs: vec![],
            fingerprint: String::new(),
        }
        .with_fingerprint()
    }
}

fn ontology_class_for_rule(rule_id: &str) -> Option<String> {
    let class = match rule_id {
        "svm-missing-signer-check" => "MissingSignerCheck",
        "svm-missing-owner-check" => "MissingOwnerCheck",
        "svm-arbitrary-cpi" => "ArbitraryCpi",
        "svm-unvalidated-writable-account" => "UnvalidatedWritableAccount",
        "svm-lamports-overflow-risk" => "LamportsOverflowRisk",
        "svm-close-without-zeroing" => "CloseWithoutZeroing",
        _ => return None,
    };
    Some(class.into())
}

/// Optional metadata stamp for when a finding was first observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationTime {
    pub observed_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_lint_maps_error_to_high() {
        let f = Finding::from_lint(
            "svm-close-without-zeroing",
            true,
            "close without zeroing",
            "src/lib.rs",
        );
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.chain, ChainFamily::Solana);
        assert_eq!(f.ontology_class.as_deref(), Some("CloseWithoutZeroing"));
        assert!(!f.fingerprint.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let f = Finding::from_lint(
            "svm-missing-signer-check",
            false,
            "missing signer",
            "programs/foo/src/lib.rs",
        );
        let json = serde_json::to_string(&f).unwrap();
        let back: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }
}
