use std::sync::Arc;

use altius_agents::{run_supervisor, LlmClient, OfflineLlmClient, OpenAiCompatibleClient};
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

use crate::cli::FleetServeArgs;
use crate::error::CliError;

struct FleetRunExecutor {
    offline: bool,
}

#[async_trait]
impl RunExecutor for FleetRunExecutor {
    async fn execute(&self, run: &Run) -> ProtocolResult<RunOutcome> {
        execute_prompt(self.offline, &flatten_messages(&run.input)).await
    }

    async fn resume(&self, run: &Run, message: Option<Message>) -> ProtocolResult<RunOutcome> {
        let mut prompt = flatten_messages(&run.input);
        if let Some(message) = message.as_ref() {
            prompt.push('\n');
            prompt.push_str(&flatten_messages(std::slice::from_ref(message)));
        }
        execute_prompt(self.offline, &prompt).await
    }
}

async fn execute_prompt(offline: bool, prompt: &str) -> ProtocolResult<RunOutcome> {
    let llm = llm_client(offline)?;
    match run_supervisor(llm, prompt.to_owned()).await {
        Ok((_run_id, state)) => {
            let answer = state
                .final_answer
                .unwrap_or_else(|| "(no final answer)".into());
            Ok(RunOutcome::Completed(vec![agent_text(answer)]))
        }
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

fn agent_card(public_url: &str) -> Result<AgentCard, CliError> {
    let card = AgentCard {
        protocol_version: "0.3.0".into(),
        name: "altius".into(),
        description: "Altius SVM multi-agent fleet".into(),
        url: public_url.to_owned(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: AgentCapabilities::default(),
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "fleet-supervisor".into(),
            name: "Fleet supervisor".into(),
            description: "Route, explore, code-review, and finalize SVM engineering tasks".into(),
            tags: vec!["solana".into(), "svm".into()],
            examples: vec!["detect and lint this Anchor project".into()],
        }],
    };
    card.validate()
        .map_err(|error| CliError::message(error.to_string()))?;
    Ok(card)
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

    rt.block_on(async move {
        let card = agent_card(&public_url)?;
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

        let app = if include_beeacp {
            let bee = altius_protocol::beeacp::router(BeeAcpState::new(
                Arc::new(InMemoryRunStore::new()),
                Arc::new(FleetRunExecutor { offline }),
            ));
            Router::new().merge(bee).merge(a2a).merge(anp)
        } else {
            Router::new().merge(a2a).merge(anp)
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
        axum::serve(listener, app)
            .await
            .map_err(|error| CliError::message(error.to_string()))
    })
}
