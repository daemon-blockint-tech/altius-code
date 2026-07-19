//! Tokio-based agent graph runtime.
//!
//! Inspired by LangGraph-style orchestration (nodes, edges, checkpoints,
//! fan-out/fan-in, HITL interrupts) but implemented from scratch for Altius.
//! This crate does **not** depend on the `rust-langgraph` crate.

mod checkpointer;
mod error;
mod executor;
mod graph;
mod memory;
mod node;
mod sqlite_memory;
mod state;

pub use checkpointer::{Checkpoint, Checkpointer, InMemoryCheckpointer, MemoryStoreCheckpointer};
pub use error::{GraphError, GraphResult};
pub use executor::{ExecutionOutcome, GraphExecutor};
pub use graph::{EdgeRouter, Graph, GraphBuilder, JoinReducer};
pub use memory::{CheckpointRecord, InMemoryStore, MemoryError, MemoryResult, MemoryStore};
pub use node::{Node, NodeResult};
pub use sqlite_memory::{SqliteMemoryStore, BEE_GRAPH_RUN_NS};
pub use state::State;

#[cfg(feature = "neo4j")]
pub use memory::Neo4jMemoryStore;
