//! Native Cairo/Starknet heuristics.

use std::fs;
use std::path::Path;

use altius_findings::{
    ChainFamily, Confidence, Finding, FindingLocation, ScanReport, Severity, SourceProvenance,
    ValidationState,
};

use crate::error::ScannerError;
use crate::scanner::Scanner;
use crate::util::{collect_files, first_line};

pub struct CairoScanner;

impl Scanner for CairoScanner {
    fn name(&self) -> &'static str {
        "altius-cairo-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Cairo
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report = ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Cairo);
        report.scanners.push(self.name().into());
        for path in collect_files(root, &["cairo"], 10)? {
            let contents = fs::read_to_string(&path).map_err(|e| ScannerError::Io(e.to_string()))?;
            let file = path.display().to_string();
            if contents.contains("felt")
                && (contents.contains(" + ") || contents.contains(" * "))
                && !contents.contains("overflow")
                && !contents.contains("checked")
            {
                report.push(finding(
                    "cairo-felt-overflow",
                    Severity::Medium,
                    "Felt arithmetic risk",
                    "felt arithmetic without visible overflow/checked handling",
                    &file,
                    &contents,
                    "felt",
                ));
            }
            if contents.contains("l1_handler") && !contents.contains("from_address") {
                report.push(finding(
                    "cairo-felt-overflow",
                    Severity::High,
                    "L1 handler sender unchecked",
                    "l1_handler without visible from_address validation",
                    &file,
                    &contents,
                    "l1_handler",
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
        chain: ChainFamily::Cairo,
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
        ontology_class: Some("CairoFeltOverflow".into()),
        tool: "altius-cairo-native".into(),
        provenance: SourceProvenance::native("altius-cairo-native"),
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
    fn flags_felt_math() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.cairo"),
            "fn add(a: felt252, b: felt252) -> felt252 { a + b }",
        )
        .unwrap();
        let report = CairoScanner.scan(dir.path()).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|f| f.pattern_id == "cairo-felt-overflow"));
    }
}
