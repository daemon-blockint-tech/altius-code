//! Protocol surfaces for the Altius multi-agent fleet (Phase B).
//!
//! Two unrelated protocols share the "ACP" acronym; this crate keeps them in
//! clearly separated modules and they must never be conflated:
//!
//! - [`beeacp`] — **BeeAI ACP**, the [Agent Communication Protocol]
//!   (agent ↔ agent). A REST run lifecycle (`/runs` create / get / cancel /
//!   resume) with strict state transitions:
//!   `created | in-progress | awaiting | completed | failed | cancelled`.
//! - [`editor_acp`] — **Editor ACP**, the [Agent Client Protocol]
//!   (editor ↔ agent). A JSON-RPC 2.0 codec plus typed `initialize`,
//!   `session/prompt`, and `session/cancel` messages for IDE embedding.
//!
//! The remaining modules cover agent interoperability and discovery:
//!
//! - [`a2a`] — Agent-to-Agent protocol: standards-shaped Agent Card,
//!   skills/capabilities, and task/message/artifact models with opaque task
//!   payloads, served over Axum with an injectable task handler.
//! - [`anp`] — Agent Network Protocol: agent descriptions, `did:wba`
//!   parsing with a fail-closed verification stub, and a local in-memory
//!   registry for register/discover.
//!
//! ## Trust boundaries
//!
//! Everything arriving over these surfaces is **untrusted remote content**.
//! All inbound strings and JSON payloads pass through the bounded validation
//! helpers in [`limits`], HTTP bodies are size-capped, and cryptographic
//! `did:wba` verification fails closed until a real implementation lands.
//! This crate performs no signing and no on-chain actions; run execution and
//! task handling are injectable traits implemented elsewhere.
//!
//! [Agent Communication Protocol]: https://agentcommunicationprotocol.dev
//! [Agent Client Protocol]: https://agentclientprotocol.com

pub mod a2a;
pub mod anp;
pub mod beeacp;
pub mod editor_acp;
mod error;
pub mod limits;

pub use error::{ProtocolError, Result};
