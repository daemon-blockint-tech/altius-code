use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use altius_agents::{
    agent_name_for_route, build_supervisor_graph_with, parse_slash_skill,
    run_supervisor_outcome_with_options, BrowserTooling, FleetState, LlmClient, McpTools,
    OfflineLlmClient, OpenAiCompatibleClient, SupervisorOptions, SupervisorOutcome,
};
use altius_core::{Budget, RunId};
use altius_graph::{
    Checkpointer, ExecutionOutcome, GraphExecutor, InMemoryCheckpointer, MemoryStoreCheckpointer,
    SqliteMemoryStore,
};
use altius_mcp::{McpAttachConfig, McpAttachments};
use altius_protocol::a2a::{
    A2aMessage, A2aState, AgentCapabilities, AgentCard, AgentSkill, Part, Task, TaskHandler,
    TaskState, TaskStatus,
};
use altius_protocol::anp::{
    AgentDescription, AgentRegistry, AnpState, InMemoryRegistry, InterfaceDescription,
};
use altius_protocol::beeacp::{
    require_bearer, BearerAuth, BeeAcpState, Message, MessagePart, Run, RunApproval, RunExecutor,
    RunOutcome, SqliteRunStore, openapi_router,
};
use altius_protocol::Result as ProtocolResult;
use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::{middleware, Router};
use tokio::sync::{RwLock, Semaphore};
use tower_http::services::{ServeDir, ServeFile};

use crate::cli::FleetServeArgs;
use crate::error::CliError;

/// Loopback binds may run without bearer auth for offline demos; every other
/// bind address requires `--token` / `ALTIUS_FLEET_TOKEN`.
fn validate_fleet_serve_auth(
    bind: SocketAddr,
    auth_token: &Option<String>,
) -> Result<(), CliError> {
    let token_set = auth_token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    if bind.ip().is_loopback() || token_set {
        return Ok(());
    }
    Err(CliError::message(format!(
        "refusing to serve on {bind} without bearer auth: set --token or ALTIUS_FLEET_TOKEN \
         (no-auth is allowed only on loopback addresses such as 127.0.0.1 for offline demos)"
    )))
}

#[derive(Clone)]
struct FleetHealthState {
    store: Option<Arc<dyn altius_protocol::beeacp::RunStore>>,
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "altius-fleet",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn ready_handler(
    State(state): State<FleetHealthState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Some(store) = &state.store {
        store
            .list()
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    }
    Ok(Json(serde_json::json!({ "status": "ready" })))
}

fn health_routes(store: Option<Arc<dyn altius_protocol::beeacp::RunStore>>) -> Router {
    let probes = Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .with_state(FleetHealthState { store });
    Router::new().merge(openapi_router()).merge(probes)
}

/// Bridges the BeeAI ACP run lifecycle onto the fleet supervisor.
///
/// A supervisor HITL interrupt maps to `RunOutcome::Awaiting`, pausing the
/// BeeAI run. When a run database path is configured, graph checkpoints and
/// the BeeAI-run → graph-run id map persist in SQLite via
/// [`MemoryStoreCheckpointer`] + [`SqliteMemoryStore`] (same file as
/// [`SqliteRunStore`]). `resume` re-enters the interrupted node from its
/// latest checkpoint (with the human reply appended to the state prompt).
/// Without a durable store, checkpoints and the id map are process-lifetime
/// only; resume falls back to a full re-run when no checkpoint is found.
struct FleetRunExecutor {
    offline: bool,
    options_template: SupervisorOptions,
    checkpointer: Arc<dyn Checkpointer<FleetState>>,
    /// Durable checkpoint + BeeAI→graph run mapping. `None` uses in-memory fallback.
    memory_store: Option<Arc<SqliteMemoryStore>>,
    /// Process-lifetime BeeAI → graph run map when `memory_store` is absent.
    graph_runs: Arc<RwLock<HashMap<RunId, RunId>>>,
    run_slots: Arc<Semaphore>,
}

struct FleetA2aHandler {
    offline: bool,
    options_template: SupervisorOptions,
    run_slots: Arc<Semaphore>,
}

impl FleetA2aHandler {
    fn new(offline: bool, options_template: SupervisorOptions) -> Self {
        Self {
            offline,
            options_template,
            run_slots: Arc::new(Semaphore::new(4)),
        }
    }
}

#[async_trait]
impl TaskHandler for FleetA2aHandler {
    async fn handle(&self, message: A2aMessage) -> ProtocolResult<Task> {
        let mut task = Task::submitted(message.clone());
        let prompt = message
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                Part::Data { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if prompt.trim().is_empty() {
            task.status = TaskStatus::now(TaskState::Rejected);
            task.status.message = Some(A2aMessage::agent_text(
                "A2A task requires at least one non-empty text part",
            ));
            return Ok(task);
        }

        let Ok(_permit) = Arc::clone(&self.run_slots).try_acquire_owned() else {
            task.status = TaskStatus::now(TaskState::Failed);
            task.status.message = Some(A2aMessage::agent_text(
                "fleet is at its concurrent-run limit; retry later",
            ));
            return Ok(task);
        };
        let llm = llm_client(self.offline)?;
        let checkpointer: Arc<dyn Checkpointer<FleetState>> = Arc::new(InMemoryCheckpointer::new());
        let outcome = run_supervisor_outcome_with_options(
            llm,
            checkpointer,
            fleet_budget(),
            prompt,
            self.options_template.clone(),
        )
        .await;
        match outcome {
            Ok((_run_id, SupervisorOutcome::Finished(state))) => {
                let response = A2aMessage::agent_text(
                    state
                        .final_answer
                        .unwrap_or_else(|| "(no final answer)".into()),
                );
                task.history.push(response.clone());
                task.status = TaskStatus::now(TaskState::Completed);
                task.status.message = Some(response);
            }
            Ok((_run_id, SupervisorOutcome::Awaiting { .. })) => {
                task.status = TaskStatus::now(TaskState::InputRequired);
                task.status.message = Some(A2aMessage::agent_text(
                    "fleet execution requires human approval; use the BeeAI run API to resume",
                ));
            }
            Err(error) => {
                task.status = TaskStatus::now(TaskState::Failed);
                task.status.message = Some(A2aMessage::agent_text(error.to_string()));
            }
        }
        Ok(task)
    }
}

impl FleetRunExecutor {
    /// Durable checkpoints and BeeAI→graph run mapping via SQLite (fleet serve default).
    fn with_durable_store(
        offline: bool,
        options_template: SupervisorOptions,
        memory_store: SqliteMemoryStore,
    ) -> Self {
        let memory_store = Arc::new(memory_store);
        let checkpointer: Arc<dyn Checkpointer<FleetState>> =
            Arc::new(MemoryStoreCheckpointer::new((*memory_store).clone()));
        Self {
            offline,
            options_template,
            checkpointer,
            memory_store: Some(memory_store),
            graph_runs: Arc::new(RwLock::new(HashMap::new())),
            run_slots: Arc::new(Semaphore::new(4)),
        }
    }

    async fn record_graph_run(&self, bee_run_id: RunId, graph_run_id: RunId) {
        if let Some(store) = &self.memory_store {
            let _ = store.put_bee_graph_run(bee_run_id, graph_run_id).await;
        } else {
            self.graph_runs
                .write()
                .await
                .insert(bee_run_id, graph_run_id);
        }
    }

    async fn graph_run_for_bee(&self, bee_run_id: RunId) -> Option<RunId> {
        if let Some(store) = &self.memory_store {
            store.get_bee_graph_run(bee_run_id).await.ok().flatten()
        } else {
            self.graph_runs.read().await.get(&bee_run_id).copied()
        }
    }

    /// One full supervisor pass, recording the graph run id so a later
    /// `awaiting` can be resumed from its checkpoint.
    async fn run_fleet(
        &self,
        bee_run_id: RunId,
        prompt: String,
        options: SupervisorOptions,
    ) -> ProtocolResult<RunOutcome> {
        let llm = llm_client(self.offline)?;
        let checkpointer: Arc<dyn Checkpointer<FleetState>> = Arc::clone(&self.checkpointer);
        match run_supervisor_outcome_with_options(
            llm,
            checkpointer,
            fleet_budget(),
            prompt,
            options,
        )
        .await
        {
            Ok((graph_run_id, outcome)) => {
                self.record_graph_run(bee_run_id, graph_run_id).await;
                Ok(supervisor_outcome_to_run_outcome(outcome))
            }
            Err(error) => Ok(RunOutcome::Failed(error.to_string())),
        }
    }

    /// Resume the paused graph from its latest checkpoint, if we have one.
    /// Returns `None` when no checkpoint is available (e.g. after restart).
    async fn try_resume_from_checkpoint(
        &self,
        run: &Run,
        message: Option<&Message>,
    ) -> ProtocolResult<Option<RunOutcome>> {
        let graph_run_id = self.graph_run_for_bee(run.run_id).await;
        let Some(graph_run_id) = graph_run_id else {
            return Ok(None);
        };
        let checkpointer: Arc<dyn Checkpointer<FleetState>> = Arc::clone(&self.checkpointer);
        let Ok(Some(checkpoint)) = checkpointer.latest(&graph_run_id).await else {
            return Ok(None);
        };

        let options = options_for_run(&self.options_template, &run.agent_name);
        let llm = llm_client(self.offline)?;
        let graph = match build_supervisor_graph_with(llm, &options) {
            Ok(graph) => Arc::new(graph),
            Err(error) => return Ok(Some(RunOutcome::Failed(error.to_string()))),
        };
        let executor = GraphExecutor::new(graph, checkpointer, fleet_budget());

        let mut state = checkpoint.state;
        if let Some(message) = message {
            let reply = flatten_messages(std::slice::from_ref(message));
            if !reply.is_empty() {
                state.prompt.push_str("\n\n[human response]\n");
                state.prompt.push_str(&reply);
            }
        }
        // Re-enter the node that interrupted, with the reply in state.
        match executor
            .resume(graph_run_id, checkpoint.node, state, 0)
            .await
        {
            Ok(ExecutionOutcome::Finished { state, .. }) => Ok(Some(completed_outcome(state))),
            Ok(ExecutionOutcome::Interrupted { reason, node, .. }) => Ok(Some(RunOutcome::Awaiting {
                approval: RunApproval::generic(reason, Some(node)),
            })),
            Err(error) => Ok(Some(RunOutcome::Failed(error.to_string()))),
        }
    }
}

#[async_trait]
impl RunExecutor for FleetRunExecutor {
    async fn execute(&self, run: &Run) -> ProtocolResult<RunOutcome> {
        let Ok(_permit) = Arc::clone(&self.run_slots).try_acquire_owned() else {
            return Ok(RunOutcome::Failed(
                "fleet is at its concurrent-run limit; retry later".into(),
            ));
        };
        let raw = flatten_messages(&run.input);
        let (agent_name, prompt) = if let Some(skill) = parse_slash_skill(&raw) {
            let body = if skill.remainder.is_empty() {
                raw
            } else {
                skill.remainder
            };
            (agent_name_for_route(skill.route).to_owned(), body)
        } else {
            (run.agent_name.clone(), raw)
        };
        self.run_fleet(
            run.run_id,
            prompt,
            options_for_run(&self.options_template, &agent_name),
        )
        .await
    }

    async fn resume(&self, run: &Run, message: Option<Message>) -> ProtocolResult<RunOutcome> {
        let Ok(_permit) = Arc::clone(&self.run_slots).try_acquire_owned() else {
            return Ok(RunOutcome::Failed(
                "fleet is at its concurrent-run limit; retry later".into(),
            ));
        };
        if let Some(outcome) = self
            .try_resume_from_checkpoint(run, message.as_ref())
            .await?
        {
            return Ok(outcome);
        }

        // Fallback (no checkpoint, e.g. after a restart): full re-run with
        // the resume message appended to the original prompt.
        let mut prompt = flatten_messages(&run.input);
        if let Some(message) = message.as_ref() {
            prompt.push('\n');
            prompt.push_str(&flatten_messages(std::slice::from_ref(message)));
        }
        self.run_fleet(
            run.run_id,
            prompt,
            options_for_run(&self.options_template, &run.agent_name),
        )
        .await
    }
}

/// Same shape as the supervisor's internal default budget.
fn fleet_budget() -> Budget {
    Budget::unlimited().with_max_steps(32).with_max_parallel(4)
}

fn completed_outcome(state: FleetState) -> RunOutcome {
    let answer = state
        .final_answer
        .unwrap_or_else(|| "(no final answer)".into());
    RunOutcome::Completed(vec![agent_text(answer)])
}

fn supervisor_outcome_to_run_outcome(outcome: SupervisorOutcome) -> RunOutcome {
    match outcome {
        SupervisorOutcome::Finished(state) => completed_outcome(state),
        // Graph `Interrupted` surfaces as awaiting, never as a failure.
        SupervisorOutcome::Awaiting { reason, node, .. } => RunOutcome::Awaiting {
            approval: RunApproval::generic(reason, Some(node)),
        },
    }
}

fn options_for_run(template: &SupervisorOptions, agent_name: &str) -> SupervisorOptions {
    SupervisorOptions {
        agent_name: Some(agent_name.to_owned()),
        browser: template.browser.clone(),
        hooks: template.hooks.clone(),
    }
}

fn agent_text(content: impl Into<String>) -> Message {
    Message {
        role: "agent".into(),
        parts: vec![MessagePart::text(content)],
    }
}

fn flatten_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .flat_map(|message| message.parts.iter().map(|part| part.content.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn llm_client(offline: bool) -> ProtocolResult<Arc<dyn LlmClient>> {
    if offline {
        return Ok(Arc::new(OfflineLlmClient));
    }
    if std::env::var("ALTIUS_LLM_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok() {
        return OpenAiCompatibleClient::from_env()
            .map(|client| Arc::new(client) as Arc<dyn LlmClient>)
            .map_err(|error| altius_protocol::ProtocolError::Internal(error.to_string()));
    }
    Ok(Arc::new(OfflineLlmClient))
}

fn agent_card(public_url: &str, browser_enabled: bool) -> Result<AgentCard, CliError> {
    let mut skills = vec![
        AgentSkill {
            id: "fleet-supervisor".into(),
            name: "Fleet supervisor".into(),
            description: "Route, explore, code-review, and finalize SVM engineering tasks".into(),
            tags: vec!["solana".into(), "svm".into()],
            examples: vec!["detect and lint this Anchor project".into()],
        },
        AgentSkill {
            id: "security".into(),
            name: "Security".into(),
            description:
                "Read-only vulnerability scanning via native Altius scanners (agent_name=security / @Security)"
                    .into(),
            tags: vec!["security".into(), "audit".into(), "svm".into()],
            examples: vec![
                "@Security audit this program for missing signer checks".into(),
                "agent_name=security lint the workspace".into(),
            ],
        },
    ];
    if browser_enabled {
        skills.push(AgentSkill {
            id: "browser".into(),
            name: "Browser".into(),
            description:
                "Web automation via an attached browser MCP server (agent_name=browser / @Browser)"
                    .into(),
            tags: vec!["browser".into(), "mcp".into()],
            examples: vec!["@Browser open https://example.com and summarize the title".into()],
        });
    }
    let card = AgentCard {
        protocol_version: "0.3.0".into(),
        name: "altius".into(),
        description: "Altius SVM multi-agent fleet".into(),
        url: public_url.to_owned(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: AgentCapabilities::default(),
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills,
    };
    card.validate()
        .map_err(|error| CliError::message(error.to_string()))?;
    Ok(card)
}

/// Resolve browser MCP launch config from CLI flags / environment.
///
/// Returns `None` when no command is configured (browser attach is opt-in).
fn browser_mcp_config(args: &FleetServeArgs) -> Result<Option<McpAttachConfig>, CliError> {
    let command = args
        .browser_mcp_cmd
        .clone()
        .or_else(|| std::env::var("ALTIUS_BROWSER_MCP_CMD").ok())
        .filter(|value| !value.trim().is_empty());
    let Some(command) = command else {
        return Ok(None);
    };

    let args_raw = args
        .browser_mcp_args
        .clone()
        .or_else(|| std::env::var("ALTIUS_BROWSER_MCP_ARGS").ok())
        .unwrap_or_else(|| "[]".into());
    let mcp_args: Vec<String> = serde_json::from_str(&args_raw).map_err(|error| {
        CliError::message(format!(
            "invalid --browser-mcp-args / ALTIUS_BROWSER_MCP_ARGS JSON array: {error}"
        ))
    })?;

    Ok(Some(McpAttachConfig {
        name: "browser".into(),
        command,
        args: mcp_args,
        working_directory: None,
        env_extras: vec![
            "DISPLAY".into(),
            "XAUTHORITY".into(),
            "WAYLAND_DISPLAY".into(),
        ],
    }))
}

async fn build_supervisor_options(
    args: &FleetServeArgs,
) -> Result<(SupervisorOptions, bool, Option<String>), CliError> {
    let attachments = Arc::new(McpAttachments::new());
    let mut browser_tooling = None;
    let mut plugin_name = None;

    // Plugin pack can supply browser MCP (and advertise skills). CLI
    // --browser-mcp-cmd still wins when both are set.
    let mut plugin_browser: Option<McpAttachConfig> = None;
    if let Some(path) = &args.plugin {
        let pack = crate::plugin::PluginPack::load(path)?;
        plugin_name = Some(pack.name.clone());
        if !pack.skills.is_empty() {
            eprintln!("altius: plugin skills: {}", pack.skills.join(", "));
        }
        for config in pack.mcp_configs() {
            if config.name == "browser" {
                plugin_browser = Some(config);
            } else {
                eprintln!(
                    "altius: plugin MCP `{}` noted (attach reserved for named specialists)",
                    config.name
                );
            }
        }
    }

    let browser_config = match browser_mcp_config(args)? {
        Some(config) => Some(config),
        None => plugin_browser,
    };

    if let Some(config) = browser_config {
        match attachments.attach(config).await {
            Ok(attached) => {
                let tools = McpTools::browser(Arc::clone(&attached));
                let specs = tools.tool_specs();
                eprintln!(
                    "altius: browser MCP attached ({} tool(s) allowlisted with prefix browser_)",
                    specs.len()
                );
                browser_tooling = Some(BrowserTooling {
                    tools: specs,
                    dispatcher: Arc::new(tools),
                });
            }
            Err(error) => {
                eprintln!("altius: warning: browser MCP attach failed: {error}");
            }
        }
    }
    let browser_enabled = browser_tooling.is_some();
    Ok((
        SupervisorOptions {
            agent_name: None,
            browser: browser_tooling,
            hooks: Vec::new(),
        },
        browser_enabled,
        plugin_name,
    ))
}

fn pwa_assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pwa")
}

/// `~/.altius/runs.db`, or `.altius/runs.db` under the cwd without a home.
fn default_run_db_path() -> PathBuf {
    match std::env::var_os("HOME").filter(|home| !home.is_empty()) {
        Some(home) => PathBuf::from(home).join(".altius").join("runs.db"),
        None => PathBuf::from(".altius").join("runs.db"),
    }
}

/// Serve BeeAI ACP + A2A surfaces on one HTTP listener.
pub fn run_serve_cmd(args: &FleetServeArgs) -> Result<(), CliError> {
    serve_protocols(args, true)
}

/// Serve only the A2A agent-card / task surface.
pub fn run_a2a_cmd(args: &FleetServeArgs) -> Result<(), CliError> {
    serve_protocols(args, false)
}

fn serve_protocols(args: &FleetServeArgs, include_beeacp: bool) -> Result<(), CliError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::message(format!("tokio runtime: {error}")))?;

    let bind: std::net::SocketAddr = args
        .bind
        .parse()
        .map_err(|error| CliError::message(format!("invalid --bind address: {error}")))?;
    let auth_token = args.token.clone().filter(|token| !token.trim().is_empty());
    validate_fleet_serve_auth(bind, &auth_token)?;
    let public_url = args.public_url.clone();
    let offline = args.offline;
    let local_did = format!("did:wba:{}%3A{}:agent:altius", bind.ip(), bind.port());
    let run_db_path = args.run_db.clone().unwrap_or_else(default_run_db_path);
    let serve_args = FleetServeArgs {
        bind: args.bind.clone(),
        public_url: args.public_url.clone(),
        offline: args.offline,
        browser_mcp_cmd: args.browser_mcp_cmd.clone(),
        browser_mcp_args: args.browser_mcp_args.clone(),
        token: args.token.clone(),
        run_db: args.run_db.clone(),
        plugin: args.plugin.clone(),
    };

    rt.block_on(async move {
        let (options_template, browser_enabled, plugin_name) =
            build_supervisor_options(&serve_args).await?;
        if let Some(name) = &plugin_name {
            eprintln!("altius: plugin pack loaded: {name}");
        }
        let card = agent_card(&public_url, browser_enabled)?;
        let a2a = altius_protocol::a2a::router(
            A2aState::new(
                card,
                Arc::new(FleetA2aHandler::new(offline, options_template.clone())),
            )
                .map_err(|error| CliError::message(error.to_string()))?,
        );
        let registry = Arc::new(InMemoryRegistry::new());
        let self_description = AgentDescription {
            did: local_did,
            name: "altius".into(),
            description: "Altius SVM multi-agent fleet".into(),
            interfaces: vec![
                InterfaceDescription {
                    protocol: "a2a".into(),
                    url: format!("{public_url}/message:send"),
                    description: Some("A2A task endpoint".into()),
                },
                InterfaceDescription {
                    protocol: "beeacp".into(),
                    url: format!("{public_url}/runs"),
                    description: Some("BeeAI ACP run API".into()),
                },
            ],
            version: Some(env!("CARGO_PKG_VERSION").into()),
        };
        // Local self-registration is best-effort; invalid public URLs must
        // not prevent serving the primary protocol surfaces.
        let _ = registry.register(self_description).await;
        let anp = altius_protocol::anp::router(AnpState::new(registry));

        let pwa_dir = pwa_assets_dir();
        let index = pwa_dir.join("index.html");
        let pwa = ServeDir::new(&pwa_dir).not_found_service(ServeFile::new(index));

        let (beeacp_store, protected) = if include_beeacp {
            let store = SqliteRunStore::open(&run_db_path).map_err(|error| {
                CliError::message(format!("open run db `{}`: {error}", run_db_path.display()))
            })?;
            let memory_store = SqliteMemoryStore::open(&run_db_path).map_err(|error| {
                CliError::message(format!(
                    "open checkpoint db `{}`: {error}",
                    run_db_path.display()
                ))
            })?;
            let store: Arc<dyn altius_protocol::beeacp::RunStore> = Arc::new(store);
            let bee = altius_protocol::beeacp::router(
                BeeAcpState::new(
                    Arc::clone(&store),
                    Arc::new(FleetRunExecutor::with_durable_store(
                        offline,
                        options_template,
                        memory_store,
                    )),
                )
                .with_auth_token(auth_token.clone()),
            );
            let protected = Router::new()
                .merge(bee)
                .merge(a2a)
                .merge(anp)
                .nest_service("/app", pwa);
            (Some(store), protected)
        } else {
            let protected = Router::new().merge(a2a).merge(anp).nest_service("/app", pwa);
            (None, protected)
        };

        // Protect every user-facing surface; leave /health and /ready open for probes.
        let protected = protected.layer(middleware::from_fn_with_state(
            BearerAuth::new(auth_token.clone()),
            require_bearer,
        ));
        let app = Router::new()
            .merge(health_routes(beeacp_store))
            .merge(protected);

        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|error| CliError::message(error.to_string()))?;
        eprintln!("altius: listening on http://{bind}");
        eprintln!("altius: health at http://{bind}/health (ready at /ready)");
        eprintln!("altius: OpenAPI 3.1 at http://{bind}/openapi.json");
        if include_beeacp {
            eprintln!("altius: BeeAI ACP runs at /runs (SSE at /runs/{{id}}/events)");
            eprintln!("altius: run db at {}", run_db_path.display());
            if auth_token.is_some() {
                eprintln!(
                    "altius: bearer auth ENABLED (Authorization: Bearer <token>; ?token= on /runs/{{id}}/events only)"
                );
            } else {
                eprintln!(
                    "altius: bearer auth disabled (loopback-only demo; set --token or ALTIUS_FLEET_TOKEN for remote bind)"
                );
            }
        }
        eprintln!("altius: A2A agent card at /.well-known/agent-card.json");
        eprintln!("altius: ANP discovery stub at /anp/agents");
        eprintln!("altius: PWA thin client at http://{bind}/app/");
        if browser_enabled {
            eprintln!("altius: browser dispatch enabled (agent_name=browser or @Browser)");
        } else {
            eprintln!(
                "altius: browser MCP not attached (set --browser-mcp-cmd or ALTIUS_BROWSER_MCP_CMD)"
            );
        }
        axum::serve(listener, app)
            .await
            .map_err(|error| CliError::message(error.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_bind_requires_authentication() {
        let bind: SocketAddr = "0.0.0.0:8788".parse().unwrap();
        assert!(validate_fleet_serve_auth(bind, &None).is_err());
        assert!(validate_fleet_serve_auth(bind, &Some("secret".into())).is_ok());
    }

    #[test]
    fn loopback_bind_allows_local_demo_without_authentication() {
        let bind: SocketAddr = "127.0.0.1:8788".parse().unwrap();
        assert!(validate_fleet_serve_auth(bind, &None).is_ok());
    }

    #[tokio::test]
    async fn a2a_handler_executes_offline_fleet() {
        let handler = FleetA2aHandler::new(true, SupervisorOptions::default());
        let task = handler
            .handle(A2aMessage {
                role: "user".into(),
                parts: vec![Part::Text {
                    text: "inspect this project".into(),
                }],
                message_id: None,
            })
            .await
            .unwrap();

        assert_eq!(task.status.state, TaskState::Completed);
        assert_eq!(task.history.len(), 2);
    }

    #[tokio::test]
    async fn a2a_handler_rejects_data_only_task() {
        let handler = FleetA2aHandler::new(true, SupervisorOptions::default());
        let task = handler
            .handle(A2aMessage {
                role: "user".into(),
                parts: vec![Part::Data {
                    data: serde_json::json!({"unsupported": true}),
                }],
                message_id: None,
            })
            .await
            .unwrap();

        assert_eq!(task.status.state, TaskState::Rejected);
    }

    #[tokio::test]
    async fn durable_graph_run_mapping_survives_store_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.db");
        let bee = RunId::new();
        let graph = RunId::new();
        {
            let store = SqliteMemoryStore::open(&path).unwrap();
            let exec =
                FleetRunExecutor::with_durable_store(true, SupervisorOptions::default(), store);
            exec.record_graph_run(bee, graph).await;
        }
        let store = SqliteMemoryStore::open(&path).unwrap();
        let exec = FleetRunExecutor::with_durable_store(true, SupervisorOptions::default(), store);
        assert_eq!(exec.graph_run_for_bee(bee).await, Some(graph));
    }

    #[tokio::test]
    async fn durable_checkpoint_survives_store_reopen_for_resume_lookup() {
        use altius_core::StepId;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.db");
        let bee = RunId::new();
        let graph = RunId::new();
        let step = StepId::new();
        let state = FleetState::new("approve this change");
        {
            let store = SqliteMemoryStore::open(&path).unwrap();
            let checkpointer = MemoryStoreCheckpointer::<FleetState, _>::new(store.clone());
            Checkpointer::put(&checkpointer, &graph, &step, "approve", &state)
                .await
                .unwrap();
            store.put_bee_graph_run(bee, graph).await.unwrap();
        }
        let store = SqliteMemoryStore::open(&path).unwrap();
        let exec = FleetRunExecutor::with_durable_store(true, SupervisorOptions::default(), store);
        let graph_run_id = exec.graph_run_for_bee(bee).await.unwrap();
        let latest = exec
            .checkpointer
            .latest(&graph_run_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.node, "approve");
        assert_eq!(latest.state.prompt, state.prompt);
    }
}
