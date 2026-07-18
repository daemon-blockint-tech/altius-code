use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Error type shared by every protocol surface in this crate.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Inbound (untrusted) content failed bounded validation.
    #[error("invalid {field}: {reason}")]
    Validation { field: &'static str, reason: String },

    /// A referenced resource does not exist.
    #[error("{resource} `{id}` not found")]
    NotFound { resource: &'static str, id: String },

    /// A run/task state transition that the protocol forbids.
    #[error("invalid transition from `{from}` to `{to}`")]
    InvalidTransition { from: String, to: String },

    /// A resource with the same identifier already exists.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Missing or invalid bearer credentials.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Cryptographic verification is not implemented yet; we fail closed
    /// rather than accepting unverified remote identities.
    #[error("verification unavailable (fail closed): {0}")]
    VerificationUnavailable(String),

    /// Anything unexpected on our side.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ProtocolError {
    pub fn validation(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Validation {
            field,
            reason: reason.into(),
        }
    }

    pub fn not_found(resource: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            resource,
            id: id.into(),
        }
    }

    /// Stable machine-readable code used in HTTP error bodies.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation { .. } => "invalid_input",
            Self::NotFound { .. } => "not_found",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized(_) => "unauthorized",
            Self::VerificationUnavailable(_) => "verification_unavailable",
            Self::Internal(_) => "internal_error",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Validation { .. } => StatusCode::BAD_REQUEST,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::InvalidTransition { .. } | Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            // Fail closed: an unverifiable identity is a refusal, not a bug.
            Self::VerificationUnavailable(_) => StatusCode::FORBIDDEN,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ProtocolError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        });
        (self.status(), Json(body)).into_response()
    }
}

/// Convenience alias used throughout this crate.
pub type Result<T> = std::result::Result<T, ProtocolError>;
