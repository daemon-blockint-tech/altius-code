use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use altius_core::RunId;
use async_trait::async_trait;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use super::auth::{require_bearer, BearerAuth};
use super::model::{ApprovalDecision, Message, ProtocolErrorBody, Run, RunApproval, RunStatus};
use super::store::RunStore;
use crate::error::{ProtocolError, Result};
use crate::limits;

/// How often the SSE endpoint re-reads the store looking for changes.
const SSE_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// What an executor produced for one execution (or resumption) step.
#[derive(Debug)]
pub enum RunOutcome {
    /// The run finished with the given output messages.
    Completed(Vec<Message>),
    /// The run is paused waiting for external input (`POST /runs/{id}`).
    Awaiting { approval: RunApproval },
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
    /// Bearer token required on every route when set (see [`BearerAuth`]).
    /// `None` / empty keeps the surface open, e.g. for offline demos.
    pub auth_token: Option<String>,
}

impl BeeAcpState {
    pub fn new(store: Arc<dyn RunStore>, executor: Arc<dyn RunExecutor>) -> Self {
        Self {
            store,
            executor,
            auth_token: None,
        }
    }

    /// Require `Authorization: Bearer <token>` (or `?token=`) on all routes.
    pub fn with_auth_token(mut self, token: Option<String>) -> Self {
        self.auth_token = token;
        self
    }
}

/// Body for `POST /runs`.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
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
#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ResumeRunRequest {
    /// Optional message answering whatever the run is awaiting.
    #[serde(default)]
    pub message: Option<Message>,
    /// Typed approval decision (alternative to a free-form message).
    #[serde(default)]
    pub decision: Option<ApprovalDecision>,
}

/// Build the BeeAI ACP run-lifecycle router.
pub fn router(state: BeeAcpState) -> Router {
    let auth = BearerAuth::new(state.auth_token.clone());
    Router::new()
        .route("/runs", get(list_runs).post(create_run))
        .route("/runs/{id}", get(get_run).post(resume_run))
        .route("/runs/{id}/cancel", post(cancel_run))
        // Compatibility alias for early Altius clients. ACP's canonical
        // resume endpoint is POST /runs/{id}.
        .route("/runs/{id}/resume", post(resume_run))
        .route("/runs/{id}/events", get(run_events))
        .layer(DefaultBodyLimit::max(limits::MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(auth, require_bearer))
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
                .transition(run_id, RunStatus::Completed, Some(output), None, None)
                .await
        }
        RunOutcome::Awaiting { approval } => {
            state
                .store
                .transition(run_id, RunStatus::Awaiting, None, None, Some(approval))
                .await
        }
        RunOutcome::Failed(reason) => {
            state
                .store
                .transition(run_id, RunStatus::Failed, None, Some(reason), None)
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
                .transition(run_id, RunStatus::Failed, None, Some(err.to_string()), None)
                .await
        }
    };
    if let Err(err) = applied {
        tracing::warn!(run_id = %run_id, error = %err, "background run outcome not applied");
    }
}

#[utoipa::path(
    post,
    path = "/runs",
    tag = "runs",
    request_body = CreateRunRequest,
    responses(
        (status = 202, description = "Run accepted and started", body = Run),
        (status = 400, description = "Invalid input", body = ProtocolErrorBody),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create_run(
    State(state): State<BeeAcpState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<(StatusCode, Json<Run>)> {
    request.validate()?;
    let run = Run::new(request.agent_name, request.input);
    let run_id = run.run_id;
    state.store.create(run).await?;

    let started = state
        .store
        .transition(run_id, RunStatus::InProgress, None, None, None)
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

#[utoipa::path(
    get,
    path = "/runs",
    tag = "runs",
    responses(
        (status = 200, description = "Known runs (newest first)", body = [Run]),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list_runs(State(state): State<BeeAcpState>) -> Result<Json<Vec<Run>>> {
    Ok(Json(state.store.list().await?))
}

#[utoipa::path(
    get,
    path = "/runs/{id}",
    tag = "runs",
    params(("id" = String, Path, description = "Run UUID")),
    responses(
        (status = 200, description = "Run snapshot (includes `approval` when awaiting)", body = Run),
        (status = 400, description = "Invalid run id", body = ProtocolErrorBody),
        (status = 404, description = "Run not found", body = ProtocolErrorBody),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_run(
    State(state): State<BeeAcpState>,
    Path(id): Path<String>,
) -> Result<Json<Run>> {
    let run = state.store.get(parse_run_id(&id)?).await?;
    Ok(Json(run))
}

#[utoipa::path(
    post,
    path = "/runs/{id}/cancel",
    tag = "runs",
    params(("id" = String, Path, description = "Run UUID")),
    responses(
        (status = 200, description = "Run cancelled", body = Run),
        (status = 409, description = "Invalid transition", body = ProtocolErrorBody),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn cancel_run(
    State(state): State<BeeAcpState>,
    Path(id): Path<String>,
) -> Result<Json<Run>> {
    let run = state
        .store
        .transition(parse_run_id(&id)?, RunStatus::Cancelled, None, None, None)
        .await?;
    Ok(Json(run))
}

#[utoipa::path(
    post,
    path = "/runs/{id}",
    tag = "runs",
    params(("id" = String, Path, description = "Run UUID")),
    request_body = ResumeRunRequest,
    responses(
        (status = 202, description = "Run resumed", body = Run),
        (status = 409, description = "Invalid transition", body = ProtocolErrorBody),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn resume_run(
    State(state): State<BeeAcpState>,
    Path(id): Path<String>,
    Json(request): Json<ResumeRunRequest>,
) -> Result<(StatusCode, Json<Run>)> {
    let mut message = request.message;
    if message.is_none() {
        if let Some(decision) = request.decision {
            if !decision.approved {
                let run = state
                    .store
                    .transition(parse_run_id(&id)?, RunStatus::Cancelled, None, None, None)
                    .await?;
                return Ok((StatusCode::OK, Json(run)));
            }
            if let Some(note) = decision.note.filter(|note| !note.is_empty()) {
                message = Some(Message::user_text(note));
            }
        }
    }
    if let Some(message) = &message {
        message.validate()?;
    }
    let run_id = parse_run_id(&id)?;

    // Resuming re-enters `in-progress`; the store rejects this unless the
    // run is currently `awaiting`.
    let resumed = state
        .store
        .transition(run_id, RunStatus::InProgress, None, None, None)
        .await?;

    let task_state = state.clone();
    let task_run = resumed.clone();
    tokio::spawn(async move {
        let outcome = task_state.executor.resume(&task_run, message).await;
        settle_run(&task_state, run_id, outcome).await;
    });
    Ok((StatusCode::ACCEPTED, Json(resumed)))
}

/// `GET /runs/{id}/events` — server-sent run updates.
///
/// Polls the store every [`SSE_POLL_INTERVAL`] and emits an `event: run`
/// frame whenever the run's serialized form changes (status, output, error).
/// The first frame is sent immediately; the stream closes after the frame
/// that carries a terminal status. Axum's keep-alive sends comment pings so
/// idle proxies do not drop the connection while a run is in flight.
#[utoipa::path(
    get,
    path = "/runs/{id}/events",
    tag = "runs",
    params(("id" = String, Path, description = "Run UUID")),
    responses(
        (status = 200, description = "Server-sent run snapshots (`event: run`, JSON Run body; includes `approval` when awaiting)"),
        (status = 404, description = "Run not found", body = ProtocolErrorBody),
        (status = 401, description = "Unauthorized", body = ProtocolErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn run_events(
    State(state): State<BeeAcpState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let run_id = parse_run_id(&id)?;
    // Fail fast with a proper 404 before committing to a stream response.
    state.store.get(run_id).await?;

    let stream = futures::stream::unfold(
        RunEventStream {
            store: Arc::clone(&state.store),
            run_id,
            last_payload: None,
            finished: false,
        },
        |mut ctx| async move {
            if ctx.finished {
                return None;
            }
            loop {
                let run = match ctx.store.get(ctx.run_id).await {
                    Ok(run) => run,
                    // Run vanished (or store failed): end the stream.
                    Err(_) => return None,
                };
                let terminal = run.status.is_terminal();
                let payload = match serde_json::to_string(&run) {
                    Ok(payload) => payload,
                    Err(_) => return None,
                };
                if ctx.last_payload.as_deref() != Some(payload.as_str()) {
                    ctx.last_payload = Some(payload.clone());
                    ctx.finished = terminal;
                    let event = Event::default().event("run").data(payload);
                    return Some((Ok(event), ctx));
                }
                if terminal {
                    return None;
                }
                tokio::time::sleep(SSE_POLL_INTERVAL).await;
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(5))
            .text("ping"),
    ))
}

struct RunEventStream {
    store: Arc<dyn RunStore>,
    run_id: RunId,
    last_payload: Option<String>,
    finished: bool,
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
            Ok(RunOutcome::Awaiting {
                approval: RunApproval::generic("approval required", Some("test".into())),
            })
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

    async fn send_status(app: &Router, request: Request<Body>) -> StatusCode {
        app.clone().oneshot(request).await.unwrap().status()
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
        let awaiting = wait_for_status(&app, id, "awaiting").await;
        assert_eq!(awaiting["approval"]["summary"], "approval required");
        assert_eq!(awaiting["approval"]["kind"], "generic");

        let resume = json!({
            "message": { "role": "user", "parts": [{ "content_type": "text/plain", "content": "answer" }] },
        });
        let (status, resumed) = send(&app, post_json(&format!("/runs/{id}"), resume)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(resumed["status"], "in-progress");

        let done = wait_for_status(&app, id, "completed").await;
        assert_eq!(done["output"][0]["parts"][0]["content"], "answer");
        assert!(done.get("approval").is_none() || done["approval"].is_null());
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
    async fn auth_token_gates_all_routes() {
        let app = router(
            BeeAcpState::new(Arc::new(InMemoryRunStore::new()), Arc::new(NoopExecutor))
                .with_auth_token(Some("s3cret".into())),
        );

        // Unauthenticated POST is rejected with the protocol error shape.
        let (status, body) = send(&app, post_json("/runs", create_body())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");

        // Wrong bearer is rejected too.
        let request = Request::post("/runs")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer nope")
            .body(Body::from(create_body().to_string()))
            .unwrap();
        let (status, _) = send(&app, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Correct bearer succeeds.
        let request = Request::post("/runs")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer s3cret")
            .body(Body::from(create_body().to_string()))
            .unwrap();
        let (status, created) = send(&app, request).await;
        assert_eq!(status, StatusCode::ACCEPTED);

        // Query-string token works for header-less SSE clients (EventSource).
        let id = created["run_id"].as_str().unwrap();
        let status = send_status(
            &app,
            Request::get(format!("/runs/{id}/events?token=s3cret"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn empty_auth_token_disables_auth() {
        let app = router(
            BeeAcpState::new(Arc::new(InMemoryRunStore::new()), Arc::new(NoopExecutor))
                .with_auth_token(Some(String::new())),
        );
        let (status, _) = send(&app, post_json("/runs", create_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn events_stream_includes_approval_when_awaiting() {
        let app = app(Arc::new(AwaitingExecutor));
        let (_, created) = send(&app, post_json("/runs", create_body())).await;
        let id = created["run_id"].as_str().unwrap().to_owned();
        wait_for_status(&app, &id, "awaiting").await;

        // Approval lives on the run snapshot (SSE `event: run` frames carry the
        // same JSON). Do not drain the SSE body here — awaiting is non-terminal,
        // so keep-alive pings keep the stream open indefinitely.
        let (_, run) = send(
            &app,
            Request::get(format!("/runs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(run["status"], "awaiting");
        assert_eq!(run["approval"]["summary"], "approval required");
        assert_eq!(run["approval"]["kind"], "generic");

        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/runs/{id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
    }

    #[tokio::test]
    async fn openapi_json_is_available() {
        let app = super::super::openapi::openapi_router().merge(app(Arc::new(NoopExecutor)));
        let (status, body) = send(
            &app,
            Request::get("/openapi.json").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["openapi"], "3.1.0");
        assert!(body["paths"]["/runs"].is_object());
    }

    #[tokio::test]
    async fn events_stream_emits_run_and_closes_on_terminal() {
        let app = app(Arc::new(NoopExecutor));
        let (_, created) = send(&app, post_json("/runs", create_body())).await;
        let id = created["run_id"].as_str().unwrap().to_owned();
        wait_for_status(&app, &id, "completed").await;

        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/runs/{id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        // The run is terminal, so the stream emits one frame and closes;
        // reading the body to completion must therefore terminate.
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("event: run"), "missing run event: {text}");
        assert!(text.contains("\"completed\""), "missing status: {text}");
    }

    #[tokio::test]
    async fn events_for_unknown_run_is_404() {
        let app = app(Arc::new(NoopExecutor));
        let missing = RunId::new();
        let (status, body) = send(
            &app,
            Request::get(format!("/runs/{missing}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
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
