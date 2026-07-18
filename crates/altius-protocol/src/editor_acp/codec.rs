//! JSON-RPC 2.0 codec for the Editor ACP (Agent Client Protocol).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ProtocolError, Result};
use crate::limits;

/// The only JSON-RPC version this codec speaks.
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC request identifier: a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

impl From<i64> for RequestId {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

impl From<&str> for RequestId {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

/// A JSON-RPC request (has an `id`, expects a response).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: impl Into<RequestId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC notification (no `id`, fire-and-forget).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// `-32601 Method not found`, per the JSON-RPC 2.0 spec.
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
            data: None,
        }
    }

    /// `-32602 Invalid params`, per the JSON-RPC 2.0 spec.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }
}

/// A JSON-RPC response carrying exactly one of `result` or `error`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Any incoming JSON-RPC message, classified by shape.
///
/// Order matters for `untagged`: a request has an `id` and a `method`, a
/// response has an `id` but no `method`, and a notification has a `method`
/// but no `id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

impl JsonRpcMessage {
    /// Decode a single message from raw untrusted bytes, enforcing a size
    /// bound and the JSON-RPC version before anything else.
    pub fn decode(raw: &[u8]) -> Result<Self> {
        if raw.len() > limits::MAX_BODY_BYTES {
            return Err(ProtocolError::validation(
                "jsonrpc message",
                format!("exceeds {} bytes", limits::MAX_BODY_BYTES),
            ));
        }
        let message: Self = serde_json::from_slice(raw)
            .map_err(|e| ProtocolError::validation("jsonrpc message", e.to_string()))?;
        let version = match &message {
            Self::Request(r) => &r.jsonrpc,
            Self::Response(r) => &r.jsonrpc,
            Self::Notification(n) => &n.jsonrpc,
        };
        if version != JSONRPC_VERSION {
            return Err(ProtocolError::validation(
                "jsonrpc",
                format!("unsupported version `{version}`"),
            ));
        }
        if let Self::Request(r) = &message {
            limits::bounded_string("method", &r.method, limits::MAX_NAME_LEN)?;
        }
        if let Self::Notification(n) = &message {
            limits::bounded_string("method", &n.method, limits::MAX_NAME_LEN)?;
        }
        Ok(message)
    }

    /// Encode a message to bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| ProtocolError::Internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trips() {
        let request = JsonRpcRequest::new(1, "initialize", Some(json!({"protocolVersion": 1})));
        let bytes = JsonRpcMessage::Request(request.clone()).encode().unwrap();
        match JsonRpcMessage::decode(&bytes).unwrap() {
            JsonRpcMessage::Request(decoded) => assert_eq!(decoded, request),
            other => panic!("expected request, got {other:?}"),
        }
    }

    #[test]
    fn notification_has_no_id_and_classifies_correctly() {
        let raw = br#"{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"s1"}}"#;
        match JsonRpcMessage::decode(raw).unwrap() {
            JsonRpcMessage::Notification(n) => assert_eq!(n.method, "session/cancel"),
            other => panic!("expected notification, got {other:?}"),
        }
    }

    #[test]
    fn response_classifies_correctly() {
        let raw = br#"{"jsonrpc":"2.0","id":"a","result":{"ok":true}}"#;
        match JsonRpcMessage::decode(raw).unwrap() {
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.id, RequestId::String("a".into()));
                assert!(r.error.is_none());
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[test]
    fn error_response_round_trips() {
        let response = JsonRpcResponse::failure(
            RequestId::Number(7),
            JsonRpcError::method_not_found("nope"),
        );
        let bytes = JsonRpcMessage::Response(response.clone()).encode().unwrap();
        match JsonRpcMessage::decode(&bytes).unwrap() {
            JsonRpcMessage::Response(decoded) => {
                assert_eq!(decoded.error.unwrap().code, -32601);
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_wrong_version_and_garbage() {
        assert!(JsonRpcMessage::decode(br#"{"jsonrpc":"1.0","id":1,"method":"x"}"#).is_err());
        assert!(JsonRpcMessage::decode(b"not json").is_err());
    }

    #[test]
    fn decode_rejects_oversized_messages() {
        let padding = "x".repeat(limits::MAX_BODY_BYTES);
        let raw = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"m","params":"{padding}"}}"#);
        assert!(JsonRpcMessage::decode(raw.as_bytes()).is_err());
    }
}
