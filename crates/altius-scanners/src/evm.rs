//! Native EVM/Solidity heuristics (Wake-inspired themes, Altius-owned rules).

use std::fs;
use std::path::Path;

use altius_findings::{
    ChainFamily, Confidence, Finding, FindingLocation, ScanReport, Severity, SourceProvenance,
    ValidationState,
};

use crate::error::ScannerError;
use crate::scanner::Scanner;
use crate::util::{collect_files, first_line};

pub struct EvmScanner;

impl Scanner for EvmScanner {
    fn name(&self) -> &'static str {
        "altius-evm-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Evm
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report = ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Evm);
        report.scanners.push(self.name().into());
        for path in collect_files(root, &["sol"], 10)? {
            let contents =
                fs::read_to_string(&path).map_err(|e| ScannerError::Io(e.to_string()))?;
            let file = path.display().to_string();
            if contents.contains(".call{") || contents.contains(".call(") {
                if !contents.contains("require(") && !contents.contains("success") {
                    report.push(make(
                        "evm-unchecked-call",
                        Severity::High,
                        "Unchecked low-level call",
                        "low-level call without visible success check",
                        &file,
                        &contents,
                        ".call",
                        "EvmUncheckedCall",
                    ));
                }
            }
            if (contents.contains("call.value") || contents.contains(".call{value:"))
                && contents.contains("balances[")
                && !contents.to_ascii_lowercase().contains("nonreentrant")
            {
                report.push(make(
                    "evm-reentrancy",
                    Severity::Critical,
                    "Possible reentrancy",
                    "external call precedes state effects without nonReentrant guard",
                    &file,
                    &contents,
                    ".call",
                    "EvmReentrancy",
                ));
            }
            if contents.contains("function ")
                && (contents.contains("onlyOwner") || contents.contains("msg.sender"))
                && contents.contains("selfdestruct")
                && !contents.contains("onlyOwner")
            {
                report.push(make(
                    "evm-access-control",
                    Severity::High,
                    "Dangerous privileged op",
                    "selfdestruct without onlyOwner-style access control",
                    &file,
                    &contents,
                    "selfdestruct",
                    "EvmAccessControl",
                ));
            }
            if contents.contains("tx.origin") {
                report.push(make(
                    "evm-access-control",
                    Severity::Medium,
                    "tx.origin auth",
                    "authorization via tx.origin is spoofable by intermediate contracts",
                    &file,
                    &contents,
                    "tx.origin",
                    "EvmAccessControl",
                ));
            }
        }
        Ok(report)
    }
}

fn make(
    pattern_id: &str,
    severity: Severity,
    title: &str,
    description: &str,
    file: &str,
    contents: &str,
    needle: &str,
    ontology: &str,
) -> Finding {
    let line = first_line(contents, needle);
    Finding {
        id: format!("{pattern_id}:{file}:{}", line.unwrap_or(0)),
        chain: ChainFamily::Evm,
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
        ontology_class: Some(ontology.into()),
        tool: "altius-evm-native".into(),
        provenance: SourceProvenance::native("altius-evm-native"),
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
    fn flags_tx_origin() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Vault.sol"),
            "contract V { function w() external { require(tx.origin == owner); } }",
        )
        .unwrap();
        let report = EvmScanner.scan(dir.path()).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|f| f.pattern_id == "evm-access-control"));
    }

    #[test]
    fn clean_contract_has_no_findings() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Safe.sol"),
            "contract S { function x() external view returns (uint256) { return 1; } }",
        )
        .unwrap();
        let report = EvmScanner.scan(dir.path()).unwrap();
        assert!(report.findings.is_empty());
    }
}
