//! **A2A** — the [Agent-to-Agent protocol] for opaque agent interoperability.
//!
//! Rather than depending on an unstable third-party crate, this module
//! defines custom serde models shaped after the A2A specification
//! (camelCase fields, kebab-case task states, `kind`-tagged message parts)
//! so the wire format stays interoperable:
//!
//! - [`AgentCard`] with [`AgentSkill`] / [`AgentCapabilities`], served at
//!   the well-known path `/.well-known/agent-card.json`
//! - [`Task`] / [`TaskStatus`] / [`A2aMessage`] / [`Artifact`] models with
//!   **opaque payloads** ([`Part::Data`] carries arbitrary bounded JSON)
//! - an Axum route delegating to an injectable [`TaskHandler`]
//!
//! Remote messages are untrusted: all inbound content is bounds-checked
//! and opaque payloads are size-capped before a handler ever sees them.
//!
//! [Agent-to-Agent protocol]: https://github.com/a2aproject/A2A

mod model;
mod routes;

pub use model::{
    A2aMessage, AgentCapabilities, AgentCard, AgentSkill, Artifact, Part, Task, TaskStatus,
    TaskState,
};
pub use routes::{router, A2aState, EchoTaskHandler, SendMessageRequest, TaskHandler, AGENT_CARD_PATH};
