//! **BeeAI ACP** — the [Agent Communication Protocol] (agent ↔ agent).
//!
//! Not to be confused with the Editor ACP (Agent Client Protocol) in
//! [`crate::editor_acp`]. This module implements the REST run lifecycle:
//! runs move through `created → in-progress → (awaiting ⇄ in-progress) →
//! completed | failed | cancelled` under the strict transition rules
//! encoded in [`RunStatus::can_transition_to`].
//!
//! Actual agent execution is injectable behind [`RunExecutor`]; this module
//! only manages state. Nothing here signs or submits transactions.
//!
//! [Agent Communication Protocol]: https://agentcommunicationprotocol.dev

mod model;
mod routes;
mod store;

pub use model::{Message, MessagePart, Run, RunStatus};
pub use routes::{
    router, BeeAcpState, CreateRunRequest, NoopExecutor, ResumeRunRequest, RunExecutor, RunOutcome,
};
pub use store::{InMemoryRunStore, RunStore};
