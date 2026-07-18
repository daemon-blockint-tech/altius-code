use std::path::PathBuf;

use rmcp::{
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_LEN: usize = 4096;

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
    service: RunningService<RoleClient, ()>,
}

impl AgentLspAttachment {
    pub async fn tool_names(&self) -> Result<Vec<String>, AgentLspError> {
        self.service
            .list_all_tools()
            .await
            .map(|tools| tools.into_iter().map(|tool| tool.name.to_string()).collect())
            .map_err(|error| AgentLspError::Request(error.to_string()))
    }

    pub fn is_closed(&self) -> bool {
        self.service.is_closed()
    }

    pub async fn close(mut self) -> Result<(), AgentLspError> {
        self.service
            .close()
            .await
            .map_err(|error| AgentLspError::Shutdown(error.to_string()))?;
        Ok(())
    }
}

pub async fn attach_agent_lsp(
    config: AgentLspConfig,
) -> Result<AgentLspAttachment, AgentLspError> {
    validate_config(&config)?;
    let command_name = config.command;
    let args = config.args;
    let working_directory = config.working_directory;
    let transport = TokioChildProcess::new(Command::new(command_name).configure(|command| {
        command.args(args);
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }
    }))
    .map_err(|error| AgentLspError::Start(error.to_string()))?;
    let service = ()
        .serve(transport)
        .await
        .map_err(|error| AgentLspError::Start(error.to_string()))?;
    Ok(AgentLspAttachment { service })
}

fn validate_config(config: &AgentLspConfig) -> Result<(), AgentLspError> {
    if config.command.trim().is_empty() || config.command.len() > MAX_ARGUMENT_LEN {
        return Err(AgentLspError::InvalidConfig(
            "command must be a non-empty bounded string".into(),
        ));
    }
    if config.args.len() > MAX_ARGUMENTS {
        return Err(AgentLspError::InvalidConfig("too many arguments".into()));
    }
    if config.args.iter().any(|arg| arg.len() > MAX_ARGUMENT_LEN) {
        return Err(AgentLspError::InvalidConfig(
            "an argument exceeds the length limit".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_command() {
        let error = validate_config(&AgentLspConfig {
            command: " ".into(),
            args: vec![],
            working_directory: None,
        })
        .unwrap_err();
        assert!(matches!(error, AgentLspError::InvalidConfig(_)));
    }
}
