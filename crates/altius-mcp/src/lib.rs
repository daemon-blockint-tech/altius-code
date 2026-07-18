//! MCP server for safe SVM project inspection and local build/test/lint tools.
//!
//! This crate deliberately exposes no deploy, sign, payment, or broadcast tool.

mod server;

#[cfg(feature = "agent-lsp")]
mod agent_lsp;

#[cfg(feature = "agent-lsp")]
pub use agent_lsp::{attach_agent_lsp, AgentLspAttachment, AgentLspConfig};
pub use server::{serve_http, serve_stdio, AltiusMcpServer, McpServerError};
