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
    /// Built-in, Altius-owned, offline SVM suite with vulnerable and clean
    /// cross-file projects.
    pub fn builtin_smoke() -> Self {
        Self {
            name: "altius-svm-smoke".into(),
            cases: vec![
                GoldCase {
                    id: "svm-cross-file-attack-path".into(),
                    path: "fixtures/svm/vulnerable_cross_file".into(),
                    chain: "solana".into(),
                    labels: vec![
                        GoldLabel {
                            pattern_id: "svm-missing-signer-check".into(),
                            severity: "medium".into(),
                            file_contains: Some("authority.rs".into()),
                        },
                        GoldLabel {
                            pattern_id: "svm-arbitrary-cpi".into(),
                            severity: "medium".into(),
                            file_contains: Some("cpi.rs".into()),
                        },
                        GoldLabel {
                            pattern_id: "svm-close-without-zeroing".into(),
                            severity: "high".into(),
                            file_contains: Some("close.rs".into()),
                        },
                    ],
                    expect_clean: false,
                },
                GoldCase {
                    id: "svm-checked-clean".into(),
                    path: "fixtures/svm/clean_checked".into(),
                    chain: "solana".into(),
                    labels: vec![],
                    expect_clean: true,
                },
            ],
        }
    }
}
