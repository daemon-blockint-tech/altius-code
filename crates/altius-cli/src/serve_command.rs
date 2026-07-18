use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use altius_agents::{
    agent_name_for_route, build_supervisor_graph_with, parse_slash_skill,
    run_supervisor_outcome_with_options, BrowserTooling, FleetState, LlmClient, McpTools,
    OfflineLlmClient, OpenAiCompatibleClient, SupervisorOptions, SupervisorOutcome,
};
use altius_core::{Budget, RunId};
use altius_graph::{Checkpointer, ExecutionOutcome, GraphExecutor, InMemoryCheckpointer};
use altius_mcp::{McpAttachConfig, McpAttachments};
use altius_protocol::a2a::{A2aState, AgentCapabilities, AgentCard, AgentSkill, EchoTaskHandler};
use altius_protocol::anp::{
    AgentDescription, AgentRegistry, AnpState, InMemoryRegistry, InterfaceDescription,
};
use altius_protocol::beeacp::{
    BeeAcpState, Message, MessagePart, Run, RunExecutor, RunOutcome, SqliteRunStore,
};
use altius_protocol::Result as ProtocolResult;
use async_trait::async_trait;
use axum::Router;
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};

use crate::cli::FleetServeArgs;
use crate::error::CliError;

/// Bridges the BeeAI ACP run lifecycle onto the fleet supervisor.
///
/// A supervisor HITL interrupt maps to `RunOutcome::Awaiting`, pausing the
/// BeeAI run. Graph checkpoints are held in a process-lifetime
/// [`InMemoryCheckpointer`] shared across execute/resume, with a map from
/// BeeAI run id to graph run id: `resume` re-enters the interrupted node
/// from its latest checkpoint (with the human reply appended to the state
/// prompt). BeeAI ACP runs themselves persist in SQLite, but graph
/// checkpoints are in-memory for this slice, so after a process restart —
/// or if no checkpoint exists — `resume` falls back to a full re-run with
/// the resume message appended to the original prompt.
struct FleetRunExecutor {
    offline: bool,
    options_template: SupervisorOptions,
    checkpointer: Arc<InMemoryCheckpointer<FleetState>>,
    /// BeeAI ACP run id → graph run id, for checkpoint lookups on resume.
    graph_runs: Arc<RwLock<HashMap<RunId, RunId>>>,
}

impl FleetRunExecutor {
    fn new(offline: bool, options_template: SupervisorOptions) -> Self {
        Self {
            offline,
            options_template,
            checkpointer: Arc::new(InMemoryCheckpointer::new()),
            graph_runs: Arc::new(RwLock::new(HashMap::new())),
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
        let checkpointer: Arc<dyn Checkpointer<FleetState>> =
            Arc::clone(&self.checkpointer) as Arc<dyn Checkpointer<FleetState>>;
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
                self.graph_runs
                    .write()
                    .await
                    .insert(bee_run_id, graph_run_id);
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
        let graph_run_id = self.graph_runs.read().await.get(&run.run_id).copied();
        let Some(graph_run_id) = graph_run_id else {
            return Ok(None);
        };
        let checkpointer: Arc<dyn Checkpointer<FleetState>> =
            Arc::clone(&self.checkpointer) as Arc<dyn Checkpointer<FleetState>>;
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
            Ok(ExecutionOutcome::Interrupted { .. }) => Ok(Some(RunOutcome::Awaiting)),
            Err(error) => Ok(Some(RunOutcome::Failed(error.to_string()))),
        }
    }
}

#[async_trait]
impl RunExecutor for FleetRunExecutor {
    async fn execute(&self, run: &Run) -> ProtocolResult<RunOutcome> {
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
        SupervisorOutcome::Awaiting { .. } => RunOutcome::Awaiting,
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
    let public_url = args.public_url.clone();
    let offline = args.offline;
    let local_did = format!("did:wba:{}%3A{}:agent:altius", bind.ip(), bind.port());
    let auth_token = args.token.clone().filter(|token| !token.trim().is_empty());
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
            A2aState::new(card, Arc::new(EchoTaskHandler))
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

        let app = if include_beeacp {
            let store = SqliteRunStore::open(&run_db_path).map_err(|error| {
                CliError::message(format!("open run db `{}`: {error}", run_db_path.display()))
            })?;
            let bee = altius_protocol::beeacp::router(
                BeeAcpState::new(
                    Arc::new(store),
                    Arc::new(FleetRunExecutor::new(offline, options_template)),
                )
                .with_auth_token(auth_token.clone()),
            );
            Router::new()
                .merge(bee)
                .merge(a2a)
                .merge(anp)
                .nest_service("/app", pwa)
        } else {
            Router::new()
                .merge(a2a)
                .merge(anp)
                .nest_service("/app", pwa)
        };

        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|error| CliError::message(error.to_string()))?;
        eprintln!("altius: listening on http://{bind}");
        if include_beeacp {
            eprintln!("altius: BeeAI ACP runs at /runs (SSE at /runs/{{id}}/events)");
            eprintln!("altius: run db at {}", run_db_path.display());
            if auth_token.is_some() {
                eprintln!(
                    "altius: bearer auth ENABLED (send Authorization: Bearer <token> or ?token=)"
                );
            } else {
                eprintln!("altius: bearer auth disabled (set --token or ALTIUS_FLEET_TOKEN)");
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
