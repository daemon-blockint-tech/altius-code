use std::collections::HashMap;
use std::sync::Arc;

use altius_core::{RunId, StepId};
use async_trait::async_trait;
use tokio::sync::Mutex;

/// Errors from [`MemoryStore`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("{0}")]
    Message(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

pub type MemoryResult<T> = Result<T, MemoryError>;

impl MemoryError {
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

/// Opaque checkpoint blob persisted by a [`MemoryStore`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointRecord {
    pub run_id: RunId,
    pub step_id: StepId,
    pub node: String,
    pub payload: Vec<u8>,
}

/// Persistence abstraction for workflow checkpoints and scratch key/value data.
///
/// A full `altius-memory` crate lands in Phase D; Phase A keeps the trait here
/// (or in a small module) with an in-memory implementation for tests and a
/// Neo4j stub behind the `neo4j` feature.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Persist a checkpoint blob for `run_id` after `node` completed.
    async fn put_checkpoint(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        payload: &[u8],
    ) -> MemoryResult<()>;

    /// Return the most recent checkpoint for `run_id`, if any.
    async fn latest_checkpoint(&self, run_id: &RunId) -> MemoryResult<Option<CheckpointRecord>>;

    /// Put a namespaced scratch value.
    async fn put_kv(&self, namespace: &str, key: &str, value: &[u8]) -> MemoryResult<()>;

    /// Get a namespaced scratch value.
    async fn get_kv(&self, namespace: &str, key: &str) -> MemoryResult<Option<Vec<u8>>>;
}

#[derive(Default)]
struct InMemoryInner {
    checkpoints: HashMap<RunId, CheckpointRecord>,
    kv: HashMap<(String, String), Vec<u8>>,
}

/// Process-local [`MemoryStore`] used by unit tests and offline CI.
#[derive(Clone, Default)]
pub struct InMemoryStore {
    inner: Arc<Mutex<InMemoryInner>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MemoryStore for InMemoryStore {
    async fn put_checkpoint(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        payload: &[u8],
    ) -> MemoryResult<()> {
        let mut guard = self.inner.lock().await;
        guard.checkpoints.insert(
            *run_id,
            CheckpointRecord {
                run_id: *run_id,
                step_id: *step_id,
                node: node.to_owned(),
                payload: payload.to_vec(),
            },
        );
        Ok(())
    }

    async fn latest_checkpoint(&self, run_id: &RunId) -> MemoryResult<Option<CheckpointRecord>> {
        let guard = self.inner.lock().await;
        Ok(guard.checkpoints.get(run_id).cloned())
    }

    async fn put_kv(&self, namespace: &str, key: &str, value: &[u8]) -> MemoryResult<()> {
        let mut guard = self.inner.lock().await;
        guard
            .kv
            .insert((namespace.to_owned(), key.to_owned()), value.to_vec());
        Ok(())
    }

    async fn get_kv(&self, namespace: &str, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        let guard = self.inner.lock().await;
        Ok(guard
            .kv
            .get(&(namespace.to_owned(), key.to_owned()))
            .cloned())
    }
}

/// Neo4j-backed [`MemoryStore`] stub (feature `neo4j`).
///
/// Compiles against `neo4rs` 0.9 rc and wraps connection setup. Real Cypher
/// persistence is intentionally TODO — Phase D owns the full schema.
#[cfg(feature = "neo4j")]
pub struct Neo4jMemoryStore {
    graph: neo4rs::Graph,
}

#[cfg(feature = "neo4j")]
impl Neo4jMemoryStore {
    /// Connect using a bolt URI and credentials.
    ///
    /// Example URI: `bolt://127.0.0.1:7687`
    pub fn connect(uri: &str, user: &str, password: &str) -> MemoryResult<Self> {
        let config = neo4rs::ConfigBuilder::new()
            .uri(uri)
            .user(user)
            .password(password)
            .build()
            .map_err(|e| MemoryError::message(format!("neo4j config: {e}")))?;
        let graph = neo4rs::Graph::connect(config)
            .map_err(|e| MemoryError::message(format!("neo4j connect: {e}")))?;
        Ok(Self { graph })
    }

    /// Expose the underlying driver for Phase D schema work.
    pub fn graph(&self) -> &neo4rs::Graph {
        &self.graph
    }
}

#[cfg(feature = "neo4j")]
#[async_trait]
impl MemoryStore for Neo4jMemoryStore {
    async fn put_checkpoint(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        payload: &[u8],
    ) -> MemoryResult<()> {
        // Phase D: MERGE (:Run)-[:HAS_CHECKPOINT]->(:Checkpoint {…})
        let _ = (&self.graph, run_id, step_id, node, payload);
        Err(MemoryError::NotImplemented(
            "Neo4jMemoryStore::put_checkpoint — schema lands in Phase D".into(),
        ))
    }

    async fn latest_checkpoint(&self, run_id: &RunId) -> MemoryResult<Option<CheckpointRecord>> {
        let _ = (&self.graph, run_id);
        Err(MemoryError::NotImplemented(
            "Neo4jMemoryStore::latest_checkpoint — schema lands in Phase D".into(),
        ))
    }

    async fn put_kv(&self, namespace: &str, key: &str, value: &[u8]) -> MemoryResult<()> {
        let _ = (&self.graph, namespace, key, value);
        Err(MemoryError::NotImplemented(
            "Neo4jMemoryStore::put_kv — schema lands in Phase D".into(),
        ))
    }

    async fn get_kv(&self, namespace: &str, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        let _ = (&self.graph, namespace, key);
        Err(MemoryError::NotImplemented(
            "Neo4jMemoryStore::get_kv — schema lands in Phase D".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_kv_and_checkpoint_roundtrip() {
        let store = InMemoryStore::new();
        let run = RunId::new();
        let step = StepId::new();
        store
            .put_checkpoint(&run, &step, "router", b"{\"ok\":true}")
            .await
            .unwrap();
        let latest = store.latest_checkpoint(&run).await.unwrap().unwrap();
        assert_eq!(latest.node, "router");
        assert_eq!(latest.payload, b"{\"ok\":true}");

        store.put_kv("scratch", "k", b"v").await.unwrap();
        assert_eq!(
            store.get_kv("scratch", "k").await.unwrap().as_deref(),
            Some(b"v".as_slice())
        );
    }
}
