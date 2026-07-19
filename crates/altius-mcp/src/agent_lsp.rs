//! Compatibility shim around [`crate::mcp_client`] for the historical
//! agent-lsp attachment API.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::mcp_client::{attach_mcp, AttachedMcp, McpAttachConfig, McpClientError};

/// Configuration for an optional external agent-lsp MCP stdio process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLspConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum AgentLspError {
    #[error("invalid agent-lsp configuration: {0}")]
    InvalidConfig(String),
    #[error("failed to start agent-lsp MCP client: {0}")]
    Start(String),
    #[error("agent-lsp MCP request failed: {0}")]
    Request(String),
    #[error("agent-lsp MCP shutdown failed: {0}")]
    Shutdown(String),
}

/// Live attachment to an external agent-lsp MCP server.
///
/// The child process receives no credentials from Altius. Environment
/// inheritance and executable selection remain explicit caller policy.
pub struct AgentLspAttachment {
    inner: Arc<AttachedMcp>,
}

impl AgentLspAttachment {
    pub async fn tool_names(&self) -> Result<Vec<String>, AgentLspError> {
        Ok(self
            .inner
            .tools()
            .iter()
            .map(|tool| tool.name.clone())
            .collect())
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    pub async fn close(self) -> Result<(), AgentLspError> {
        match Arc::try_unwrap(self.inner) {
            Ok(attached) => attached.close().await.map_err(map_error),
            Err(_) => Ok(()),
        }
    }

    pub fn inner(&self) -> &Arc<AttachedMcp> {
        &self.inner
    }
}

pub async fn attach_agent_lsp(config: AgentLspConfig) -> Result<AgentLspAttachment, AgentLspError> {
    let attached = attach_mcp(McpAttachConfig {
        name: "agent-lsp".into(),
        command: config.command,
        args: config.args,
        working_directory: config.working_directory,
        env_extras: vec![],
    })
    .await
    .map_err(map_error)?;
    Ok(AgentLspAttachment {
        inner: Arc::new(attached),
    })
}

fn map_error(error: McpClientError) -> AgentLspError {
    match error {
        McpClientError::InvalidConfig(message) => AgentLspError::InvalidConfig(message),
        McpClientError::Start(_, message) => AgentLspError::Start(message),
        McpClientError::TooMany => AgentLspError::Start("too many MCP attachments".into()),
        McpClientError::Request(_, message) => AgentLspError::Request(message),
        McpClientError::NotFound(name) => AgentLspError::Request(format!("not found: {name}")),
        McpClientError::Shutdown(_, message) => AgentLspError::Shutdown(message),
        McpClientError::MissingCredential(env_var) => AgentLspError::InvalidConfig(format!(
            "missing credential environment variable `{env_var}`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_empty_command() {
        let result = attach_agent_lsp(AgentLspConfig {
            command: " ".into(),
            args: vec![],
            working_directory: None,
        })
        .await;
        assert!(matches!(result, Err(AgentLspError::InvalidConfig(_))));
    }
}
