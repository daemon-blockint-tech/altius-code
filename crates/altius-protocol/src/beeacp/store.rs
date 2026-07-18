use std::collections::HashMap;
use std::sync::Arc;

use altius_core::RunId;
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use super::model::{Message, Run, RunStatus};
use crate::error::{ProtocolError, Result};

/// Async, thread-safe storage for BeeAI ACP runs.
///
/// The store is the single authority for state transitions: every status
/// change goes through [`RunStore::transition`], which enforces the strict
/// table in [`RunStatus::can_transition_to`] atomically per run.
#[async_trait]
pub trait RunStore: Send + Sync {
    /// Persist a freshly created run. Fails on a duplicate `run_id`.
    async fn create(&self, run: Run) -> Result<()>;

    /// Fetch a run by id.
    async fn get(&self, run_id: RunId) -> Result<Run>;

    /// Atomically transition a run to `next`, optionally attaching output
    /// or an error message. Returns the updated run.
    async fn transition(
        &self,
        run_id: RunId,
        next: RunStatus,
        output: Option<Vec<Message>>,
        error: Option<String>,
    ) -> Result<Run>;
}

/// In-memory [`RunStore`] backed by a `tokio::sync::RwLock`.
#[derive(Clone, Default)]
pub struct InMemoryRunStore {
    runs: Arc<RwLock<HashMap<RunId, Run>>>,
}

impl InMemoryRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RunStore for InMemoryRunStore {
    async fn create(&self, run: Run) -> Result<()> {
        let mut runs = self.runs.write().await;
        if runs.contains_key(&run.run_id) {
            return Err(ProtocolError::Conflict(format!(
                "run `{}` already exists",
                run.run_id
            )));
        }
        runs.insert(run.run_id, run);
        Ok(())
    }

    async fn get(&self, run_id: RunId) -> Result<Run> {
        self.runs
            .read()
            .await
            .get(&run_id)
            .cloned()
            .ok_or_else(|| ProtocolError::not_found("run", run_id.to_string()))
    }

    async fn transition(
        &self,
        run_id: RunId,
        next: RunStatus,
        output: Option<Vec<Message>>,
        error: Option<String>,
    ) -> Result<Run> {
        let mut runs = self.runs.write().await;
        let run = runs
            .get_mut(&run_id)
            .ok_or_else(|| ProtocolError::not_found("run", run_id.to_string()))?;
        if !run.status.can_transition_to(next) {
            return Err(ProtocolError::InvalidTransition {
                from: run.status.as_str().to_owned(),
                to: next.as_str().to_owned(),
            });
        }
        run.status = next;
        if let Some(output) = output {
            run.output = output;
        }
        if let Some(error) = error {
            run.error = Some(error);
        }
        if next.is_terminal() {
            run.finished_at = Some(Utc::now());
        }
        Ok(run.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_run() -> Run {
        Run::new("altius", vec![Message::user_text("hi")])
    }

    #[tokio::test]
    async fn create_get_round_trip() {
        let store = InMemoryRunStore::new();
        let run = new_run();
        let id = run.run_id;
        store.create(run).await.unwrap();
        let fetched = store.get(id).await.unwrap();
        assert_eq!(fetched.run_id, id);
        assert_eq!(fetched.status, RunStatus::Created);
    }

    #[tokio::test]
    async fn duplicate_create_conflicts() {
        let store = InMemoryRunStore::new();
        let run = new_run();
        store.create(run.clone()).await.unwrap();
        assert!(matches!(
            store.create(run).await,
            Err(ProtocolError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn transition_enforces_table_and_stamps_finish() {
        let store = InMemoryRunStore::new();
        let run = new_run();
        let id = run.run_id;
        store.create(run).await.unwrap();

        // created → completed is forbidden.
        let err = store
            .transition(id, RunStatus::Completed, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidTransition { .. }));

        store
            .transition(id, RunStatus::InProgress, None, None)
            .await
            .unwrap();
        let done = store
            .transition(
                id,
                RunStatus::Completed,
                Some(vec![Message::user_text("done")]),
                None,
            )
            .await
            .unwrap();
        assert_eq!(done.status, RunStatus::Completed);
        assert!(done.finished_at.is_some());
        assert_eq!(done.output.len(), 1);

        // Terminal states are frozen.
        assert!(store
            .transition(id, RunStatus::InProgress, None, None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn store_is_shareable_across_tasks() {
        let store = Arc::new(InMemoryRunStore::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                let run = new_run();
                let id = run.run_id;
                store.create(run).await.unwrap();
                store
                    .transition(id, RunStatus::InProgress, None, None)
                    .await
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
    }
}
