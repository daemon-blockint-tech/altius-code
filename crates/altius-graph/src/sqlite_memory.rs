//! SQLite-backed [`MemoryStore`] for durable graph checkpoints and scratch kv.
//!
//! Shares the fleet run database file (`runs.db`) with [`altius_protocol::beeacp::SqliteRunStore`]:
//! each store opens its own connection; SQLite handles concurrent readers/writers.

use std::path::Path;
use std::sync::Arc;

use altius_core::{RunId, StepId};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::memory::{CheckpointRecord, MemoryError, MemoryResult, MemoryStore};

/// Namespace for BeeAI ACP run id → graph run id mappings in [`MemoryStore::put_kv`].
pub const BEE_GRAPH_RUN_NS: &str = "beeacp_graph_runs";

/// Persistent [`MemoryStore`] backed by a single-file SQLite database.
#[derive(Clone)]
pub struct SqliteMemoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteMemoryStore {
    /// Open (or create) the database at `path` and ensure the schema exists.
    /// Parent directories are created if missing. On Unix the file is chmod `0600`.
    pub fn open(path: impl AsRef<Path>) -> MemoryResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    MemoryError::message(format!(
                        "create memory-db directory `{}`: {e}",
                        parent.display()
                    ))
                })?;
            }
        }
        let conn = Connection::open(path).map_err(|e| {
            MemoryError::message(format!("open memory db `{}`: {e}", path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
                |e| {
                    MemoryError::message(format!(
                        "secure memory db permissions `{}`: {e}",
                        path.display()
                    ))
                },
            )?;
        }
        Self::from_connection(conn)
    }

    /// In-memory database, useful for tests.
    pub fn open_in_memory() -> MemoryResult<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| MemoryError::message(format!("open in-memory memory db: {e}")))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> MemoryResult<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                run_id   TEXT PRIMARY KEY,
                step_id  TEXT NOT NULL,
                node     TEXT NOT NULL,
                payload  BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS kv (
                namespace TEXT NOT NULL,
                key       TEXT NOT NULL,
                value     BLOB NOT NULL,
                PRIMARY KEY (namespace, key)
            );",
        )
        .map_err(|e| MemoryError::message(format!("sqlite schema: {e}")))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Persist BeeAI ACP run id → supervisor graph run id for checkpoint lookup on resume.
    pub async fn put_bee_graph_run(
        &self,
        bee_run_id: RunId,
        graph_run_id: RunId,
    ) -> MemoryResult<()> {
        self.put_kv(
            BEE_GRAPH_RUN_NS,
            &bee_run_id.to_string(),
            graph_run_id.to_string().as_bytes(),
        )
        .await
    }

    /// Load the graph run id associated with a BeeAI ACP run, if any.
    pub async fn get_bee_graph_run(&self, bee_run_id: RunId) -> MemoryResult<Option<RunId>> {
        let raw = self
            .get_kv(BEE_GRAPH_RUN_NS, &bee_run_id.to_string())
            .await?;
        let Some(bytes) = raw else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes)
            .map_err(|e| MemoryError::message(format!("corrupt bee graph run id: {e}")))?;
        text.parse::<uuid::Uuid>()
            .map(RunId::from)
            .map(Some)
            .map_err(|e| MemoryError::message(format!("parse graph run id: {e}")))
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn put_checkpoint(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        payload: &[u8],
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO checkpoints (run_id, step_id, node, payload)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(run_id) DO UPDATE SET
                 step_id = excluded.step_id,
                 node = excluded.node,
                 payload = excluded.payload",
            params![run_id.to_string(), step_id.to_string(), node, payload,],
        )
        .map_err(|e| MemoryError::message(format!("sqlite put_checkpoint: {e}")))?;
        Ok(())
    }

    async fn latest_checkpoint(&self, run_id: &RunId) -> MemoryResult<Option<CheckpointRecord>> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                "SELECT step_id, node, payload FROM checkpoints WHERE run_id = ?1",
                params![run_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| MemoryError::message(format!("sqlite latest_checkpoint: {e}")))?;
        let Some((step_id_raw, node, payload)) = row else {
            return Ok(None);
        };
        let step_id = step_id_raw
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
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO kv (namespace, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value",
            params![namespace, key, value],
        )
        .map_err(|e| MemoryError::message(format!("sqlite put_kv: {e}")))?;
        Ok(())
    }

    async fn get_kv(&self, namespace: &str, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        let conn = self.conn.lock().await;
        conn.query_row(
            "SELECT value FROM kv WHERE namespace = ?1 AND key = ?2",
            params![namespace, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| MemoryError::message(format!("sqlite get_kv: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_and_kv_round_trip() {
        let store = SqliteMemoryStore::open_in_memory().unwrap();
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

    #[tokio::test]
    async fn checkpoint_overwrites_previous_for_same_run() {
        let store = SqliteMemoryStore::open_in_memory().unwrap();
        let run = RunId::new();
        store
            .put_checkpoint(&run, &StepId::new(), "a", b"1")
            .await
            .unwrap();
        let step2 = StepId::new();
        store.put_checkpoint(&run, &step2, "b", b"2").await.unwrap();
        let latest = store.latest_checkpoint(&run).await.unwrap().unwrap();
        assert_eq!(latest.step_id, step2);
        assert_eq!(latest.node, "b");
        assert_eq!(latest.payload, b"2");
    }

    #[tokio::test]
    async fn bee_graph_run_mapping_round_trip() {
        let store = SqliteMemoryStore::open_in_memory().unwrap();
        let bee = RunId::new();
        let graph = RunId::new();
        store.put_bee_graph_run(bee, graph).await.unwrap();
        assert_eq!(store.get_bee_graph_run(bee).await.unwrap(), Some(graph));
    }

    #[tokio::test]
    async fn survives_reopen_on_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/memory.db");
        let run = RunId::new();
        let step = StepId::new();
        let bee = RunId::new();
        let graph = RunId::new();
        {
            let store = SqliteMemoryStore::open(&path).unwrap();
            store
                .put_checkpoint(&run, &step, "approve", b"state")
                .await
                .unwrap();
            store.put_bee_graph_run(bee, graph).await.unwrap();
        }
        let store = SqliteMemoryStore::open(&path).unwrap();
        let latest = store.latest_checkpoint(&run).await.unwrap().unwrap();
        assert_eq!(latest.node, "approve");
        assert_eq!(latest.payload, b"state");
        assert_eq!(store.get_bee_graph_run(bee).await.unwrap(), Some(graph));
    }

    #[cfg(unix)]
    #[test]
    fn memory_database_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        SqliteMemoryStore::open(&path).unwrap();

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
