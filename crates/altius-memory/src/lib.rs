//! Fleet knowledge and state layer (Phase D).
//!
//! Three pieces:
//!
//! - [`schema`] — the Neo4j fleet knowledge-graph schema (`Agent`, `Run`,
//!   `Step`, `Artifact`, `Contract`, `Vulnerability`, `Skill` plus the
//!   call/deploy/pay relationships), defined once so Cypher, docs, and tests
//!   never drift.
//! - [`KnowledgeStore`] — durable cross-session memory of runs, steps, and
//!   artifacts. [`InMemoryKnowledgeStore`] backs unit tests and offline CI;
//!   [`Neo4jKnowledgeStore`] (feature `neo4j`) persists to Neo4j via
//!   `neo4rs`.
//! - [`JsonlTrajectoryLogger`] — redacted JSONL trajectory logging that
//!   works with or without Neo4j.
//!
//! Per-run graph checkpointing stays in `altius-graph`
//! ([`altius_graph::MemoryStore`]); this crate is the durable knowledge
//! complement. Everything persisted here is redacted first — secrets never
//! reach Neo4j or trace files.

pub mod schema;
mod security;
mod store;
mod trajectory;

#[cfg(feature = "neo4j")]
mod neo4j;

pub use schema::{schema_statements, NodeLabel, RelType};
pub use security::{
    aggregate_confidence, aggregate_severity, EvidenceRecord, InMemorySecurityStore,
    MitigationRecord, ScannerRecord, SecurityKnowledge, TargetRecord, VulnerabilityRecord,
};
pub use store::{
    new_run_record, new_step_record, ArtifactRecord, InMemoryKnowledgeStore, KnowledgeError,
    KnowledgeResult, KnowledgeStore, RunRecord, RunStatus, StepRecord,
};
pub use trajectory::{read_trajectory, JsonlTrajectoryLogger, TrajectoryEvent};

#[cfg(feature = "neo4j")]
pub use neo4j::Neo4jKnowledgeStore;
