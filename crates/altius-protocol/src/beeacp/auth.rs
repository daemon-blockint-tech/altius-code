//! Bearer-token auth for the BeeAI ACP surface.
//!
//! When no token is configured the middleware is a no-op, so offline demos
//! keep working with zero setup. When a token is set, every request must
//! carry `Authorization: Bearer <token>` — or `?token=<token>` in the query
//! string, because `EventSource` cannot set request headers.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ProtocolError;

/// Shared middleware state: the expected bearer token, if any.
///
/// `None` or an empty string disables authentication entirely.
#[derive(Clone, Debug, Default)]
pub struct BearerAuth {
    expected: Option<String>,
}

impl BearerAuth {
    pub fn new(expected: Option<String>) -> Self {
        Self {
            expected: expected.filter(|token| !token.trim().is_empty()),
        }
    }

    /// Whether requests will actually be challenged.
    pub fn is_enabled(&self) -> bool {
        self.expected.is_some()
    }

    fn authorizes(&self, request: &Request) -> bool {
        let Some(expected) = self.expected.as_deref() else {
            return true;
        };
        if let Some(value) = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        {
            if let Some(token) = value.strip_prefix("Bearer ") {
                if constant_time_eq(token.trim(), expected) {
                    return true;
                }
            }
        }
        // Query-string fallback is restricted to the SSE endpoint because
        // EventSource cannot set headers. Accepting credentials in URLs on
        // ordinary endpoints needlessly exposes them to logs and history.
        if request.uri().path().ends_with("/events") {
            if let Some(query) = request.uri().query() {
                for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
                    if key == "token" && constant_time_eq(&value, expected) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Compare tokens without early exit on the first mismatched byte.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Axum middleware fn; wire with `middleware::from_fn_with_state`.
pub async fn require_bearer(
    State(auth): State<BearerAuth>,
    request: Request,
    next: Next,
) -> Response {
    if auth.authorizes(&request) {
        return next.run(request).await;
    }
    ProtocolError::Unauthorized(
        "missing or invalid bearer token (use `Authorization: Bearer <token>`; \
         `?token=` is accepted only on `/runs/{id}/events` for SSE)"
            .to_owned(),
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str, auth_header: Option<&str>) -> Request {
        let mut builder = Request::builder().uri(uri);
        if let Some(value) = auth_header {
            builder = builder.header(header::AUTHORIZATION, value);
        }
        builder.body(axum::body::Body::empty()).unwrap()
    }

    #[test]
    fn disabled_auth_allows_everything() {
        for auth in [BearerAuth::new(None), BearerAuth::new(Some("  ".into()))] {
            assert!(!auth.is_enabled());
            assert!(auth.authorizes(&request("/runs", None)));
        }
    }

    #[test]
    fn enabled_auth_checks_header_and_query() {
        let auth = BearerAuth::new(Some("s3cret".into()));
        assert!(auth.is_enabled());
        assert!(!auth.authorizes(&request("/runs", None)));
        assert!(!auth.authorizes(&request("/runs", Some("Bearer wrong"))));
        assert!(auth.authorizes(&request("/runs", Some("Bearer s3cret"))));
        assert!(!auth.authorizes(&request("/runs?token=s3cret", None)));
        assert!(auth.authorizes(&request("/runs/id/events?token=s3cret", None)));
        assert!(!auth.authorizes(&request("/runs/id/events?token=wrong", None)));
    }
}
