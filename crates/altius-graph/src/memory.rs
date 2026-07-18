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

/// Neo4j-backed [`MemoryStore`] (feature `neo4j`).
///
/// Persists workflow checkpoints as `(:Run)-[:HAS_CHECKPOINT]->(:Checkpoint)`
/// and scratch key/value data as `(:KvEntry)` nodes, mirroring the semantics
/// of [`InMemoryStore`]. Binary payloads are base64-encoded into string
/// properties so they survive the Bolt type system unchanged. Checkpoints are
/// append-only and ordered by a server-side `seq` (`timestamp()`), so
/// [`latest_checkpoint`](MemoryStore::latest_checkpoint) returns the newest.
#[cfg(feature = "neo4j")]
pub struct Neo4jMemoryStore {
    graph: neo4rs::Graph,
}

#[cfg(feature = "neo4j")]
impl From<neo4rs::Error> for MemoryError {
    fn from(error: neo4rs::Error) -> Self {
        MemoryError::Message(format!("neo4j: {error}"))
    }
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

    /// Apply idempotent constraints/indexes this store relies on. Safe to call
    /// repeatedly; run once at startup.
    pub async fn ensure_schema(&self) -> MemoryResult<()> {
        for statement in [
            "CREATE CONSTRAINT kv_entry_key IF NOT EXISTS \
             FOR (k:KvEntry) REQUIRE (k.namespace, k.key) IS UNIQUE",
            "CREATE INDEX checkpoint_seq IF NOT EXISTS \
             FOR (c:Checkpoint) ON (c.seq)",
        ] {
            self.graph.run(neo4rs::query(statement)).await?;
        }
        Ok(())
    }

    /// Expose the underlying driver for adjacent schema work.
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
        use base64::Engine as _;
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (r:Run {id: $run_id}) \
                     CREATE (c:Checkpoint {id: randomUUID(), step_id: $step_id, node: $node, \
                             payload: $payload, seq: timestamp()}) \
                     CREATE (r)-[:HAS_CHECKPOINT]->(c)",
                )
                .param("run_id", run_id.to_string())
                .param("step_id", step_id.to_string())
                .param("node", node.to_owned())
                .param("payload", payload_b64),
            )
            .await?;
        Ok(())
    }

    async fn latest_checkpoint(&self, run_id: &RunId) -> MemoryResult<Option<CheckpointRecord>> {
        use base64::Engine as _;
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Run {id: $run_id})-[:HAS_CHECKPOINT]->(c:Checkpoint) \
                     RETURN c.step_id AS step_id, c.node AS node, c.payload AS payload \
                     ORDER BY c.seq DESC LIMIT 1",
                )
                .param("run_id", run_id.to_string()),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let step_id: String = row
            .get("step_id")
            .map_err(|e| MemoryError::message(e.to_string()))?;
        let node: String = row
            .get("node")
            .map_err(|e| MemoryError::message(e.to_string()))?;
        let payload_b64: String = row
            .get("payload")
            .map_err(|e| MemoryError::message(e.to_string()))?;
        let payload = base64::engine::general_purpose::STANDARD
            .decode(payload_b64.as_bytes())
            .map_err(|e| MemoryError::message(format!("payload base64: {e}")))?;
        let step_id = step_id
            .parse::<uuid::Uuid>()
            .map(StepId::from)
            .map_err(|e| MemoryError::message(format!("step_id: {e}")))?;
        Ok(Some(CheckpointRecord {
            run_id: *run_id,
            step_id,
            node,
            payload,
        }))
    }

    async fn put_kv(&self, namespace: &str, key: &str, value: &[u8]) -> MemoryResult<()> {
        use base64::Engine as _;
        let value_b64 = base64::engine::general_purpose::STANDARD.encode(value);
        self.graph
            .run(
                neo4rs::query("MERGE (k:KvEntry {namespace: $ns, key: $key}) SET k.value = $value")
                    .param("ns", namespace.to_owned())
                    .param("key", key.to_owned())
                    .param("value", value_b64),
            )
            .await?;
        Ok(())
    }

    async fn get_kv(&self, namespace: &str, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        use base64::Engine as _;
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (k:KvEntry {namespace: $ns, key: $key}) RETURN k.value AS value",
                )
                .param("ns", namespace.to_owned())
                .param("key", key.to_owned()),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let value_b64: String = row
            .get("value")
            .map_err(|e| MemoryError::message(e.to_string()))?;
        let value = base64::engine::general_purpose::STANDARD
            .decode(value_b64.as_bytes())
            .map_err(|e| MemoryError::message(format!("value base64: {e}")))?;
        Ok(Some(value))
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
