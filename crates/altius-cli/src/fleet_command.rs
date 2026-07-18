use std::sync::Arc;

use altius_agents::{run_supervisor, LlmClient, OfflineLlmClient, OpenAiCompatibleClient};
use altius_core::redact_secrets;

use crate::cli::{FleetMcpArgs, FleetRunArgs, McpTransport};
use crate::error::CliError;

/// Execute `altius fleet run --prompt ...` headlessly.
pub fn run_fleet_cmd(args: &FleetRunArgs) -> Result<(), CliError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::message(format!("tokio runtime: {e}")))?;

    let prompt = args.prompt.clone();
    let offline = args.offline;
    let project = args.project.display().to_string();

    rt.block_on(async move {
        let llm: Arc<dyn LlmClient> = if offline {
            Arc::new(OfflineLlmClient)
        } else if std::env::var("ALTIUS_LLM_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok()
        {
            Arc::new(
                OpenAiCompatibleClient::from_env().map_err(|e| CliError::message(e.to_string()))?,
            )
        } else {
            eprintln!(
                "altius: no ALTIUS_LLM_API_KEY/OPENAI_API_KEY — using OfflineLlmClient \
                 (pass --offline to silence this message)"
            );
            Arc::new(OfflineLlmClient)
        };

        let grounded = format!("{prompt}\n\n[project_path={project}]");

        let (run_id, state) = run_supervisor(llm, grounded)
            .await
            .map_err(|e| CliError::message(format!("fleet run failed: {e}")))?;

        let answer = state.final_answer.as_deref().unwrap_or("(no final answer)");
        let safe = redact_secrets(answer);

        println!("run_id: {run_id}");
        if let Some(cid) = state.correlation_id {
            println!("correlation_id: {cid}");
        }
        println!("project: {project}");
        println!("route: {:?}", state.route);
        println!("trace: {}", state.trace.join(" -> "));
        println!();
        println!("{safe}");
        Ok(())
    })
}

pub fn run_mcp_cmd(args: &FleetMcpArgs) -> Result<(), CliError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::message(format!("tokio runtime: {error}")))?;
    let workspace = args.workspace.clone();
    match args.transport {
        McpTransport::Stdio => rt
            .block_on(altius_mcp::serve_stdio(workspace))
            .map_err(|error| CliError::message(error.to_string())),
        McpTransport::Http => {
            let bind = args
                .bind
                .parse()
                .map_err(|error| CliError::message(format!("invalid --bind address: {error}")))?;
            rt.block_on(altius_mcp::serve_http(workspace, bind))
                .map_err(|error| CliError::message(error.to_string()))
        }
    }
}
