//! Bounded validation for untrusted remote content.
//!
//! Every protocol surface in this crate accepts input from remote peers
//! (agents, editors, registries). All inbound strings, lists, and opaque
//! JSON payloads must pass through these helpers before being stored or
//! forwarded, and every Axum router applies [`MAX_BODY_BYTES`] as a
//! request-body cap.

use serde_json::Value;

use crate::error::{ProtocolError, Result};

/// Maximum accepted HTTP request body, applied via `DefaultBodyLimit`.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// Maximum length for short identifiers and names (agent names, skill ids).
pub const MAX_NAME_LEN: usize = 128;

/// Maximum length for human-readable descriptions.
pub const MAX_DESCRIPTION_LEN: usize = 2_048;

/// Maximum length for free-form text content (prompts, message parts).
pub const MAX_TEXT_LEN: usize = 16 * 1024;

/// Maximum length for URLs and URIs.
pub const MAX_URL_LEN: usize = 2_048;

/// Maximum length for a DID string.
pub const MAX_DID_LEN: usize = 512;

/// Maximum number of elements in inbound lists (skills, parts, endpoints).
pub const MAX_LIST_LEN: usize = 64;

/// Maximum serialized size for an opaque JSON payload embedded in a message.
pub const MAX_OPAQUE_JSON_BYTES: usize = 128 * 1024;

/// Validate a required string: non-empty, within `max` bytes, and free of
/// control characters (which have no business in protocol metadata).
pub fn bounded_string(field: &'static str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        return Err(ProtocolError::validation(field, "must not be empty"));
    }
    if value.len() > max {
        return Err(ProtocolError::validation(
            field,
            format!("exceeds {max} bytes"),
        ));
    }
    if value.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(ProtocolError::validation(
            field,
            "contains control characters",
        ));
    }
    Ok(())
}

/// Validate an optional string with the same rules as [`bounded_string`].
pub fn bounded_opt_string(field: &'static str, value: Option<&str>, max: usize) -> Result<()> {
    match value {
        Some(v) => bounded_string(field, v, max),
        None => Ok(()),
    }
}

/// Reject lists longer than `max` elements.
pub fn bounded_list(field: &'static str, len: usize, max: usize) -> Result<()> {
    if len > max {
        return Err(ProtocolError::validation(
            field,
            format!("exceeds {max} elements"),
        ));
    }
    Ok(())
}

/// Validate that a URL parses and is http(s), without following it.
pub fn bounded_url(field: &'static str, value: &str) -> Result<url::Url> {
    bounded_string(field, value, MAX_URL_LEN)?;
    let parsed = url::Url::parse(value)
        .map_err(|e| ProtocolError::validation(field, format!("not a valid URL: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(ProtocolError::validation(
            field,
            format!("unsupported URL scheme `{other}`"),
        )),
    }
}

/// Bound the serialized size of an opaque JSON payload we store or forward.
pub fn bounded_opaque_json(field: &'static str, value: &Value) -> Result<()> {
    // Serialization cost is acceptable at these sizes and gives an exact
    // bound regardless of the payload's shape.
    let bytes = serde_json::to_vec(value)
        .map_err(|e| ProtocolError::Internal(format!("serializing {field}: {e}")))?;
    if bytes.len() > MAX_OPAQUE_JSON_BYTES {
        return Err(ProtocolError::validation(
            field,
            format!("exceeds {MAX_OPAQUE_JSON_BYTES} serialized bytes"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_empty_and_oversized_strings() {
        assert!(bounded_string("name", "", MAX_NAME_LEN).is_err());
        assert!(bounded_string("name", &"x".repeat(MAX_NAME_LEN + 1), MAX_NAME_LEN).is_err());
        assert!(bounded_string("name", "ok", MAX_NAME_LEN).is_ok());
    }

    #[test]
    fn rejects_control_characters() {
        assert!(bounded_string("name", "a\u{0000}b", MAX_NAME_LEN).is_err());
        // Newlines and tabs are fine in text content.
        assert!(bounded_string("text", "line one\nline two", MAX_TEXT_LEN).is_ok());
    }

    #[test]
    fn url_scheme_is_restricted() {
        assert!(bounded_url("url", "https://example.com/agent").is_ok());
        assert!(bounded_url("url", "file:///etc/passwd").is_err());
        assert!(bounded_url("url", "not a url").is_err());
    }

    #[test]
    fn opaque_json_is_size_bounded() {
        assert!(bounded_opaque_json("input", &json!({"k": "v"})).is_ok());
        let big = json!({ "k": "x".repeat(MAX_OPAQUE_JSON_BYTES + 1) });
        assert!(bounded_opaque_json("input", &big).is_err());
    }
}
