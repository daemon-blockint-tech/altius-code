//! Native Algorand TEAL/PyTeal heuristics.

use std::fs;
use std::path::Path;

use altius_findings::{
    ChainFamily, Confidence, Finding, FindingLocation, ScanReport, Severity, SourceProvenance,
    ValidationState,
};

use crate::error::ScannerError;
use crate::scanner::Scanner;
use crate::util::{collect_files, first_line};

pub struct AlgorandScanner;

impl Scanner for AlgorandScanner {
    fn name(&self) -> &'static str {
        "altius-algorand-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Algorand
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report =
            ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Algorand);
        report.scanners.push(self.name().into());
        for path in collect_files(root, &["teal", "py"], 10)? {
            let contents =
                fs::read_to_string(&path).map_err(|e| ScannerError::Io(e.to_string()))?;
            let file = path.display().to_string();
            let is_pyteal = file.ends_with(".py")
                && (contents.contains("pyteal") || contents.contains("PyTeal"));
            let is_teal = file.ends_with(".teal");
            if !is_pyteal && !is_teal {
                continue;
            }
            if contents.contains("RekeyTo")
                || contents.contains("rekey_to")
                || contents.contains("CloseRemainderTo")
            {
                if !contents.contains("Global.ZeroAddress")
                    && !contents.contains("Txn.RekeyTo")
                    && !contents.contains("==")
                {
                    report.push(finding(
                        "algorand-rekey-risk",
                        Severity::High,
                        "Rekey/close field risk",
                        "transaction rekey/close fields referenced without zero-address guard",
                        &file,
                        &contents,
                        if contents.contains("rekey_to") {
                            "rekey_to"
                        } else if contents.contains("RekeyTo") {
                            "RekeyTo"
                        } else {
                            "CloseRemainderTo"
                        },
                    ));
                }
            }
            if (contents.contains("GroupSize") || contents.contains("group_size"))
                && !contents.contains("==")
            {
                report.push(finding(
                    "algorand-rekey-risk",
                    Severity::Medium,
                    "Group size unchecked",
                    "group transactions referenced without explicit GroupSize equality check",
                    &file,
                    &contents,
                    "Group",
                ));
            }
        }
        Ok(report)
    }
}

fn finding(
    pattern_id: &str,
    severity: Severity,
    title: &str,
    description: &str,
    file: &str,
    contents: &str,
    needle: &str,
) -> Finding {
    let line = first_line(contents, needle);
    Finding {
        id: format!("{pattern_id}:{file}"),
        chain: ChainFamily::Algorand,
        pattern_id: pattern_id.into(),
        severity,
        confidence: Confidence::Medium,
        title: title.into(),
        description: description.into(),
        location: FindingLocation {
            file: file.into(),
            start_line: line,
            end_line: line,
            start_column: None,
            end_column: None,
            snippet: None,
        },
        evidence: vec![],
        attack_scenario: None,
        recommendation: Some("Force RekeyTo/CloseRemainderTo to ZeroAddress when unused.".into()),
        ontology_class: Some("AlgorandRekeyRisk".into()),
        tool: "altius-algorand-native".into(),
        provenance: SourceProvenance::native("altius-algorand-native"),
        validation: ValidationState::Unverified,
        remediation_refs: vec![],
        fingerprint: String::new(),
    }
    .with_fingerprint()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn flags_rekey() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.teal"), "txn RekeyTo\npop\n").unwrap();
        let report = AlgorandScanner.scan(dir.path()).unwrap();
        assert!(!report.findings.is_empty());
    }
}
