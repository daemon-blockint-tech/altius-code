use std::collections::HashMap;
use std::sync::Arc;

use altius_core::{RunId, StepId};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::{GraphError, GraphResult};
use crate::memory::MemoryStore;
use crate::state::State;

/// A checkpoint captured after a node completes (or interrupts).
#[derive(Clone, Debug)]
pub struct Checkpoint<S> {
    pub run_id: RunId,
    pub step_id: StepId,
    pub node: String,
    pub state: S,
}

/// Persist and reload typed graph state after each node.
#[async_trait]
pub trait Checkpointer<S: State>: Send + Sync {
    async fn put(&self, run_id: &RunId, step_id: &StepId, node: &str, state: &S)
        -> GraphResult<()>;

    async fn latest(&self, run_id: &RunId) -> GraphResult<Option<Checkpoint<S>>>;
}

/// Process-local checkpointer (default for unit tests).
#[derive(Clone, Default)]
pub struct InMemoryCheckpointer<S> {
    inner: Arc<Mutex<HashMap<RunId, Checkpoint<S>>>>,
}

impl<S> InMemoryCheckpointer<S> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl<S: State> Checkpointer<S> for InMemoryCheckpointer<S> {
    async fn put(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        state: &S,
    ) -> GraphResult<()> {
        let mut guard = self.inner.lock().await;
        guard.insert(
            *run_id,
            Checkpoint {
                run_id: *run_id,
                step_id: *step_id,
                node: node.to_owned(),
                state: state.clone(),
            },
        );
        Ok(())
    }

    async fn latest(&self, run_id: &RunId) -> GraphResult<Option<Checkpoint<S>>> {
        let guard = self.inner.lock().await;
        Ok(guard.get(run_id).cloned())
    }
}

/// [`Checkpointer`] adapter that serializes state through a [`MemoryStore`].
pub struct MemoryStoreCheckpointer<S, M> {
    store: M,
    _marker: std::marker::PhantomData<S>,
}

impl<S, M> MemoryStoreCheckpointer<S, M> {
    pub fn new(store: M) -> Self {
        Self {
            store,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn store(&self) -> &M {
        &self.store
    }
}

#[async_trait]
impl<S, M> Checkpointer<S> for MemoryStoreCheckpointer<S, M>
where
    S: State + Serialize + DeserializeOwned,
    M: MemoryStore,
{
    async fn put(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        state: &S,
    ) -> GraphResult<()> {
        let payload = serde_json::to_vec(state)
            .map_err(|e| GraphError::checkpoint(format!("serialize state: {e}")))?;
        self.store
            .put_checkpoint(run_id, step_id, node, &payload)
            .await
            .map_err(|e| GraphError::memory(e.to_string()))
    }

    async fn latest(&self, run_id: &RunId) -> GraphResult<Option<Checkpoint<S>>> {
        let record = self
            .store
            .latest_checkpoint(run_id)
            .await
            .map_err(|e| GraphError::memory(e.to_string()))?;
        match record {
            None => Ok(None),
            Some(rec) => {
                let state: S = serde_json::from_slice(&rec.payload)
                    .map_err(|e| GraphError::checkpoint(format!("deserialize state: {e}")))?;
                Ok(Some(Checkpoint {
                    run_id: rec.run_id,
                    step_id: rec.step_id,
                    node: rec.node,
                    state,
                }))
            }
        }
    }
}
