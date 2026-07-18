use std::path::Path;

use altius_findings::{ChainFamily, Severity};
use altius_scanners::{default_registry, ScannerRegistry};
use thiserror::Error;

use crate::gold::{GoldCase, GoldSuite};
use crate::report::{EvalReport, ScoreCard};

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("{0}")]
    Message(String),
}

pub fn score_suite(suite: &GoldSuite, fixtures_root: &Path) -> Result<EvalReport, EvalError> {
    let registry = default_registry();
    let mut cards = Vec::new();
    for case in &suite.cases {
        cards.push(score_case(&registry, fixtures_root, case)?);
    }
    Ok(EvalReport::from_cards(suite.name.clone(), cards))
}

fn score_case(
    registry: &ScannerRegistry,
    fixtures_root: &Path,
    case: &GoldCase,
) -> Result<ScoreCard, EvalError> {
    let root = fixtures_root.join(&case.path);
    if !root.exists() {
        // Synthetic in-memory scoring path for missing fixture dirs: treat as skip.
        return Ok(ScoreCard {
            case_id: case.id.clone(),
            true_positives: 0,
            false_negatives: case.labels.len() as u32,
            false_positives: 0,
            critical_high_recall_num: 0,
            critical_high_recall_den: case
                .labels
                .iter()
                .filter(|l| l.severity == "high" || l.severity == "critical")
                .count() as u32,
            notes: Some("fixture missing; counted as false negatives".into()),
        });
    }
    let chain = parse_chain(&case.chain);
    let report = registry
        .scan_chain(&root, chain)
        .map_err(|e| EvalError::Message(e.to_string()))?;

    let mut tp = 0u32;
    let mut fn_count = 0u32;
    let mut ch_tp = 0u32;
    let mut ch_den = 0u32;
    for label in &case.labels {
        let critical_high = label.severity == "high" || label.severity == "critical";
        if critical_high {
            ch_den += 1;
        }
        let matched = report.findings.iter().any(|f| {
            f.pattern_id == label.pattern_id
                && label
                    .file_contains
                    .as_ref()
                    .map(|frag| f.location.file.contains(frag))
                    .unwrap_or(true)
        });
        if matched {
            tp += 1;
            if critical_high {
                ch_tp += 1;
            }
        } else {
            fn_count += 1;
        }
    }

    let labeled_patterns: Vec<&str> = case.labels.iter().map(|l| l.pattern_id.as_str()).collect();
    let fp = report
        .findings
        .iter()
        .filter(|f| {
            f.severity >= Severity::High && !labeled_patterns.contains(&f.pattern_id.as_str())
        })
        .count() as u32;

    Ok(ScoreCard {
        case_id: case.id.clone(),
        true_positives: tp,
        false_negatives: fn_count,
        false_positives: fp,
        critical_high_recall_num: ch_tp,
        critical_high_recall_den: ch_den,
        notes: None,
    })
}

fn parse_chain(raw: &str) -> ChainFamily {
    match raw.to_ascii_lowercase().as_str() {
        "solana" | "svm" => ChainFamily::Solana,
        "evm" | "solidity" => ChainFamily::Evm,
        "algorand" => ChainFamily::Algorand,
        "cairo" | "starknet" => ChainFamily::Cairo,
        "cosmos" | "cosmwasm" => ChainFamily::Cosmos,
        "ton" => ChainFamily::Ton,
        _ => ChainFamily::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gold::{GoldLabel, GoldSuite};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scores_evm_fixture() {
        let dir = tempdir().unwrap();
        let case_dir = dir.path().join("fixtures/evm/tx_origin");
        fs::create_dir_all(&case_dir).unwrap();
        fs::write(
            case_dir.join("Vault.sol"),
            "contract V { function w() external { require(tx.origin == owner); } }",
        )
        .unwrap();
        let suite = GoldSuite {
            name: "t".into(),
            cases: vec![crate::gold::GoldCase {
                id: "evm".into(),
                path: "fixtures/evm/tx_origin".into(),
                chain: "evm".into(),
                labels: vec![GoldLabel {
                    pattern_id: "evm-access-control".into(),
                    severity: "medium".into(),
                    file_contains: Some(".sol".into()),
                }],
                expect_clean: false,
            }],
        };
        let report = score_suite(&suite, dir.path()).unwrap();
        assert_eq!(report.cards[0].true_positives, 1);
        assert_eq!(report.cards[0].false_negatives, 0);
    }
}
