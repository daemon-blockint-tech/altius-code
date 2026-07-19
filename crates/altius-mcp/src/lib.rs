//! MCP server for safe SVM project inspection and local build/test/lint tools.
//!
//! This crate deliberately exposes no deploy, sign, payment, or broadcast tool.
//! Optional client-side attach (`mcp-client` / `agent-lsp` features) talks to
//! external MCP servers over stdio or authenticated streamable HTTP.

mod server;

#[cfg(feature = "mcp-client")]
mod mcp_client;

#[cfg(feature = "agent-lsp")]
mod agent_lsp;

#[cfg(feature = "mcp-client")]
pub use mcp_client::{
    attach_mcp, attach_remote_mcp, AttachedMcp, DiscoveredTool, McpAttachConfig, McpAttachments,
    McpClientError, McpRemoteConfig,
};

#[cfg(feature = "agent-lsp")]
pub use agent_lsp::{attach_agent_lsp, AgentLspAttachment, AgentLspConfig, AgentLspError};

pub use server::{serve_http, serve_stdio, AltiusMcpServer, McpServerError};
