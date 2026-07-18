//! Redacted JSONL trajectory logging (Phase E hardening).
//!
//! Every agent/tool step can be appended as one JSON line, giving a
//! LangSmith-style trajectory that survives without Neo4j and can be
//! replayed for evals. Secrets are redacted before anything hits disk.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use altius_core::{redact_secrets, RunId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::store::{KnowledgeError, KnowledgeResult};

/// One trajectory event: an agent step, a tool call, or a lifecycle marker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub run_id: RunId,
    pub timestamp: DateTime<Utc>,
    /// Agent role or subsystem emitting the event.
    pub agent: String,
    /// Event kind: `"step"`, `"tool_call"`, `"interrupt"`, `"finalize"`, …
    pub kind: String,
    /// Redacted free-form detail.
    pub detail: String,
}

impl TrajectoryEvent {
    /// Build an event, redacting the detail text.
    pub fn new(run_id: RunId, agent: &str, kind: &str, detail: &str) -> Self {
        Self {
            run_id,
            timestamp: Utc::now(),
            agent: agent.to_owned(),
            kind: kind.to_owned(),
            detail: redact_secrets(detail),
        }
    }
}

/// Append-only JSONL trajectory writer.
pub struct JsonlTrajectoryLogger {
    path: PathBuf,
    file: File,
}

impl JsonlTrajectoryLogger {
    /// Open (creating parent directories if needed) the JSONL file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> KnowledgeResult<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| KnowledgeError::Message(e.to_string()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| KnowledgeError::Message(e.to_string()))?;
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event as a JSON line and flush.
    pub fn append(&mut self, event: &TrajectoryEvent) -> KnowledgeResult<()> {
        let line =
            serde_json::to_string(event).map_err(|e| KnowledgeError::Message(e.to_string()))?;
        self.file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.flush())
            .map_err(|e| KnowledgeError::Message(e.to_string()))
    }
}

/// Read every event back from a trajectory JSONL file.
pub fn read_trajectory(path: impl AsRef<Path>) -> KnowledgeResult<Vec<TrajectoryEvent>> {
    let file = File::open(path.as_ref()).map_err(|e| KnowledgeError::Message(e.to_string()))?;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| KnowledgeError::Message(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        events
            .push(serde_json::from_str(&line).map_err(|e| KnowledgeError::Message(e.to_string()))?);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_through_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traces").join("run.jsonl");
        let run_id = RunId::new();

        let mut logger = JsonlTrajectoryLogger::open(&path).unwrap();
        logger
            .append(&TrajectoryEvent::new(
                run_id,
                "router",
                "step",
                "routed to coder",
            ))
            .unwrap();
        logger
            .append(&TrajectoryEvent::new(
                run_id,
                "coder",
                "tool_call",
                "cargo build",
            ))
            .unwrap();

        let events = read_trajectory(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].agent, "router");
        assert_eq!(events[1].kind, "tool_call");
    }

    #[test]
    fn detail_is_redacted() {
        let event = TrajectoryEvent::new(RunId::new(), "payment", "step", "paid with api_key=xyz");
        assert!(!event.detail.contains("xyz"));
        assert!(event.detail.contains("[REDACTED]"));
    }
}
