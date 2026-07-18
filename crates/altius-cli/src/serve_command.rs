use std::path::PathBuf;
use std::sync::Arc;

use altius_agents::{
    run_supervisor_outcome_for, BrowserTooling, LlmClient, McpTools, OfflineLlmClient,
    OpenAiCompatibleClient, SupervisorOptions, SupervisorOutcome,
};
use altius_mcp::{McpAttachConfig, McpAttachments};
use altius_protocol::a2a::{A2aState, AgentCapabilities, AgentCard, AgentSkill, EchoTaskHandler};
use altius_protocol::anp::{
    AgentDescription, AgentRegistry, AnpState, InMemoryRegistry, InterfaceDescription,
};
use altius_protocol::beeacp::{
    BeeAcpState, InMemoryRunStore, Message, MessagePart, Run, RunExecutor, RunOutcome,
};
use altius_protocol::Result as ProtocolResult;
use async_trait::async_trait;
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::cli::FleetServeArgs;
use crate::error::CliError;

/// Bridges the BeeAI ACP run lifecycle onto the fleet supervisor.
///
/// A supervisor HITL interrupt maps to `RunOutcome::Awaiting`, pausing the
/// BeeAI run. `resume` currently re-runs the supervisor with the original
/// prompt plus the resume message; true resume-from-checkpoint wiring is
/// future work.
struct FleetRunExecutor {
    offline: bool,
    options_template: SupervisorOptions,
}

#[async_trait]
impl RunExecutor for FleetRunExecutor {
    async fn execute(&self, run: &Run) -> ProtocolResult<RunOutcome> {
        execute_prompt(
            self.offline,
            &flatten_messages(&run.input),
            options_for_run(&self.options_template, &run.agent_name),
        )
        .await
    }

    async fn resume(&self, run: &Run, message: Option<Message>) -> ProtocolResult<RunOutcome> {
        let mut prompt = flatten_messages(&run.input);
        if let Some(message) = message.as_ref() {
            prompt.push('\n');
            prompt.push_str(&flatten_messages(std::slice::from_ref(message)));
        }
        execute_prompt(
            self.offline,
            &prompt,
            options_for_run(&self.options_template, &run.agent_name),
        )
        .await
    }
}

fn options_for_run(template: &SupervisorOptions, agent_name: &str) -> SupervisorOptions {
    SupervisorOptions {
        agent_name: Some(agent_name.to_owned()),
        browser: template.browser.clone(),
    }
}

async fn execute_prompt(
    offline: bool,
    prompt: &str,
    options: SupervisorOptions,
) -> ProtocolResult<RunOutcome> {
    let llm = llm_client(offline)?;
    match run_supervisor_outcome_for(llm, prompt.to_owned(), options).await {
        Ok((_run_id, SupervisorOutcome::Finished(state))) => {
            let answer = state
                .final_answer
                .unwrap_or_else(|| "(no final answer)".into());
            Ok(RunOutcome::Completed(vec![agent_text(answer)]))
        }
        Ok((_run_id, SupervisorOutcome::Awaiting { .. })) => Ok(RunOutcome::Awaiting),
        Err(error) => Ok(RunOutcome::Failed(error.to_string())),
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
    let mut skills = vec![AgentSkill {
        id: "fleet-supervisor".into(),
        name: "Fleet supervisor".into(),
        description: "Route, explore, code-review, and finalize SVM engineering tasks".into(),
        tags: vec!["solana".into(), "svm".into()],
        examples: vec!["detect and lint this Anchor project".into()],
    }];
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
) -> Result<(SupervisorOptions, bool), CliError> {
    let attachments = Arc::new(McpAttachments::new());
    let mut browser_tooling = None;
    if let Some(config) = browser_mcp_config(args)? {
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
        },
        browser_enabled,
    ))
}

fn pwa_assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pwa")
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
    let serve_args = FleetServeArgs {
        bind: args.bind.clone(),
        public_url: args.public_url.clone(),
        offline: args.offline,
        browser_mcp_cmd: args.browser_mcp_cmd.clone(),
        browser_mcp_args: args.browser_mcp_args.clone(),
    };

    rt.block_on(async move {
        let (options_template, browser_enabled) = build_supervisor_options(&serve_args).await?;
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
            let bee = altius_protocol::beeacp::router(BeeAcpState::new(
                Arc::new(InMemoryRunStore::new()),
                Arc::new(FleetRunExecutor {
                    offline,
                    options_template,
                }),
            ));
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
            eprintln!("altius: BeeAI ACP runs at /runs");
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
