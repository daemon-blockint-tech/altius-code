//! SQLite-backed [`RunStore`] so BeeAI ACP runs survive process restarts.
//!
//! One `rusqlite::Connection` guarded by a `tokio::sync::Mutex`; statements
//! are short and synchronous, so holding the async mutex across them is
//! acceptable for the fleet-serve workload. The same strict transition
//! table as [`super::store::InMemoryRunStore`] is enforced while the row
//! lock (mutex) is held, keeping transitions atomic per store.

use std::path::Path;
use std::sync::Arc;

use altius_core::RunId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use super::model::{Message, Run, RunStatus};
use super::store::RunStore;
use crate::error::{ProtocolError, Result};

/// Persistent [`RunStore`] backed by a single-file SQLite database.
#[derive(Clone)]
pub struct SqliteRunStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRunStore {
    /// Open (or create) the database at `path` and ensure the schema exists.
    /// Parent directories are created if missing.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ProtocolError::Internal(format!(
                        "create run-db directory `{}`: {e}",
                        parent.display()
                    ))
                })?;
            }
        }
        let conn = Connection::open(path)
            .map_err(|e| ProtocolError::Internal(format!("open run db `{}`: {e}", path.display())))?;
        Self::from_connection(conn)
    }

    /// In-memory database, useful for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| ProtocolError::Internal(format!("open in-memory run db: {e}")))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS runs (
                run_id      TEXT PRIMARY KEY,
                agent_name  TEXT NOT NULL,
                status      TEXT NOT NULL,
                input_json  TEXT NOT NULL,
                output_json TEXT NOT NULL,
                error       TEXT,
                created_at  TEXT NOT NULL,
                finished_at TEXT
            );",
        )
        .map_err(internal)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

fn internal(error: rusqlite::Error) -> ProtocolError {
    ProtocolError::Internal(format!("sqlite: {error}"))
}

fn status_from_str(raw: &str) -> Result<RunStatus> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| ProtocolError::Internal(format!("unknown run status `{raw}` in db")))
}

fn messages_from_json(raw: &str) -> Result<Vec<Message>> {
    serde_json::from_str(raw)
        .map_err(|e| ProtocolError::Internal(format!("corrupt messages json in db: {e}")))
}

fn datetime_from_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| ProtocolError::Internal(format!("corrupt timestamp `{raw}` in db: {e}")))
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<(String, String, String, String, String, Option<String>, String, Option<String>)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn decode_run(
    (run_id, agent_name, status, input_json, output_json, error, created_at, finished_at): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
    ),
) -> Result<Run> {
    Ok(Run {
        run_id: run_id
            .parse()
            .map_err(|_| ProtocolError::Internal(format!("corrupt run_id `{run_id}` in db")))?,
        agent_name,
        status: status_from_str(&status)?,
        input: messages_from_json(&input_json)?,
        output: messages_from_json(&output_json)?,
        error,
        created_at: datetime_from_rfc3339(&created_at)?,
        finished_at: finished_at.as_deref().map(datetime_from_rfc3339).transpose()?,
    })
}

const SELECT_COLUMNS: &str =
    "run_id, agent_name, status, input_json, output_json, error, created_at, finished_at";

fn get_run(conn: &Connection, run_id: RunId) -> Result<Run> {
    let row = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM runs WHERE run_id = ?1"),
            params![run_id.to_string()],
            run_from_row,
        )
        .optional()
        .map_err(internal)?
        .ok_or_else(|| ProtocolError::not_found("run", run_id.to_string()))?;
    decode_run(row)
}

#[async_trait]
impl RunStore for SqliteRunStore {
    async fn create(&self, run: Run) -> Result<()> {
        let conn = self.conn.lock().await;
        let input_json = serde_json::to_string(&run.input)
            .map_err(|e| ProtocolError::Internal(format!("serialize input: {e}")))?;
        let output_json = serde_json::to_string(&run.output)
            .map_err(|e| ProtocolError::Internal(format!("serialize output: {e}")))?;
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO runs
                 (run_id, agent_name, status, input_json, output_json, error, created_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    run.run_id.to_string(),
                    run.agent_name,
                    run.status.as_str(),
                    input_json,
                    output_json,
                    run.error,
                    run.created_at.to_rfc3339(),
                    run.finished_at.map(|t| t.to_rfc3339()),
                ],
            )
            .map_err(internal)?;
        if inserted == 0 {
            return Err(ProtocolError::Conflict(format!(
                "run `{}` already exists",
                run.run_id
            )));
        }
        Ok(())
    }

    async fn get(&self, run_id: RunId) -> Result<Run> {
        let conn = self.conn.lock().await;
        get_run(&conn, run_id)
    }

    async fn list(&self) -> Result<Vec<Run>> {
        let conn = self.conn.lock().await;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM runs ORDER BY created_at DESC, run_id DESC"
            ))
            .map_err(internal)?;
        let rows = statement
            .query_map([], run_from_row)
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?;
        rows.into_iter().map(decode_run).collect()
    }

    async fn transition(
        &self,
        run_id: RunId,
        next: RunStatus,
        output: Option<Vec<Message>>,
        error: Option<String>,
    ) -> Result<Run> {
        // The mutex serializes read-check-write, so the transition table is
        // enforced atomically per store instance.
        let conn = self.conn.lock().await;
        let mut run = get_run(&conn, run_id)?;
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
        let output_json = serde_json::to_string(&run.output)
            .map_err(|e| ProtocolError::Internal(format!("serialize output: {e}")))?;
        conn.execute(
            "UPDATE runs SET status = ?2, output_json = ?3, error = ?4, finished_at = ?5
             WHERE run_id = ?1",
            params![
                run.run_id.to_string(),
                run.status.as_str(),
                output_json,
                run.error,
                run.finished_at.map(|t| t.to_rfc3339()),
            ],
        )
        .map_err(internal)?;
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_run() -> Run {
        Run::new("altius", vec![Message::user_text("hi")])
    }

    fn tempfile_store() -> (tempfile::TempDir, SqliteRunStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteRunStore::open(dir.path().join("nested/runs.db")).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn create_get_round_trip_on_tempfile() {
        let (_dir, store) = tempfile_store();
        let run = new_run();
        let id = run.run_id;
        store.create(run).await.unwrap();
        let fetched = store.get(id).await.unwrap();
        assert_eq!(fetched.run_id, id);
        assert_eq!(fetched.agent_name, "altius");
        assert_eq!(fetched.status, RunStatus::Created);
        assert_eq!(fetched.input, vec![Message::user_text("hi")]);
        assert!(fetched.finished_at.is_none());
    }

    #[tokio::test]
    async fn duplicate_create_conflicts() {
        let (_dir, store) = tempfile_store();
        let run = new_run();
        store.create(run.clone()).await.unwrap();
        assert!(matches!(
            store.create(run).await,
            Err(ProtocolError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn missing_run_is_not_found() {
        let (_dir, store) = tempfile_store();
        assert!(matches!(
            store.get(RunId::new()).await,
            Err(ProtocolError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn transition_enforces_table_and_stamps_finish() {
        let (_dir, store) = tempfile_store();
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

        // The update persisted, not just the returned copy.
        let fetched = store.get(id).await.unwrap();
        assert_eq!(fetched.status, RunStatus::Completed);
        assert!(fetched.finished_at.is_some());

        // Terminal states are frozen.
        assert!(store
            .transition(id, RunStatus::InProgress, None, None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn list_returns_newest_first() {
        let (_dir, store) = tempfile_store();
        let older = new_run();
        let older_id = older.run_id;
        store.create(older).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let newer = new_run();
        let newer_id = newer.run_id;
        store.create(newer).await.unwrap();
        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].run_id, newer_id);
        assert_eq!(listed[1].run_id, older_id);
    }

    #[tokio::test]
    async fn survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.db");
        let run = new_run();
        let id = run.run_id;
        {
            let store = SqliteRunStore::open(&path).unwrap();
            store.create(run).await.unwrap();
            store
                .transition(id, RunStatus::InProgress, None, None)
                .await
                .unwrap();
        }
        let store = SqliteRunStore::open(&path).unwrap();
        let fetched = store.get(id).await.unwrap();
        assert_eq!(fetched.status, RunStatus::InProgress);
    }
}
