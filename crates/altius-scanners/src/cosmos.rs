//! Native Cosmos/CosmWasm heuristics.

use std::fs;
use std::path::Path;

use altius_findings::{
    ChainFamily, Confidence, Finding, FindingLocation, ScanReport, Severity, SourceProvenance,
    ValidationState,
};

use crate::error::ScannerError;
use crate::scanner::Scanner;
use crate::util::{collect_files, first_line};

pub struct CosmosScanner;

impl Scanner for CosmosScanner {
    fn name(&self) -> &'static str {
        "altius-cosmos-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Cosmos
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report =
            ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Cosmos);
        report.scanners.push(self.name().into());
        for path in collect_files(root, &["rs", "go"], 10)? {
            let contents = fs::read_to_string(&path).map_err(|e| ScannerError::Io(e.to_string()))?;
            let file = path.display().to_string();
            let looks_cosmwasm = contents.contains("cosmwasm")
                || contents.contains("CosmosMsg")
                || contents.contains("IbcMsg");
            if !looks_cosmwasm {
                continue;
            }
            if contents.contains("SystemTime")
                || contents.contains("thread_rng")
                || contents.contains("Instant::now")
            {
                report.push(finding(
                    "cosmos-nondeterminism",
                    Severity::High,
                    "Nondeterministic API",
                    "consensus-path code appears to use nondeterministic time/rng APIs",
                    &file,
                    &contents,
                    if contents.contains("SystemTime") {
                        "SystemTime"
                    } else if contents.contains("thread_rng") {
                        "thread_rng"
                    } else {
                        "Instant::now"
                    },
                ));
            }
            if contents.contains("panic!(") || contents.contains("unwrap()") {
                report.push(finding(
                    "cosmos-nondeterminism",
                    Severity::Medium,
                    "Panic/unwrap in contract path",
                    "panic/unwrap can diverge state across nodes on CosmWasm/SDK paths",
                    &file,
                    &contents,
                    if contents.contains("panic!(") {
                        "panic!("
                    } else {
                        "unwrap()"
                    },
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
        chain: ChainFamily::Cosmos,
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
        ontology_class: Some("CosmosNondeterminism".into()),
        tool: "altius-cosmos-native".into(),
        provenance: SourceProvenance::native("altius-cosmos-native"),
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
    fn flags_nondeterminism() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("contract.rs"),
            "use cosmwasm_std::*; fn x() { let _ = std::time::SystemTime::now(); }",
        )
        .unwrap();
        let report = CosmosScanner.scan(dir.path()).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|f| f.pattern_id == "cosmos-nondeterminism"));
    }
}
