//! MCP-backed [`OntologyClient`] (feature `mcp`).
//!
//! Speaks to an external OWL/RDF ontology MCP server (open-ontologies style)
//! over a stdio child process, using the same `rmcp` client transport as
//! `altius-mcp`'s agent-lsp attach. The remote server is expected to expose
//! four read-only tools that return JSON matching the schema types:
//!
//! | Tool             | Arguments               | Returns              |
//! |------------------|-------------------------|----------------------|
//! | `list_classes`   | `{}`                    | `[ClassDef]`         |
//! | `describe_class` | `{ "name": str }`       | `ClassDef`           |
//! | `properties_of`  | `{ "class_name": str }` | `[PropertyDef]`      |
//! | `subclasses_of`  | `{ "class_name": str }` | `[ClassDef]`         |
//!
//! # Security posture
//!
//! Everything the server returns is **untrusted input**. Responses are bounded
//! (total byte size, item count, and per-field string length) before
//! deserialization, and the child process is launched with a cleared
//! environment (only `PATH` is forwarded) so it never receives Altius
//! credentials.

use async_trait::async_trait;
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ContentBlock},
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use tokio::process::Command;

use crate::client::{OntologyClient, OntologyError, OntologyResult};
use crate::schema::{ClassDef, PropertyDef};

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_LEN: usize = 4096;

/// Cap on the decoded JSON payload accepted from the remote server.
const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;
/// Cap on the number of classes/properties accepted in one response.
const MAX_ITEMS: usize = 50_000;
/// Cap on any single string field (name, description, …) from the server.
const MAX_STRING_LEN: usize = 64 * 1024;

/// How to launch the external ontology MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOntologyConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
}

/// Live [`OntologyClient`] attached to an external ontology MCP server.
pub struct McpOntologyClient {
    service: RunningService<RoleClient, ()>,
}

impl McpOntologyClient {
    /// Spawn and attach to the configured ontology MCP server.
    pub async fn connect(config: McpOntologyConfig) -> OntologyResult<Self> {
        validate_config(&config)?;
        let path = std::env::var_os("PATH");
        let transport = TokioChildProcess::new(Command::new(config.command).configure(|command| {
            command.env_clear();
            if let Some(path) = path {
                command.env("PATH", path);
            }
            command.args(config.args);
            if let Some(directory) = config.working_directory {
                command.current_dir(directory);
            }
        }))
        .map_err(|e| OntologyError::Message(format!("ontology MCP start: {e}")))?;
        let service = ()
            .serve(transport)
            .await
            .map_err(|e| OntologyError::Message(format!("ontology MCP handshake: {e}")))?;
        Ok(Self { service })
    }

    /// Whether the underlying MCP session has closed.
    pub fn is_closed(&self) -> bool {
        self.service.is_closed()
    }

    /// Gracefully shut the session down.
    pub async fn close(mut self) -> OntologyResult<()> {
        self.service
            .close()
            .await
            .map_err(|e| OntologyError::Message(format!("ontology MCP shutdown: {e}")))?;
        Ok(())
    }

    async fn call_json<T: DeserializeOwned>(
        &self,
        tool: &'static str,
        arguments: Map<String, Value>,
    ) -> OntologyResult<T> {
        let result = self
            .service
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .map_err(|e| OntologyError::Message(format!("ontology MCP call {tool}: {e}")))?;
        let value = extract_json(tool, result)?;
        serde_json::from_value(value)
            .map_err(|e| OntologyError::Message(format!("ontology MCP decode {tool}: {e}")))
    }
}

/// Pull a bounded JSON [`Value`] out of a tool result, preferring
/// `structured_content` and falling back to concatenated text blocks.
fn extract_json(tool: &'static str, result: CallToolResult) -> OntologyResult<Value> {
    if result.is_error.unwrap_or(false) {
        let message = result
            .content
            .iter()
            .filter_map(ContentBlock::as_text)
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(OntologyError::Message(format!(
            "ontology MCP tool {tool} reported an error: {message}"
        )));
    }

    if let Some(structured) = result.structured_content {
        return bound_value(tool, structured);
    }

    let text = result
        .content
        .iter()
        .filter_map(ContentBlock::as_text)
        .map(|t| t.text.as_str())
        .collect::<String>();
    if text.trim().is_empty() {
        return Err(OntologyError::Message(format!(
            "ontology MCP tool {tool} returned no content"
        )));
    }
    if text.len() > MAX_RESULT_BYTES {
        return Err(OntologyError::Message(format!(
            "ontology MCP tool {tool} returned {} bytes; cap is {MAX_RESULT_BYTES}",
            text.len()
        )));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| OntologyError::Message(format!("ontology MCP parse {tool}: {e}")))?;
    bound_value(tool, value)
}

/// Reject responses that exceed the item-count / string-length bounds before
/// they reach `serde_json::from_value` and the rest of the fleet.
fn bound_value(tool: &'static str, value: Value) -> OntologyResult<Value> {
    let mut items = 0usize;
    check_bounds(tool, &value, &mut items)?;
    Ok(value)
}

fn check_bounds(tool: &'static str, value: &Value, items: &mut usize) -> OntologyResult<()> {
    match value {
        Value::String(s) => {
            if s.len() > MAX_STRING_LEN {
                return Err(OntologyError::Message(format!(
                    "ontology MCP tool {tool}: string field exceeds {MAX_STRING_LEN} bytes"
                )));
            }
        }
        Value::Array(arr) => {
            *items += arr.len();
            if *items > MAX_ITEMS {
                return Err(OntologyError::Message(format!(
                    "ontology MCP tool {tool}: response exceeds {MAX_ITEMS} items"
                )));
            }
            for item in arr {
                check_bounds(tool, item, items)?;
            }
        }
        Value::Object(map) => {
            for (_, v) in map {
                check_bounds(tool, v, items)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn one_arg(key: &str, value: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(key.to_owned(), Value::String(value.to_owned()));
    map
}

fn validate_config(config: &McpOntologyConfig) -> OntologyResult<()> {
    if config.command.trim().is_empty() || config.command.len() > MAX_ARGUMENT_LEN {
        return Err(OntologyError::Message(
            "ontology MCP command must be a non-empty bounded string".into(),
        ));
    }
    if config.args.len() > MAX_ARGUMENTS {
        return Err(OntologyError::Message(
            "ontology MCP: too many arguments".into(),
        ));
    }
    if config.args.iter().any(|arg| arg.len() > MAX_ARGUMENT_LEN) {
        return Err(OntologyError::Message(
            "ontology MCP: an argument exceeds the length limit".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl OntologyClient for McpOntologyClient {
    async fn list_classes(&self) -> OntologyResult<Vec<ClassDef>> {
        self.call_json("list_classes", Map::new()).await
    }

    async fn describe_class(&self, name: &str) -> OntologyResult<ClassDef> {
        self.call_json("describe_class", one_arg("name", name))
            .await
    }

    async fn properties_of(&self, class_name: &str) -> OntologyResult<Vec<PropertyDef>> {
        self.call_json("properties_of", one_arg("class_name", class_name))
            .await
    }

    async fn subclasses_of(&self, class_name: &str) -> OntologyResult<Vec<ClassDef>> {
        self.call_json("subclasses_of", one_arg("class_name", class_name))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_command() {
        let err = validate_config(&McpOntologyConfig {
            command: "  ".into(),
            args: vec![],
            working_directory: None,
        })
        .unwrap_err();
        assert!(matches!(err, OntologyError::Message(_)));
    }

    #[test]
    fn bounds_reject_oversized_arrays() {
        let huge = Value::Array(vec![Value::Null; MAX_ITEMS + 1]);
        assert!(bound_value("list_classes", huge).is_err());
    }

    #[test]
    fn bounds_reject_oversized_strings() {
        let big = Value::String("x".repeat(MAX_STRING_LEN + 1));
        assert!(bound_value("describe_class", big).is_err());
    }

    #[test]
    fn bounds_accept_reasonable_payloads() {
        let value = serde_json::json!([
            {"name": "Contract", "description": "an on-chain program", "subclass_of": "Artifact"}
        ]);
        assert!(bound_value("list_classes", value).is_ok());
    }
}
