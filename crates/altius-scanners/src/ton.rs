//! Native TON FunC/Tact heuristics.

use std::fs;
use std::path::Path;

use altius_findings::{
    ChainFamily, Confidence, Finding, FindingLocation, ScanReport, Severity, SourceProvenance,
    ValidationState,
};

use crate::error::ScannerError;
use crate::scanner::Scanner;
use crate::util::{collect_files, first_line};

pub struct TonScanner;

impl Scanner for TonScanner {
    fn name(&self) -> &'static str {
        "altius-ton-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Ton
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report = ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Ton);
        report.scanners.push(self.name().into());
        for path in collect_files(root, &["fc", "func", "tact"], 10)? {
            let contents = fs::read_to_string(&path).map_err(|e| ScannerError::Io(e.to_string()))?;
            let file = path.display().to_string();
            if (contents.contains("sender()") || contents.contains("msg.sender"))
                && !contents.contains("equal")
                && !contents.contains("==")
            {
                report.push(finding(
                    "ton-sender-check",
                    Severity::High,
                    "Sender authentication risk",
                    "sender() referenced without equality check against an expected address",
                    &file,
                    &contents,
                    "sender",
                ));
            }
            if contents.contains("transfer_notification")
                || (contents.contains("jetton") && contents.contains("op::"))
            {
                if !contents.contains("sender()") && !contents.contains("msg.sender") {
                    report.push(finding(
                        "ton-sender-check",
                        Severity::Medium,
                        "Jetton notification trust",
                        "jetton/transfer notification path without sender authentication",
                        &file,
                        &contents,
                        "jetton",
                    ));
                }
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
        chain: ChainFamily::Ton,
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
        recommendation: None,
        ontology_class: Some("TonSenderCheck".into()),
        tool: "altius-ton-native".into(),
        provenance: SourceProvenance::native("altius-ton-native"),
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
    fn flags_sender() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.fc"), "() recv() { sender(); }").unwrap();
        let report = TonScanner.scan(dir.path()).unwrap();
        assert!(!report.findings.is_empty());
    }
}
