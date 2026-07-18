//! Knowledge-store abstraction over the fleet graph schema.
//!
//! [`KnowledgeStore`] is the durable, cross-session complement to the
//! per-run checkpointing in `altius-graph`: runs, steps, artifacts, and the
//! relationships between them. The in-memory implementation backs unit tests
//! and offline CI; the Neo4j implementation (feature `neo4j`) persists the
//! same records with the schema in [`crate::schema`].

use std::collections::HashMap;
use std::sync::Arc;

use altius_core::redact_secrets;
use altius_core::{RunId, StepId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Errors from [`KnowledgeStore`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("{0}")]
    Message(String),

    #[error("unknown run {0}")]
    UnknownRun(RunId),
}

pub type KnowledgeResult<T> = Result<T, KnowledgeError>;

/// One user task execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: RunId,
    /// Redacted before persistence — never store raw prompts with secrets.
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub status: RunStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    InProgress,
    Completed,
    Failed,
}

/// One agent step inside a run (`(:Agent)-[:EXECUTED]->(:Step)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub run_id: RunId,
    pub step_id: StepId,
    /// Agent role that executed the step (e.g. `"coder"`).
    pub agent: String,
    /// Redacted, human-readable summary of what happened.
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

/// Something a step produced (`(:Step)-[:PRODUCED]->(:Artifact)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub run_id: RunId,
    pub step_id: StepId,
    /// Artifact kind: `"patch"`, `"report"`, `"plan"`, `"tx_signature"`, …
    pub kind: String,
    /// Redacted artifact body or reference.
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// Durable, cross-session fleet memory.
#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn record_run(&self, run: RunRecord) -> KnowledgeResult<()>;

    async fn set_run_status(&self, run_id: &RunId, status: RunStatus) -> KnowledgeResult<()>;

    async fn record_step(&self, step: StepRecord) -> KnowledgeResult<()>;

    async fn record_artifact(&self, artifact: ArtifactRecord) -> KnowledgeResult<()>;

    async fn run(&self, run_id: &RunId) -> KnowledgeResult<Option<RunRecord>>;

    async fn steps(&self, run_id: &RunId) -> KnowledgeResult<Vec<StepRecord>>;

    async fn artifacts(&self, run_id: &RunId) -> KnowledgeResult<Vec<ArtifactRecord>>;
}

/// Build a [`RunRecord`] with the prompt redacted.
pub fn new_run_record(run_id: RunId, prompt: &str) -> RunRecord {
    RunRecord {
        run_id,
        prompt: redact_secrets(prompt),
        created_at: Utc::now(),
        status: RunStatus::InProgress,
    }
}

/// Build a [`StepRecord`] with the summary redacted.
pub fn new_step_record(run_id: RunId, step_id: StepId, agent: &str, summary: &str) -> StepRecord {
    StepRecord {
        run_id,
        step_id,
        agent: agent.to_owned(),
        summary: redact_secrets(summary),
        created_at: Utc::now(),
    }
}

#[derive(Default)]
struct InMemoryInner {
    runs: HashMap<RunId, RunRecord>,
    steps: Vec<StepRecord>,
    artifacts: Vec<ArtifactRecord>,
}

/// Process-local [`KnowledgeStore`] for unit tests and offline CI.
#[derive(Clone, Default)]
pub struct InMemoryKnowledgeStore {
    inner: Arc<Mutex<InMemoryInner>>,
}

impl InMemoryKnowledgeStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl KnowledgeStore for InMemoryKnowledgeStore {
    async fn record_run(&self, run: RunRecord) -> KnowledgeResult<()> {
        self.inner.lock().await.runs.insert(run.run_id, run);
        Ok(())
    }

    async fn set_run_status(&self, run_id: &RunId, status: RunStatus) -> KnowledgeResult<()> {
        let mut guard = self.inner.lock().await;
        let run = guard
            .runs
            .get_mut(run_id)
            .ok_or(KnowledgeError::UnknownRun(*run_id))?;
        run.status = status;
        Ok(())
    }

    async fn record_step(&self, step: StepRecord) -> KnowledgeResult<()> {
        let mut guard = self.inner.lock().await;
        if !guard.runs.contains_key(&step.run_id) {
            return Err(KnowledgeError::UnknownRun(step.run_id));
        }
        guard.steps.push(step);
        Ok(())
    }

    async fn record_artifact(&self, artifact: ArtifactRecord) -> KnowledgeResult<()> {
        let mut guard = self.inner.lock().await;
        if !guard.runs.contains_key(&artifact.run_id) {
            return Err(KnowledgeError::UnknownRun(artifact.run_id));
        }
        guard.artifacts.push(artifact);
        Ok(())
    }

    async fn run(&self, run_id: &RunId) -> KnowledgeResult<Option<RunRecord>> {
        Ok(self.inner.lock().await.runs.get(run_id).cloned())
    }

    async fn steps(&self, run_id: &RunId) -> KnowledgeResult<Vec<StepRecord>> {
        Ok(self
            .inner
            .lock()
            .await
            .steps
            .iter()
            .filter(|step| step.run_id == *run_id)
            .cloned()
            .collect())
    }

    async fn artifacts(&self, run_id: &RunId) -> KnowledgeResult<Vec<ArtifactRecord>> {
        Ok(self
            .inner
            .lock()
            .await
            .artifacts
            .iter()
            .filter(|artifact| artifact.run_id == *run_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_step_artifact_lifecycle() {
        let store = InMemoryKnowledgeStore::new();
        let run_id = RunId::new();
        let step_id = StepId::new();

        store
            .record_run(new_run_record(run_id, "lint the vault program"))
            .await
            .unwrap();
        store
            .record_step(new_step_record(run_id, step_id, "coder", "ran clippy"))
            .await
            .unwrap();
        store
            .record_artifact(ArtifactRecord {
                run_id,
                step_id,
                kind: "report".into(),
                content: "no warnings".into(),
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        store
            .set_run_status(&run_id, RunStatus::Completed)
            .await
            .unwrap();

        let run = store.run(&run_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(store.steps(&run_id).await.unwrap().len(), 1);
        assert_eq!(store.artifacts(&run_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn prompts_and_summaries_are_redacted() {
        let run = new_run_record(RunId::new(), "deploy with api_key=super-secret");
        assert!(!run.prompt.contains("super-secret"));
        let step = new_step_record(run.run_id, StepId::new(), "coder", "token=abc123 used");
        assert!(!step.summary.contains("abc123"));
    }

    #[tokio::test]
    async fn steps_for_unknown_run_are_rejected() {
        let store = InMemoryKnowledgeStore::new();
        let err = store
            .record_step(new_step_record(RunId::new(), StepId::new(), "coder", "x"))
            .await
            .unwrap_err();
        assert!(matches!(err, KnowledgeError::UnknownRun(_)));
    }
}
