use serde::{Deserialize, Serialize};

/// One expected finding label for a fixture path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldLabel {
    pub pattern_id: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_contains: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldCase {
    pub id: String,
    pub path: String,
    pub chain: String,
    pub labels: Vec<GoldLabel>,
    /// Whether the fixture is expected to be clean (no Critical/High).
    #[serde(default)]
    pub expect_clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldSuite {
    pub name: String,
    pub cases: Vec<GoldCase>,
}

impl GoldSuite {
    /// Built-in offline suite covering SVM lint themes + one EVM case shape.
    pub fn builtin_smoke() -> Self {
        Self {
            name: "altius-smoke".into(),
            cases: vec![
                GoldCase {
                    id: "svm-missing-signer".into(),
                    path: "fixtures/svm/missing_signer".into(),
                    chain: "solana".into(),
                    labels: vec![GoldLabel {
                        pattern_id: "svm-missing-signer-check".into(),
                        severity: "medium".into(),
                        file_contains: Some("lib.rs".into()),
                    }],
                    expect_clean: false,
                },
                GoldCase {
                    id: "evm-tx-origin".into(),
                    path: "fixtures/evm/tx_origin".into(),
                    chain: "evm".into(),
                    labels: vec![GoldLabel {
                        pattern_id: "evm-access-control".into(),
                        severity: "medium".into(),
                        file_contains: Some(".sol".into()),
                    }],
                    expect_clean: false,
                },
            ],
        }
    }
}
