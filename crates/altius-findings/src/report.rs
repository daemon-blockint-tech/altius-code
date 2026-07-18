use serde::{Deserialize, Serialize};

use crate::chain::ChainFamily;
use crate::finding::Finding;
use crate::fingerprint::fingerprint_finding;

/// Aggregated output of one or more scanners over a target path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScanReport {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<ChainFamily>,
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scanners: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ScanReport {
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            chain: None,
            findings: Vec::new(),
            scanners: Vec::new(),
            notes: None,
        }
    }

    pub fn with_chain(mut self, chain: ChainFamily) -> Self {
        self.chain = Some(chain);
        self
    }

    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding.with_fingerprint());
    }

    pub fn extend(&mut self, findings: impl IntoIterator<Item = Finding>) {
        for finding in findings {
            self.push(finding);
        }
    }

    /// Deduplicate by fingerprint, keeping the highest-confidence instance.
    pub fn dedupe_by_fingerprint(&mut self) {
        use std::collections::HashMap;
        let mut best: HashMap<String, Finding> = HashMap::new();
        for mut finding in self.findings.drain(..) {
            if finding.fingerprint.is_empty() {
                finding.fingerprint = fingerprint_finding(&finding);
            }
            match best.get(&finding.fingerprint) {
                Some(existing) if existing.confidence >= finding.confidence => {}
                _ => {
                    best.insert(finding.fingerprint.clone(), finding);
                }
            }
        }
        self.findings = best.into_values().collect();
        self.findings.sort_by(|a, b| {
            a.pattern_id
                .cmp(&b.pattern_id)
                .then(a.location.file.cmp(&b.location.file))
        });
    }

    pub fn has_errors(&self) -> bool {
        use crate::severity::Severity;
        self.findings.iter().any(|f| f.severity >= Severity::High)
    }

    /// Compact JSON view used by MCP / agent tool responses (backward compatible
    /// with the previous lint DTO shape).
    pub fn to_lint_compat_json(&self) -> serde_json::Value {
        serde_json::json!({
            "has_errors": self.has_errors(),
            "findings": self.findings.iter().map(|f| serde_json::json!({
                "rule_id": f.pattern_id,
                "severity": legacy_severity_label(f.severity),
                "message": f.description,
                "file": f.location.file,
                "id": f.id,
                "fingerprint": f.fingerprint,
                "confidence": f.confidence.as_str(),
                "chain": f.chain.as_str(),
            })).collect::<Vec<_>>(),
        })
    }
}

fn legacy_severity_label(severity: crate::severity::Severity) -> &'static str {
    use crate::severity::Severity;
    match severity {
        Severity::Info | Severity::Low | Severity::Medium => "warning",
        Severity::High | Severity::Critical => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Finding;

    #[test]
    fn dedupe_keeps_higher_confidence() {
        let mut report = ScanReport::new(".");
        let mut a = Finding::from_lint("svm-missing-signer-check", false, "a", "src/lib.rs");
        let mut b = Finding::from_lint("svm-missing-signer-check", false, "b", "src/lib.rs");
        a.confidence = crate::severity::Confidence::Low;
        b.confidence = crate::severity::Confidence::High;
        // Same fingerprint inputs.
        a.fingerprint.clear();
        b.fingerprint.clear();
        report.push(a);
        report.push(b);
        report.dedupe_by_fingerprint();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.findings[0].confidence,
            crate::severity::Confidence::High
        );
    }

    #[test]
    fn lint_compat_json_preserves_rule_ids() {
        let mut report = ScanReport::new("proj").with_chain(ChainFamily::Solana);
        report.push(Finding::from_lint(
            "svm-arbitrary-cpi",
            false,
            "cpi",
            "src/lib.rs",
        ));
        let json = report.to_lint_compat_json();
        assert_eq!(json["findings"][0]["rule_id"], "svm-arbitrary-cpi");
        assert_eq!(json["findings"][0]["severity"], "warning");
    }
}
