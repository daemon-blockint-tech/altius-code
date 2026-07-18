use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::model::{A2aMessage, AgentCard, Task, TaskState, TaskStatus};
use crate::error::Result;
use crate::limits;

/// Well-known path where the agent card is served.
pub const AGENT_CARD_PATH: &str = "/.well-known/agent-card.json";

/// Injectable handler that turns an inbound A2A message into a task.
///
/// Implementations decide what the task does (typically: enqueue work on
/// the fleet runtime). The message it receives has already passed bounded
/// validation, but its content is still untrusted remote data.
#[async_trait]
pub trait TaskHandler: Send + Sync {
    async fn handle(&self, message: A2aMessage) -> Result<Task>;
}

/// Handler that acknowledges the message and immediately completes the
/// task with an echo reply. Placeholder until the fleet runtime attaches.
#[derive(Clone, Copy, Debug, Default)]
pub struct EchoTaskHandler;

#[async_trait]
impl TaskHandler for EchoTaskHandler {
    async fn handle(&self, message: A2aMessage) -> Result<Task> {
        let mut task = Task::submitted(message);
        task.status = TaskStatus::now(TaskState::Completed);
        task.status.message = Some(A2aMessage::agent_text("acknowledged"));
        Ok(task)
    }
}

/// Shared state for the A2A router.
#[derive(Clone)]
pub struct A2aState {
    pub card: Arc<AgentCard>,
    pub handler: Arc<dyn TaskHandler>,
}

impl A2aState {
    /// Validates the card once up front so we never serve a malformed one.
    pub fn new(card: AgentCard, handler: Arc<dyn TaskHandler>) -> Result<Self> {
        card.validate()?;
        Ok(Self {
            card: Arc::new(card),
            handler,
        })
    }
}

/// Body for `POST /message:send`: the inbound message plus pass-through
/// metadata we do not interpret.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub message: A2aMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Build the A2A router: the well-known agent card plus the task route.
pub fn router(state: A2aState) -> Router {
    Router::new()
        .route(AGENT_CARD_PATH, get(agent_card))
        .route("/message:send", post(send_message))
        // Compatibility alias for the pre-1.0 slash spelling.
        .route("/message/send", post(send_message))
        .layer(DefaultBodyLimit::max(limits::MAX_BODY_BYTES))
        .with_state(state)
}

async fn agent_card(State(state): State<A2aState>) -> Json<AgentCard> {
    Json((*state.card).clone())
}

async fn send_message(
    State(state): State<A2aState>,
    Json(request): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<Task>)> {
    request.message.validate()?;
    if let Some(metadata) = &request.metadata {
        limits::bounded_opaque_json("metadata", metadata)?;
    }
    let task = state.handler.handle(request.message).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::{AgentCapabilities, AgentSkill, Part};
    use axum::body::Body;
    use axum::http::{header, Request};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn card() -> AgentCard {
        AgentCard {
            protocol_version: "0.3.0".into(),
            name: "altius".into(),
            description: "Altius SVM fleet agent".into(),
            url: "https://agents.example.com/a2a".into(),
            version: "0.1.0".into(),
            capabilities: AgentCapabilities::default(),
            default_input_modes: vec!["text/plain".into()],
            default_output_modes: vec!["text/plain".into()],
            skills: vec![AgentSkill {
                id: "svm-detect".into(),
                name: "SVM project detection".into(),
                description: "Detect Solana project frameworks".into(),
                tags: vec![],
                examples: vec![],
            }],
        }
    }

    fn app() -> Router {
        router(A2aState::new(card(), Arc::new(EchoTaskHandler)).unwrap())
    }

    async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn serves_agent_card_at_well_known_path() {
        let (status, body) = send(
            &app(),
            Request::get(AGENT_CARD_PATH).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "altius");
        assert_eq!(body["skills"][0]["id"], "svm-detect");
        // Round-trips through our interoperable model.
        let parsed: AgentCard = serde_json::from_value(body).unwrap();
        assert_eq!(parsed, card());
    }

    #[tokio::test]
    async fn state_construction_rejects_invalid_card() {
        let mut bad = card();
        bad.name = String::new();
        assert!(A2aState::new(bad, Arc::new(EchoTaskHandler)).is_err());
    }

    #[tokio::test]
    async fn send_message_delegates_to_handler() {
        let body = json!({
            "message": {
                "role": "user",
                "parts": [
                    { "kind": "text", "text": "detect this project" },
                    { "kind": "data", "data": { "opaque": { "anything": true } } },
                ],
            },
            "metadata": { "traceId": "abc" },
        });
        let request = Request::post("/message:send")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, task) = send(&app(), request).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(task["status"]["state"], "completed");
        // Opaque payload survives untouched in task history.
        assert_eq!(task["history"][0]["parts"][1]["data"]["opaque"]["anything"], true);
    }

    #[tokio::test]
    async fn send_message_rejects_oversized_opaque_payload() {
        let message = A2aMessage {
            role: "user".into(),
            parts: vec![Part::Data {
                data: json!({ "blob": "x".repeat(limits::MAX_OPAQUE_JSON_BYTES + 1) }),
            }],
            message_id: None,
        };
        let body = json!({ "message": message });
        let request = Request::post("/message:send")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        // Payload exceeds the body cap as well, so either 400 (validation)
        // or 413 (body limit) is acceptable fail-closed behavior.
        let response = app().oneshot(request).await.unwrap();
        assert!(response.status().is_client_error());
    }
}
