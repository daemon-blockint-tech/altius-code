//! Typed messages for the Editor ACP methods Altius implements first.
//!
//! Field names use camelCase on the wire, matching the Agent Client
//! Protocol schema.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::limits;

/// `initialize` — version and capability negotiation (editor → agent).
pub const METHOD_INITIALIZE: &str = "initialize";
/// `session/new` — create a conversation session (editor → agent).
pub const METHOD_SESSION_NEW: &str = "session/new";
/// `session/prompt` — send a user prompt into a session (editor → agent).
pub const METHOD_SESSION_PROMPT: &str = "session/prompt";
/// `session/cancel` — notification cancelling an in-flight prompt.
pub const METHOD_SESSION_CANCEL: &str = "session/cancel";

/// Opaque session identifier assigned by the agent.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn validate(&self) -> Result<()> {
        limits::bounded_string("sessionId", &self.0, limits::MAX_NAME_LEN)
    }
}

/// Capabilities the editor (client) advertises during `initialize`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientCapabilities {
    /// Whether the editor exposes filesystem read/write to the agent.
    pub fs: bool,
    /// Whether the editor can run terminal commands for the agent.
    pub terminal: bool,
}

/// Capabilities the agent advertises back during `initialize`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCapabilities {
    /// Whether the agent supports loading previously persisted sessions.
    pub load_session: bool,
}

/// Params for [`METHOD_INITIALIZE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Latest protocol version the editor supports.
    pub protocol_version: u16,
    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
}

/// Result for [`METHOD_INITIALIZE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Protocol version the agent settled on.
    pub protocol_version: u16,
    #[serde(default)]
    pub agent_capabilities: AgentCapabilities,
}

/// Params for [`METHOD_SESSION_NEW`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewSessionParams {
    /// Absolute working directory for the session, when provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl NewSessionParams {
    pub fn validate(&self) -> Result<()> {
        limits::bounded_opt_string("cwd", self.cwd.as_deref(), limits::MAX_PATH_LEN)
    }
}

/// Result for [`METHOD_SESSION_NEW`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResult {
    pub session_id: SessionId,
}

/// A block of prompt content. Only text is supported initially; the enum is
/// extensible for images / resources later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text { text: String },
}

impl ContentBlock {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Text { text } => limits::bounded_string("text", text, limits::MAX_TEXT_LEN),
        }
    }
}

/// Params for [`METHOD_SESSION_PROMPT`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptParams {
    pub session_id: SessionId,
    pub prompt: Vec<ContentBlock>,
}

impl PromptParams {
    /// Bounded validation for untrusted editor input.
    pub fn validate(&self) -> Result<()> {
        self.session_id.validate()?;
        limits::bounded_list("prompt", self.prompt.len(), limits::MAX_LIST_LEN)?;
        for block in &self.prompt {
            block.validate()?;
        }
        Ok(())
    }
}

/// Why a prompt turn stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Refusal,
    Cancelled,
}

/// Result for [`METHOD_SESSION_PROMPT`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResult {
    pub stop_reason: StopReason,
}

/// Params for the [`METHOD_SESSION_CANCEL`] notification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: SessionId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_acp::{JsonRpcMessage, JsonRpcRequest};
    use serde_json::json;

    #[test]
    fn initialize_round_trips_camel_case() {
        let params = InitializeParams {
            protocol_version: 1,
            client_capabilities: ClientCapabilities {
                fs: true,
                terminal: false,
            },
        };
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            json!({
                "protocolVersion": 1,
                "clientCapabilities": { "fs": true, "terminal": false },
            })
        );
        let back: InitializeParams = serde_json::from_value(value).unwrap();
        assert_eq!(back, params);
    }

    #[test]
    fn prompt_request_encodes_as_typed_jsonrpc() {
        let params = PromptParams {
            session_id: SessionId("sess-1".into()),
            prompt: vec![ContentBlock::Text {
                text: "fix the build".into(),
            }],
        };
        params.validate().unwrap();
        let request = JsonRpcRequest::new(
            1,
            METHOD_SESSION_PROMPT,
            Some(serde_json::to_value(&params).unwrap()),
        );
        let bytes = JsonRpcMessage::Request(request).encode().unwrap();
        let JsonRpcMessage::Request(decoded) = JsonRpcMessage::decode(&bytes).unwrap() else {
            panic!("expected request");
        };
        assert_eq!(decoded.method, METHOD_SESSION_PROMPT);
        let back: PromptParams = serde_json::from_value(decoded.params.unwrap()).unwrap();
        assert_eq!(back, params);
    }

    #[test]
    fn prompt_params_are_bounded() {
        let params = PromptParams {
            session_id: SessionId("sess-1".into()),
            prompt: vec![ContentBlock::Text {
                text: "x".repeat(limits::MAX_TEXT_LEN + 1),
            }],
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn stop_reason_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&PromptResult {
                stop_reason: StopReason::EndTurn,
            })
            .unwrap(),
            r#"{"stopReason":"end_turn"}"#
        );
    }

    #[test]
    fn cancel_params_round_trip() {
        let params = SessionCancelParams {
            session_id: SessionId("sess-9".into()),
        };
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({ "sessionId": "sess-9" }));
    }
}
