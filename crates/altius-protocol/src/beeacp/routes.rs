use std::sync::Arc;

use altius_core::RunId;
use async_trait::async_trait;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::model::{Message, Run, RunStatus};
use super::store::RunStore;
use crate::error::{ProtocolError, Result};
use crate::limits;

/// What an executor produced for one execution (or resumption) step.
#[derive(Debug)]
pub enum RunOutcome {
    /// The run finished with the given output messages.
    Completed(Vec<Message>),
    /// The run is paused waiting for external input (`POST /runs/{id}`).
    Awaiting,
    /// The run failed with a human-readable reason.
    Failed(String),
}

/// Injectable execution behind the run lifecycle.
///
/// The HTTP layer owns all state transitions; implementations only compute
/// an outcome. No implementation in this crate signs or submits anything.
#[async_trait]
pub trait RunExecutor: Send + Sync {
    /// Execute a freshly started run.
    async fn execute(&self, run: &Run) -> Result<RunOutcome>;

    /// Resume a run that is `awaiting`, with an optional caller message.
    async fn resume(&self, run: &Run, message: Option<Message>) -> Result<RunOutcome>;
}

/// Trivial executor that echoes the run input back as output. Useful for
/// wiring tests and as a placeholder until the fleet runtime is attached.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopExecutor;

#[async_trait]
impl RunExecutor for NoopExecutor {
    async fn execute(&self, run: &Run) -> Result<RunOutcome> {
        Ok(RunOutcome::Completed(run.input.clone()))
    }

    async fn resume(&self, run: &Run, message: Option<Message>) -> Result<RunOutcome> {
        let mut output = run.input.clone();
        output.extend(message);
        Ok(RunOutcome::Completed(output))
    }
}

/// Shared state for the BeeAI ACP router.
#[derive(Clone)]
pub struct BeeAcpState {
    pub store: Arc<dyn RunStore>,
    pub executor: Arc<dyn RunExecutor>,
}

impl BeeAcpState {
    pub fn new(store: Arc<dyn RunStore>, executor: Arc<dyn RunExecutor>) -> Self {
        Self { store, executor }
    }
}

/// Body for `POST /runs`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRunRequest {
    /// Name of the agent to run.
    pub agent_name: String,
    /// Input messages for the run.
    pub input: Vec<Message>,
}

impl CreateRunRequest {
    fn validate(&self) -> Result<()> {
        limits::bounded_string("agent_name", &self.agent_name, limits::MAX_NAME_LEN)?;
        limits::bounded_list("input", self.input.len(), limits::MAX_LIST_LEN)?;
        for message in &self.input {
            message.validate()?;
        }
        Ok(())
    }
}

/// Body for `POST /runs/{id}`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResumeRunRequest {
    /// Optional message answering whatever the run is awaiting.
    #[serde(default)]
    pub message: Option<Message>,
}

/// Build the BeeAI ACP run-lifecycle router.
pub fn router(state: BeeAcpState) -> Router {
    Router::new()
        .route("/runs", get(list_runs).post(create_run))
        .route("/runs/{id}", get(get_run).post(resume_run))
        .route("/runs/{id}/cancel", post(cancel_run))
        // Compatibility alias for early Altius clients. ACP's canonical
        // resume endpoint is POST /runs/{id}.
        .route("/runs/{id}/resume", post(resume_run))
        .layer(DefaultBodyLimit::max(limits::MAX_BODY_BYTES))
        .with_state(state)
}

fn parse_run_id(raw: &str) -> Result<RunId> {
    raw.parse()
        .map_err(|_| ProtocolError::validation("run_id", "not a valid UUID"))
}

/// Apply an executor outcome as a store transition from `in-progress`.
async fn apply_outcome(state: &BeeAcpState, run_id: RunId, outcome: RunOutcome) -> Result<Run> {
    match outcome {
        RunOutcome::Completed(output) => {
            state
                .store
                .transition(run_id, RunStatus::Completed, Some(output), None)
                .await
        }
        RunOutcome::Awaiting => {
            state
                .store
                .transition(run_id, RunStatus::Awaiting, None, None)
                .await
        }
        RunOutcome::Failed(reason) => {
            state
                .store
                .transition(run_id, RunStatus::Failed, None, Some(reason))
                .await
        }
    }
}

/// Settle a background execution result against the store.
///
/// Transition failures are logged, not propagated: a cancel racing the
/// spawned task makes the strict transition table reject the late outcome,
/// which is expected — the cancel wins.
async fn settle_run(state: &BeeAcpState, run_id: RunId, outcome: Result<RunOutcome>) {
    let applied = match outcome {
        Ok(outcome) => apply_outcome(state, run_id, outcome).await,
        Err(err) => {
            state
                .store
                .transition(run_id, RunStatus::Failed, None, Some(err.to_string()))
                .await
        }
    };
    if let Err(err) = applied {
        tracing::warn!(run_id = %run_id, error = %err, "background run outcome not applied");
    }
}

async fn create_run(
    State(state): State<BeeAcpState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<(StatusCode, Json<Run>)> {
    request.validate()?;
    let run = Run::new(request.agent_name, request.input);
    let run_id = run.run_id;
    state.store.create(run).await?;

    let started = state
        .store
        .transition(run_id, RunStatus::InProgress, None, None)
        .await?;

    // Execute in the background; the caller polls `GET /runs/{id}`.
    let task_state = state.clone();
    let task_run = started.clone();
    tokio::spawn(async move {
        let outcome = task_state.executor.execute(&task_run).await;
        settle_run(&task_state, run_id, outcome).await;
    });
    Ok((StatusCode::ACCEPTED, Json(started)))
}

async fn list_runs(State(state): State<BeeAcpState>) -> Result<Json<Vec<Run>>> {
    Ok(Json(state.store.list().await?))
}

async fn get_run(State(state): State<BeeAcpState>, Path(id): Path<String>) -> Result<Json<Run>> {
    let run = state.store.get(parse_run_id(&id)?).await?;
    Ok(Json(run))
}

async fn cancel_run(State(state): State<BeeAcpState>, Path(id): Path<String>) -> Result<Json<Run>> {
    let run = state
        .store
        .transition(parse_run_id(&id)?, RunStatus::Cancelled, None, None)
        .await?;
    Ok(Json(run))
}

async fn resume_run(
    State(state): State<BeeAcpState>,
    Path(id): Path<String>,
    Json(request): Json<ResumeRunRequest>,
) -> Result<(StatusCode, Json<Run>)> {
    if let Some(message) = &request.message {
        message.validate()?;
    }
    let run_id = parse_run_id(&id)?;

    // Resuming re-enters `in-progress`; the store rejects this unless the
    // run is currently `awaiting`.
    let resumed = state
        .store
        .transition(run_id, RunStatus::InProgress, None, None)
        .await?;

    let task_state = state.clone();
    let task_run = resumed.clone();
    let message = request.message;
    tokio::spawn(async move {
        let outcome = task_state.executor.resume(&task_run, message).await;
        settle_run(&task_state, run_id, outcome).await;
    });
    Ok((StatusCode::ACCEPTED, Json(resumed)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beeacp::InMemoryRunStore;
    use axum::body::Body;
    use axum::http::{header, Request};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    /// Executor that awaits on first execution and completes on resume.
    struct AwaitingExecutor;

    #[async_trait]
    impl RunExecutor for AwaitingExecutor {
        async fn execute(&self, _run: &Run) -> Result<RunOutcome> {
            Ok(RunOutcome::Awaiting)
        }

        async fn resume(&self, _run: &Run, message: Option<Message>) -> Result<RunOutcome> {
            Ok(RunOutcome::Completed(message.into_iter().collect()))
        }
    }

    fn app(executor: Arc<dyn RunExecutor>) -> Router {
        router(BeeAcpState::new(
            Arc::new(InMemoryRunStore::new()),
            executor,
        ))
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

    fn post_json(uri: &str, body: Value) -> Request<Body> {
        Request::post(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn create_body() -> Value {
        json!({
            "agent_name": "altius",
            "input": [{ "role": "user", "parts": [{ "content_type": "text/plain", "content": "hi" }] }],
        })
    }

    /// Poll `GET /runs/{id}` until the run reaches `expected`.
    async fn wait_for_status(app: &Router, id: &str, expected: &str) -> Value {
        for _ in 0..200 {
            let (status, body) = send(
                app,
                Request::get(format!("/runs/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            if body["status"] == expected {
                return body;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("run {id} never reached status `{expected}`");
    }

    #[tokio::test]
    async fn create_run_is_accepted_and_completes_via_noop_executor() {
        let app = app(Arc::new(NoopExecutor));
        let (status, body) = send(&app, post_json("/runs", create_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body["status"], "in-progress");

        let id = body["run_id"].as_str().unwrap();
        let done = wait_for_status(&app, id, "completed").await;
        assert_eq!(done["output"][0]["parts"][0]["content"], "hi");
    }

    #[tokio::test]
    async fn list_runs_returns_created_runs() {
        let app = app(Arc::new(NoopExecutor));
        let (_status, created) = send(&app, post_json("/runs", create_body())).await;
        let id = created["run_id"].as_str().unwrap().to_owned();
        wait_for_status(&app, &id, "completed").await;

        let (status, body) = send(&app, Request::get("/runs").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let runs = body.as_array().expect("list should be an array");
        assert!(runs.iter().any(|run| run["run_id"] == id));
    }

    #[tokio::test]
    async fn get_unknown_run_is_404_and_bad_id_is_400() {
        let app = app(Arc::new(NoopExecutor));
        let missing = RunId::new();
        let (status, body) = send(
            &app,
            Request::get(format!("/runs/{missing}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");

        let (status, body) = send(
            &app,
            Request::get("/runs/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    #[tokio::test]
    async fn awaiting_run_resumes_to_completion() {
        let app = app(Arc::new(AwaitingExecutor));
        let (status, created) = send(&app, post_json("/runs", create_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(created["status"], "in-progress");
        let id = created["run_id"].as_str().unwrap();
        wait_for_status(&app, id, "awaiting").await;

        let resume = json!({
            "message": { "role": "user", "parts": [{ "content_type": "text/plain", "content": "answer" }] },
        });
        let (status, resumed) = send(&app, post_json(&format!("/runs/{id}"), resume)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(resumed["status"], "in-progress");

        let done = wait_for_status(&app, id, "completed").await;
        assert_eq!(done["output"][0]["parts"][0]["content"], "answer");
    }

    #[tokio::test]
    async fn resume_of_completed_run_is_rejected() {
        let app = app(Arc::new(NoopExecutor));
        let (_, created) = send(&app, post_json("/runs", create_body())).await;
        let id = created["run_id"].as_str().unwrap();
        wait_for_status(&app, id, "completed").await;

        let (status, body) = send(&app, post_json(&format!("/runs/{id}"), json!({}))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "invalid_transition");
    }

    #[tokio::test]
    async fn cancel_follows_transition_rules() {
        let app = app(Arc::new(AwaitingExecutor));
        let (_, created) = send(&app, post_json("/runs", create_body())).await;
        let id = created["run_id"].as_str().unwrap();
        wait_for_status(&app, id, "awaiting").await;

        let (status, cancelled) =
            send(&app, post_json(&format!("/runs/{id}/cancel"), json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cancelled["status"], "cancelled");

        // Cancelling twice violates the transition table.
        let (status, body) = send(&app, post_json(&format!("/runs/{id}/cancel"), json!({}))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "invalid_transition");
    }

    #[tokio::test]
    async fn oversized_input_is_rejected() {
        let app = app(Arc::new(NoopExecutor));
        let body = json!({
            "agent_name": "altius",
            "input": [{
                "role": "user",
                "parts": [{
                    "content_type": "text/plain",
                    "content": "x".repeat(limits::MAX_TEXT_LEN + 1),
                }],
            }],
        });
        let (status, body) = send(&app, post_json("/runs", body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
    }
}
