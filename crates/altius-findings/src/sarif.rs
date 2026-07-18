//! Minimal SARIF 2.1.0 emitter for CI integrations.

use serde_json::{json, Value};

use crate::report::ScanReport;
use crate::severity::Severity;

/// Convert a [`ScanReport`] into a SARIF 2.1.0 JSON value.
pub fn to_sarif(report: &ScanReport) -> Value {
    let rules: Vec<Value> = {
        let mut seen = std::collections::BTreeSet::new();
        report
            .findings
            .iter()
            .filter(|f| seen.insert(f.pattern_id.clone()))
            .map(|f| {
                json!({
                    "id": f.pattern_id,
                    "shortDescription": { "text": f.title },
                    "fullDescription": { "text": f.description },
                    "defaultConfiguration": {
                        "level": sarif_level(f.severity)
                    }
                })
            })
            .collect()
    };

    let results: Vec<Value> = report
        .findings
        .iter()
        .map(|f| {
            let mut result = json!({
                "ruleId": f.pattern_id,
                "level": sarif_level(f.severity),
                "message": { "text": f.description },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": f.location.file },
                        "region": {
                            "startLine": f.location.start_line.unwrap_or(1)
                        }
                    }
                }]
            });
            if let Some(end) = f.location.end_line {
                result["locations"][0]["physicalLocation"]["region"]["endLine"] = json!(end);
            }
            result
        })
        .collect();

    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "altius",
                    "informationUri": "https://github.com/daemon-blockint-tech/altius-code",
                    "rules": rules
                }
            },
            "results": results
        }]
    })
}

fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "note",
        Severity::Low | Severity::Medium => "warning",
        Severity::High | Severity::Critical => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Finding;

    #[test]
    fn emits_sarif_with_results() {
        let mut report = ScanReport::new(".");
        report.push(Finding::from_lint(
            "svm-missing-signer-check",
            false,
            "missing signer",
            "src/lib.rs",
        ));
        let sarif = to_sarif(&report);
        assert_eq!(sarif["version"], "2.1.0");
        assert_eq!(
            sarif["runs"][0]["results"][0]["ruleId"],
            "svm-missing-signer-check"
        );
    }
}
