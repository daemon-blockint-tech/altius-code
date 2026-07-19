use std::path::Path;
use std::time::Instant;

use altius_findings::ChainFamily;
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
            true_negatives: 0,
            critical_high_recall_num: 0,
            critical_high_recall_den: case
                .labels
                .iter()
                .filter(|l| l.severity == "high" || l.severity == "critical")
                .count() as u32,
            latency_ms: 0,
            tool_succeeded: false,
            notes: Some("fixture missing; counted as false negatives".into()),
        });
    }
    let chain = parse_chain(&case.chain);
    let started = Instant::now();
    let report = match registry.scan_chain(&root, chain) {
        Ok(report) => report,
        Err(error) => {
            return Ok(ScoreCard {
                case_id: case.id.clone(),
                true_positives: 0,
                false_negatives: case.labels.len() as u32,
                false_positives: 0,
                true_negatives: 0,
                critical_high_recall_num: 0,
                critical_high_recall_den: case
                    .labels
                    .iter()
                    .filter(|label| label.severity == "high" || label.severity == "critical")
                    .count() as u32,
                latency_ms: elapsed_ms(started),
                tool_succeeded: false,
                notes: Some(format!("scanner failed: {error}")),
            });
        }
    };
    let latency_ms = elapsed_ms(started);

    let mut tp = 0u32;
    let mut fn_count = 0u32;
    let mut ch_tp = 0u32;
    let mut ch_den = 0u32;
    let mut matched_findings = vec![false; report.findings.len()];
    for label in &case.labels {
        let critical_high = label.severity == "high" || label.severity == "critical";
        if critical_high {
            ch_den += 1;
        }
        let matched = report.findings.iter().enumerate().position(|(index, f)| {
            !matched_findings[index]
                && f.pattern_id == label.pattern_id
                && label
                    .file_contains
                    .as_ref()
                    .map(|frag| f.location.file.contains(frag))
                    .unwrap_or(true)
        });
        if let Some(index) = matched {
            matched_findings[index] = true;
            tp += 1;
            if critical_high {
                ch_tp += 1;
            }
        } else {
            fn_count += 1;
        }
    }

    let fp = matched_findings.iter().filter(|matched| !**matched).count() as u32;
    let true_negatives = u32::from(case.expect_clean && report.findings.is_empty());

    Ok(ScoreCard {
        case_id: case.id.clone(),
        true_positives: tp,
        false_negatives: fn_count,
        false_positives: fp,
        true_negatives,
        critical_high_recall_num: ch_tp,
        critical_high_recall_den: ch_den,
        latency_ms,
        tool_succeeded: true,
        notes: None,
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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

    #[test]
    fn builtin_svm_suite_is_offline_and_measurable() {
        let report = score_suite(&GoldSuite::builtin_smoke(), &crate::builtin_fixtures_root())
            .expect("built-in fixtures should scan");
        assert_eq!(report.precision, 1.0);
        assert_eq!(report.recall, 1.0);
        assert_eq!(report.false_positive_rate, 0.0);
        assert_eq!(report.tool_success_rate, 1.0);
        assert_eq!(report.critical_high_recall, "1/1");
        assert!(report.cards.iter().any(|card| card.true_negatives == 1));
        assert_eq!(report.estimated_cost_usd, None);
    }
}
