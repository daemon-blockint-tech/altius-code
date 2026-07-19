//! Generic MCP client attach for external stdio servers (browser, agent-lsp, …).
//!
//! Child processes are launched with a cleared environment plus an explicit
//! allowlist (`PATH`, `HOME`, and optional extras). Altius credentials are
//! never forwarded. Tool results are treated as untrusted remote content.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ContentBlock, Tool},
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, ConfigureCommandExt,
        StreamableHttpClientTransport, TokioChildProcess,
    },
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::RwLock;

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_LEN: usize = 4096;
const MAX_ATTACHMENTS: usize = 8;
const MAX_NAME_LEN: usize = 64;
const MAX_ENV_EXTRAS: usize = 32;
const MAX_ENV_KEY_LEN: usize = 128;
const MAX_URL_LEN: usize = 2048;
const MAX_RESULT_BYTES: usize = 64 * 1024;

/// Always-forwarded environment keys for MCP child processes.
const DEFAULT_ENV: &[&str] = &["PATH", "HOME"];

/// How to launch an external MCP stdio server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAttachConfig {
    /// Logical attachment name (e.g. `"browser"`).
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    /// Extra environment variable *names* to forward from the parent
    /// (values are read at attach time). Keys like `DISPLAY` /
    /// `XAUTHORITY` are typical for headed browsers.
    #[serde(default)]
    pub env_extras: Vec<String>,
}

/// How to attach to a remote streamable-HTTP MCP server.
///
/// Authentication is read from `authorization_token_env` at connect time.
/// The token value is never serialized into config, logs, or API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoteConfig {
    /// Logical attachment name (for example `"github"`).
    pub name: String,
    /// HTTPS MCP endpoint.
    pub url: String,
    /// Environment variable containing the bearer token.
    pub authorization_token_env: String,
}

/// A tool discovered on an attached MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredTool {
    pub name: String,
    pub description: String,
    /// JSON Schema object for parameters.
    pub parameters: Value,
}

#[derive(Debug, Error)]
pub enum McpClientError {
    #[error("invalid MCP attach configuration: {0}")]
    InvalidConfig(String),
    #[error("failed to start MCP client `{0}`: {1}")]
    Start(String, String),
    #[error("MCP request on `{0}` failed: {1}")]
    Request(String, String),
    #[error("MCP shutdown of `{0}` failed: {1}")]
    Shutdown(String, String),
    #[error("attachment `{0}` not found")]
    NotFound(String),
    #[error("too many MCP attachments (max {MAX_ATTACHMENTS})")]
    TooMany,
    #[error("MCP credential environment variable `{0}` is missing or empty")]
    MissingCredential(String),
}

/// Live attachment to one external MCP server.
pub struct AttachedMcp {
    name: String,
    service: RunningService<RoleClient, ()>,
    tools: Vec<DiscoveredTool>,
}

impl AttachedMcp {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Tools advertised by the server at attach time.
    pub fn tools(&self) -> &[DiscoveredTool] {
        &self.tools
    }

    /// Refresh the cached tool list from the live server.
    pub async fn refresh_tools(&mut self) -> Result<&[DiscoveredTool], McpClientError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|error| McpClientError::Request(self.name.clone(), error.to_string()))?;
        self.tools = tools.into_iter().map(discovered_from_rmcp).collect();
        Ok(&self.tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, McpClientError> {
        let args = match arguments {
            Value::Object(map) => Some(map),
            Value::Null => None,
            other => {
                let mut map = Map::new();
                map.insert("_".into(), other);
                Some(map)
            }
        };
        let mut params = CallToolRequestParams::new(name.to_owned());
        if let Some(args) = args {
            params = params.with_arguments(args);
        }
        let result = self
            .service
            .call_tool(params)
            .await
            .map_err(|error| McpClientError::Request(self.name.clone(), error.to_string()))?;
        extract_result(&self.name, result)
    }

    pub fn is_closed(&self) -> bool {
        self.service.is_closed()
    }

    pub async fn close(mut self) -> Result<(), McpClientError> {
        let name = self.name.clone();
        self.service
            .close()
            .await
            .map_err(|error| McpClientError::Shutdown(name, error.to_string()))?;
        Ok(())
    }
}

/// Registry of named MCP attachments (`"browser"`, `"agent-lsp"`, …).
#[derive(Default)]
pub struct McpAttachments {
    inner: RwLock<HashMap<String, Arc<AttachedMcp>>>,
}

impl McpAttachments {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn attach(
        &self,
        config: McpAttachConfig,
    ) -> Result<Arc<AttachedMcp>, McpClientError> {
        validate_config(&config)?;
        {
            let guard = self.inner.read().await;
            if guard.len() >= MAX_ATTACHMENTS && !guard.contains_key(&config.name) {
                return Err(McpClientError::TooMany);
            }
        }
        let attached = Arc::new(spawn_attached(config).await?);
        let name = attached.name().to_owned();
        self.inner.write().await.insert(name, Arc::clone(&attached));
        Ok(attached)
    }

    pub async fn attach_remote(
        &self,
        config: McpRemoteConfig,
    ) -> Result<Arc<AttachedMcp>, McpClientError> {
        validate_remote_config(&config)?;
        {
            let guard = self.inner.read().await;
            if guard.len() >= MAX_ATTACHMENTS && !guard.contains_key(&config.name) {
                return Err(McpClientError::TooMany);
            }
        }
        let attached = Arc::new(spawn_remote(config).await?);
        let name = attached.name().to_owned();
        self.inner.write().await.insert(name, Arc::clone(&attached));
        Ok(attached)
    }

    pub async fn get(&self, name: &str) -> Option<Arc<AttachedMcp>> {
        self.inner.read().await.get(name).cloned()
    }

    pub async fn names(&self) -> Vec<String> {
        self.inner.read().await.keys().cloned().collect()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    pub async fn remove(&self, name: &str) -> Option<Arc<AttachedMcp>> {
        self.inner.write().await.remove(name)
    }
}

/// Spawn and handshake with one MCP stdio child.
pub async fn attach_mcp(config: McpAttachConfig) -> Result<AttachedMcp, McpClientError> {
    validate_config(&config)?;
    spawn_attached(config).await
}

/// Attach directly to a remote streamable-HTTP MCP endpoint.
pub async fn attach_remote_mcp(config: McpRemoteConfig) -> Result<AttachedMcp, McpClientError> {
    validate_remote_config(&config)?;
    spawn_remote(config).await
}

async fn spawn_attached(config: McpAttachConfig) -> Result<AttachedMcp, McpClientError> {
    let name = config.name.clone();
    let command_name = config.command;
    let args = config.args;
    let working_directory = config.working_directory;
    let env_pairs = collect_env(&config.env_extras);

    let transport = TokioChildProcess::new(Command::new(&command_name).configure(|command| {
        command.env_clear();
        for (key, value) in &env_pairs {
            command.env(key, value);
        }
        command.args(&args);
        if let Some(directory) = &working_directory {
            command.current_dir(directory);
        }
    }))
    .map_err(|error| McpClientError::Start(name.clone(), error.to_string()))?;

    let service = ()
        .serve(transport)
        .await
        .map_err(|error| McpClientError::Start(name.clone(), error.to_string()))?;

    let tools = service
        .list_all_tools()
        .await
        .map_err(|error| McpClientError::Request(name.clone(), error.to_string()))?
        .into_iter()
        .map(discovered_from_rmcp)
        .collect();

    Ok(AttachedMcp {
        name,
        service,
        tools,
    })
}

async fn spawn_remote(config: McpRemoteConfig) -> Result<AttachedMcp, McpClientError> {
    let token = std::env::var(&config.authorization_token_env)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| McpClientError::MissingCredential(config.authorization_token_env.clone()))?;
    let name = config.name;
    let transport_config =
        StreamableHttpClientTransportConfig::with_uri(config.url).auth_header(token);
    let transport = StreamableHttpClientTransport::from_config(transport_config);
    let service = ()
        .serve(transport)
        .await
        .map_err(|error| McpClientError::Start(name.clone(), error.to_string()))?;
    let tools = service
        .list_all_tools()
        .await
        .map_err(|error| McpClientError::Request(name.clone(), error.to_string()))?
        .into_iter()
        .map(discovered_from_rmcp)
        .collect();

    Ok(AttachedMcp {
        name,
        service,
        tools,
    })
}

fn collect_env(extras: &[String]) -> Vec<(String, std::ffi::OsString)> {
    let mut out = Vec::new();
    for key in DEFAULT_ENV {
        if let Some(value) = std::env::var_os(key) {
            out.push(((*key).to_owned(), value));
        }
    }
    for key in extras {
        if DEFAULT_ENV.contains(&key.as_str()) {
            continue;
        }
        if let Some(value) = std::env::var_os(key) {
            out.push((key.clone(), value));
        }
    }
    out
}

fn discovered_from_rmcp(tool: Tool) -> DiscoveredTool {
    DiscoveredTool {
        name: tool.name.to_string(),
        description: tool.description.map(|d| d.to_string()).unwrap_or_default(),
        parameters: Value::Object(tool.input_schema.as_ref().clone()),
    }
}

fn extract_result(attachment: &str, result: CallToolResult) -> Result<Value, McpClientError> {
    if result.is_error.unwrap_or(false) {
        let message = result
            .content
            .iter()
            .filter_map(ContentBlock::as_text)
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(McpClientError::Request(
            attachment.to_owned(),
            format!("tool reported an error: {message}"),
        ));
    }

    if let Some(structured) = result.structured_content {
        return Ok(truncate_value(structured));
    }

    let text = result
        .content
        .iter()
        .filter_map(ContentBlock::as_text)
        .map(|t| t.text.as_str())
        .collect::<String>();
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        return Ok(truncate_value(value));
    }
    Ok(Value::String(truncate_str(&text)))
}

fn truncate_str(value: &str) -> String {
    if value.len() <= MAX_RESULT_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_RESULT_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…[truncated]", &value[..boundary])
}

fn truncate_value(value: Value) -> Value {
    match serde_json::to_string(&value) {
        Ok(raw) if raw.len() > MAX_RESULT_BYTES => Value::String(truncate_str(&raw)),
        Ok(_) => value,
        Err(_) => Value::String("[unserializable tool result]".into()),
    }
}

fn validate_config(config: &McpAttachConfig) -> Result<(), McpClientError> {
    validate_name(&config.name)?;
    if config.command.trim().is_empty() || config.command.len() > MAX_ARGUMENT_LEN {
        return Err(McpClientError::InvalidConfig(
            "command must be a non-empty bounded string".into(),
        ));
    }
    if config.args.len() > MAX_ARGUMENTS {
        return Err(McpClientError::InvalidConfig("too many arguments".into()));
    }
    if config.args.iter().any(|arg| arg.len() > MAX_ARGUMENT_LEN) {
        return Err(McpClientError::InvalidConfig(
            "an argument exceeds the length limit".into(),
        ));
    }
    if config.env_extras.len() > MAX_ENV_EXTRAS {
        return Err(McpClientError::InvalidConfig(
            "too many env_extras entries".into(),
        ));
    }
    if config.env_extras.iter().any(|key| !valid_env_key(key)) {
        return Err(McpClientError::InvalidConfig(
            "env_extras keys must be ASCII environment names and bounded".into(),
        ));
    }
    Ok(())
}

fn validate_remote_config(config: &McpRemoteConfig) -> Result<(), McpClientError> {
    validate_name(&config.name)?;
    if config.url.len() > MAX_URL_LEN || !config.url.starts_with("https://") {
        return Err(McpClientError::InvalidConfig(
            "remote MCP url must be a bounded HTTPS URL".into(),
        ));
    }
    if !valid_env_key(&config.authorization_token_env) {
        return Err(McpClientError::InvalidConfig(
            "authorization_token_env must be a valid bounded ASCII environment name".into(),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), McpClientError> {
    if name.trim().is_empty() || name.len() > MAX_NAME_LEN {
        return Err(McpClientError::InvalidConfig(
            "name must be a non-empty bounded string".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(McpClientError::InvalidConfig(
            "name must be ascii alphanumeric / '-' / '_'".into(),
        ));
    }
    Ok(())
}

fn valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_ENV_KEY_LEN
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_name_and_command() {
        assert!(matches!(
            validate_config(&McpAttachConfig {
                name: " ".into(),
                command: "npx".into(),
                args: vec![],
                working_directory: None,
                env_extras: vec![],
            }),
            Err(McpClientError::InvalidConfig(_))
        ));
        assert!(matches!(
            validate_config(&McpAttachConfig {
                name: "browser".into(),
                command: String::new(),
                args: vec![],
                working_directory: None,
                env_extras: vec![],
            }),
            Err(McpClientError::InvalidConfig(_))
        ));
    }

    #[test]
    fn rejects_unsafe_attachment_names() {
        assert!(validate_config(&McpAttachConfig {
            name: "browser;rm".into(),
            command: "npx".into(),
            args: vec![],
            working_directory: None,
            env_extras: vec![],
        })
        .is_err());
    }

    #[test]
    fn accepts_browser_config() {
        validate_config(&McpAttachConfig {
            name: "browser".into(),
            command: "npx".into(),
            args: vec!["@playwright/mcp@latest".into()],
            working_directory: None,
            env_extras: vec!["DISPLAY".into()],
        })
        .unwrap();
    }

    #[test]
    fn accepts_secure_remote_config_and_rejects_http() {
        validate_remote_config(&McpRemoteConfig {
            name: "github".into(),
            url: "https://api.githubcopilot.com/mcp/".into(),
            authorization_token_env: "GITHUB_TOKEN".into(),
        })
        .unwrap();
        assert!(validate_remote_config(&McpRemoteConfig {
            name: "github".into(),
            url: "http://example.com/mcp".into(),
            authorization_token_env: "GITHUB_TOKEN".into(),
        })
        .is_err());
    }

    #[test]
    fn truncate_str_preserves_utf8() {
        let value = "é".repeat(MAX_RESULT_BYTES);
        let truncated = truncate_str(&value);
        assert!(truncated.ends_with("…[truncated]"));
        assert!(truncated.is_char_boundary(truncated.len()));
    }
}
