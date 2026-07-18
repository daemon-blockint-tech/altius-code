use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::description::AgentDescription;
use super::registry::AgentRegistry;
use crate::error::Result;
use crate::limits;

/// Shared state for the local ANP discovery stub.
#[derive(Clone)]
pub struct AnpState {
    pub registry: Arc<dyn AgentRegistry>,
}

impl AnpState {
    pub fn new(registry: Arc<dyn AgentRegistry>) -> Self {
        Self { registry }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct DiscoverQuery {
    /// Optional interface protocol filter (e.g. `a2a`, `beeacp`).
    pub protocol: Option<String>,
}

/// Build the local ANP discovery stub router.
///
/// These endpoints store and return *claims* only. Cryptographic DID
/// verification remains fail-closed via [`super::StubDidVerifier`].
pub fn router(state: AnpState) -> Router {
    Router::new()
        .route("/anp/agents", get(discover).post(register))
        .route("/anp/agents/{did}", get(find))
        .layer(DefaultBodyLimit::max(limits::MAX_BODY_BYTES))
        .with_state(state)
}

async fn register(
    State(state): State<AnpState>,
    Json(description): Json<AgentDescription>,
) -> Result<StatusCode> {
    state.registry.register(description).await?;
    Ok(StatusCode::CREATED)
}

async fn discover(
    State(state): State<AnpState>,
    Query(query): Query<DiscoverQuery>,
) -> Result<Json<Vec<AgentDescription>>> {
    if let Some(protocol) = &query.protocol {
        limits::bounded_string("protocol", protocol, limits::MAX_NAME_LEN)?;
    }
    let agents = state.registry.discover(query.protocol.as_deref()).await?;
    Ok(Json(agents))
}

async fn find(
    State(state): State<AnpState>,
    Path(did): Path<String>,
) -> Result<Json<AgentDescription>> {
    // Axum path segments decode `%3A` to `:`, so callers may pass
    // `did:wba:...` either raw (when routing permits) or percent-encoded.
    limits::bounded_string("did", &did, limits::MAX_DID_LEN)?;
    let description = state.registry.find(&did).await?;
    Ok(Json(description))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anp::{InMemoryRegistry, InterfaceDescription};
    use axum::body::Body;
    use axum::http::{header, Request};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn app() -> Router {
        router(AnpState::new(Arc::new(InMemoryRegistry::new())))
    }

    async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    fn description() -> Value {
        json!({
            "did": "did:wba:agents.example.com:agent:altius",
            "name": "altius",
            "description": "local discovery stub",
            "interfaces": [{
                "protocol": "a2a",
                "url": "https://agents.example.com/a2a"
            }]
        })
    }

    #[tokio::test]
    async fn register_and_discover() {
        let app = app();
        let (status, _) = send(
            &app,
            Request::post("/anp/agents")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(description().to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = send(
            &app,
            Request::get("/anp/agents?protocol=a2a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert_eq!(body[0]["name"], "altius");
    }

    #[tokio::test]
    async fn register_rejects_bad_did() {
        let mut bad = description();
        bad["did"] = json!("did:web:example.com");
        let (status, body) = send(
            &app(),
            Request::post("/anp/agents")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(bad.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    #[tokio::test]
    async fn find_missing_agent() {
        let (status, body) = send(
            &app(),
            Request::get("/anp/agents/did:wba:missing.example.com:agent:x")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    #[test]
    fn description_model_still_exports_interface() {
        let _ = InterfaceDescription {
            protocol: "a2a".into(),
            url: "https://example.com".into(),
            description: None,
        };
    }
}
