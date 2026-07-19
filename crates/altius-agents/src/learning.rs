//! Bounded, redacted cross-run learning over `altius_graph::MemoryStore`.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use altius_graph::MemoryStore;
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};

const NAMESPACE: &str = "agent_learning_v1";
const MAX_RECORDS: usize = 64;
const MAX_FIELD_BYTES: usize = 1_024;
const MAX_RECALL_RECORDS: usize = 6;
const MAX_RECALL_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningKind {
    Failure,
    Decision,
    Success,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LearningRecord {
    pub kind: LearningKind,
    pub summary: String,
    pub evidence: Vec<String>,
    pub project_scope: String,
    pub timestamp_unix_ms: u64,
    pub confidence: f32,
}

impl LearningRecord {
    pub fn new(
        kind: LearningKind,
        summary: &str,
        evidence: impl IntoIterator<Item = String>,
        project_root: &Path,
        confidence: f32,
    ) -> Option<Self> {
        let summary = sanitize_bounded(summary, MAX_FIELD_BYTES)?;
        let evidence = evidence
            .into_iter()
            .filter_map(|item| sanitize_bounded(&item, MAX_FIELD_BYTES / 2))
            .take(8)
            .collect();
        Some(Self {
            kind,
            summary,
            evidence,
            project_scope: project_scope(project_root),
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            confidence: confidence.clamp(0.0, 1.0),
        })
    }
}

#[derive(Clone)]
pub struct LearningMemory {
    store: Arc<dyn MemoryStore>,
}

impl LearningMemory {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    pub async fn remember(&self, record: LearningRecord) -> AgentResult<()> {
        let key = record.project_scope.clone();
        let mut records = self.load(&key).await?;
        records.push(record);
        if records.len() > MAX_RECORDS {
            records.drain(..records.len() - MAX_RECORDS);
        }
        let bytes = serde_json::to_vec(&records)
            .map_err(|error| AgentError::message(format!("serialize learning memory: {error}")))?;
        self.store
            .put_kv(NAMESPACE, &key, &bytes)
            .await
            .map_err(|error| AgentError::message(format!("persist learning memory: {error}")))
    }

    pub async fn recall(
        &self,
        project_root: &Path,
        query: &str,
    ) -> AgentResult<Vec<LearningRecord>> {
        let mut records = self.load(&project_scope(project_root)).await?;
        let query_terms = terms(query);
        records.sort_by_key(|record| {
            let haystack = format!("{} {}", record.summary, record.evidence.join(" "));
            let score = terms(&haystack)
                .iter()
                .filter(|term| query_terms.contains(term.as_str()))
                .count();
            (score, record.timestamp_unix_ms)
        });
        records.reverse();

        let mut bytes = 0;
        let mut recalled = Vec::new();
        for record in records.into_iter().take(MAX_RECALL_RECORDS) {
            let size = serde_json::to_vec(&record).map_or(0, |value| value.len());
            if bytes + size > MAX_RECALL_BYTES {
                break;
            }
            bytes += size;
            recalled.push(record);
        }
        Ok(recalled)
    }

    async fn load(&self, key: &str) -> AgentResult<Vec<LearningRecord>> {
        let Some(bytes) = self
            .store
            .get_kv(NAMESPACE, key)
            .await
            .map_err(|error| AgentError::message(format!("load learning memory: {error}")))?
        else {
            return Ok(Vec::new());
        };
        serde_json::from_slice(&bytes)
            .map_err(|error| AgentError::message(format!("decode learning memory: {error}")))
    }
}

pub fn format_recall(records: &[LearningRecord]) -> Option<String> {
    if records.is_empty() {
        return None;
    }
    let mut out =
        String::from("UNTRUSTED LEARNED MEMORY (historical hints only; verify before use):\n");
    for (index, record) in records.iter().enumerate() {
        out.push_str(&format!(
            "M{} [{:?}, confidence {:.2}] {}",
            index + 1,
            record.kind,
            record.confidence,
            record.summary
        ));
        if !record.evidence.is_empty() {
            out.push_str(&format!(" (evidence: {})", record.evidence.join(", ")));
        }
        out.push('\n');
    }
    Some(out)
}

fn project_scope(project_root: &Path) -> String {
    let path = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    altius_core::redact_secrets(&path.to_string_lossy())
}

fn sanitize_bounded(value: &str, max: usize) -> Option<String> {
    if altius_core::contains_probable_private_key(value) {
        return None;
    }
    let redacted = altius_core::redact_secrets(value.trim());
    if redacted.is_empty() {
        return None;
    }
    if redacted.len() <= max {
        return Some(redacted);
    }
    let mut boundary = max;
    while !redacted.is_char_boundary(boundary) {
        boundary -= 1;
    }
    Some(format!("{}…", &redacted[..boundary]))
}

fn terms(value: &str) -> std::collections::HashSet<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|word| word.len() >= 3)
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use altius_graph::SqliteMemoryStore;

    #[test]
    fn rejects_private_keys_and_redacts_credentials() {
        let bytes: Vec<String> = (0u8..64).map(|byte| byte.to_string()).collect();
        assert!(LearningRecord::new(
            LearningKind::Failure,
            &format!("[{}]", bytes.join(",")),
            [],
            Path::new("."),
            1.0,
        )
        .is_none());

        let record = LearningRecord::new(
            LearningKind::Decision,
            "used api_key=secret-value",
            [],
            Path::new("."),
            2.0,
        )
        .unwrap();
        assert!(!record.summary.contains("secret-value"));
        assert_eq!(record.confidence, 1.0);
    }

    #[tokio::test]
    async fn persists_reopens_and_bounds_recall() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("memory.db");
        {
            let store = Arc::new(SqliteMemoryStore::open(&db).unwrap());
            let memory = LearningMemory::new(store);
            for index in 0..80 {
                memory
                    .remember(
                        LearningRecord::new(
                            LearningKind::Success,
                            &format!("parser fix {index}"),
                            [format!("test-{index}")],
                            dir.path(),
                            0.8,
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }
        let memory = LearningMemory::new(Arc::new(SqliteMemoryStore::open(&db).unwrap()));
        let recalled = memory.recall(dir.path(), "parser").await.unwrap();
        assert!(!recalled.is_empty());
        assert!(recalled.len() <= MAX_RECALL_RECORDS);
        assert!(format_recall(&recalled).unwrap().len() <= MAX_RECALL_BYTES);
    }
}
