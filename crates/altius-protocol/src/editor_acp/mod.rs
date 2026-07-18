//! **Editor ACP** — the [Agent Client Protocol] (editor ↔ agent).
//!
//! Not to be confused with the BeeAI ACP (Agent Communication Protocol) in
//! [`crate::beeacp`], which is an agent ↔ agent REST run API. Editor ACP is
//! a JSON-RPC 2.0 protocol an IDE speaks to an embedded agent, typically
//! over stdio.
//!
//! This module provides the codec ([`JsonRpcMessage`] and friends) and the
//! typed messages for the three methods Altius needs first:
//!
//! - `initialize` — version / capability negotiation
//! - `session/prompt` — send a user prompt into a session
//! - `session/cancel` — notification cancelling an in-flight prompt
//!
//! Transport (stdio framing, HTTP, …) is intentionally out of scope here.
//!
//! [Agent Client Protocol]: https://agentclientprotocol.com

mod codec;
mod methods;

pub use codec::{JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId, JSONRPC_VERSION};
pub use methods::{
    AgentCapabilities, ClientCapabilities, ContentBlock, InitializeParams, InitializeResult,
    PromptParams, PromptResult, SessionCancelParams, SessionId, StopReason, METHOD_INITIALIZE,
    METHOD_SESSION_CANCEL, METHOD_SESSION_PROMPT,
};
