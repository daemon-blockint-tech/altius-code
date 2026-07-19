use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use altius_agents::{run_supervisor, LlmClient, OfflineLlmClient};
use altius_protocol::editor_acp::{
    AgentCapabilities, ContentBlock, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, NewSessionParams,
    NewSessionResult, PromptParams, PromptResult, SessionCancelParams, SessionId, StopReason,
    METHOD_INITIALIZE, METHOD_SESSION_CANCEL, METHOD_SESSION_NEW, METHOD_SESSION_PROMPT,
};
use uuid::Uuid;

use crate::cli::FleetAcpArgs;
use crate::error::CliError;

/// Serve Editor ACP (Agent Client Protocol) over newline-delimited stdio JSON-RPC.
pub fn run_acp_cmd(args: &FleetAcpArgs) -> Result<(), CliError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::message(format!("tokio runtime: {error}")))?;

    let sessions = Arc::new(Mutex::new(HashSet::<String>::new()));
    let cancelled = Arc::new(Mutex::new(HashSet::<String>::new()));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(CliError::Io)?;
        if line.trim().is_empty() {
            continue;
        }
        let message = match JsonRpcMessage::decode(line.as_bytes()) {
            Ok(message) => message,
            Err(error) => {
                write_line(
                    &mut stdout,
                    &JsonRpcMessage::Response(JsonRpcResponse::failure(
                        altius_protocol::editor_acp::RequestId::Number(0),
                        JsonRpcError::invalid_params(error.to_string()),
                    )),
                )?;
                continue;
            }
        };

        match message {
            JsonRpcMessage::Request(request) => {
                let response = rt.block_on(handle_request(
                    request,
                    args.offline,
                    Arc::clone(&sessions),
                    Arc::clone(&cancelled),
                ));
                write_line(&mut stdout, &JsonRpcMessage::Response(response))?;
            }
            JsonRpcMessage::Notification(notification) => {
                handle_notification(notification, Arc::clone(&cancelled))?;
            }
            JsonRpcMessage::Response(_) => {
                // Editor ACP agents do not expect unsolicited client responses.
            }
        }
    }

    Ok(())
}

async fn handle_request(
    request: JsonRpcRequest,
    offline: bool,
    sessions: Arc<Mutex<HashSet<String>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
) -> JsonRpcResponse {
    match request.method.as_str() {
        METHOD_INITIALIZE => match decode_params::<InitializeParams>(request.params) {
            Ok(_params) => JsonRpcResponse::success(
                request.id,
                serde_json::to_value(InitializeResult {
                    protocol_version: 1,
                    agent_capabilities: AgentCapabilities {
                        load_session: false,
                    },
                })
                .unwrap_or_default(),
            ),
            Err(error) => JsonRpcResponse::failure(request.id, error),
        },
        METHOD_SESSION_NEW => match decode_params::<NewSessionParams>(request.params) {
            Ok(params) => {
                if let Err(error) = params.validate() {
                    return JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError::invalid_params(error.to_string()),
                    );
                }
                let session_id = SessionId(Uuid::new_v4().to_string());
                sessions.lock().unwrap().insert(session_id.0.clone());
                JsonRpcResponse::success(
                    request.id,
                    serde_json::to_value(NewSessionResult { session_id }).unwrap_or_default(),
                )
            }
            Err(error) => JsonRpcResponse::failure(request.id, error),
        },
        METHOD_SESSION_PROMPT => match decode_params::<PromptParams>(request.params) {
            Ok(params) => {
                if let Err(error) = params.validate() {
                    return JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError::invalid_params(error.to_string()),
                    );
                }
                if !sessions.lock().unwrap().contains(&params.session_id.0) {
                    return JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError::invalid_params("unknown sessionId"),
                    );
                }
                if cancelled.lock().unwrap().remove(&params.session_id.0) {
                    return JsonRpcResponse::success(
                        request.id,
                        serde_json::to_value(PromptResult {
                            stop_reason: StopReason::Cancelled,
                        })
                        .unwrap_or_default(),
                    );
                }
                match run_prompt(offline, &params).await {
                    Ok(result) => JsonRpcResponse::success(
                        request.id,
                        serde_json::to_value(result).unwrap_or_default(),
                    ),
                    Err(error) => JsonRpcResponse::failure(
                        request.id,
                        JsonRpcError {
                            code: -32603,
                            message: error,
                            data: None,
                        },
                    ),
                }
            }
            Err(error) => JsonRpcResponse::failure(request.id, error),
        },
        other => JsonRpcResponse::failure(request.id, JsonRpcError::method_not_found(other)),
    }
}

fn handle_notification(
    notification: JsonRpcNotification,
    cancelled: Arc<Mutex<HashSet<String>>>,
) -> Result<(), CliError> {
    if notification.method != METHOD_SESSION_CANCEL {
        return Ok(());
    }
    let params: SessionCancelParams =
        serde_json::from_value(notification.params.unwrap_or_default())
            .map_err(|error| CliError::message(error.to_string()))?;
    params
        .session_id
        .validate()
        .map_err(|error| CliError::message(error.to_string()))?;
    cancelled.lock().unwrap().insert(params.session_id.0);
    Ok(())
}

async fn run_prompt(offline: bool, params: &PromptParams) -> Result<PromptResult, String> {
    let prompt = params
        .prompt
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let llm: Arc<dyn LlmClient> = if offline {
        Arc::new(OfflineLlmClient)
    } else if std::env::var("ALTIUS_LLM_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok()
    {
        altius_agents::llm_from_env(altius_agents::TaskClass::General)
            .map_err(|error| error.to_string())?
    } else {
        Arc::new(OfflineLlmClient)
    };
    run_supervisor(llm, prompt)
        .await
        .map_err(|error| error.to_string())?;
    Ok(PromptResult {
        stop_reason: StopReason::EndTurn,
    })
}

fn decode_params<T: serde::de::DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, JsonRpcError> {
    serde_json::from_value(params.unwrap_or(serde_json::json!({})))
        .map_err(|error| JsonRpcError::invalid_params(error.to_string()))
}

fn write_line(stdout: &mut impl Write, message: &JsonRpcMessage) -> Result<(), CliError> {
    let bytes = message
        .encode()
        .map_err(|error| CliError::message(error.to_string()))?;
    stdout.write_all(&bytes).map_err(CliError::Io)?;
    stdout.write_all(b"\n").map_err(CliError::Io)?;
    stdout.flush().map_err(CliError::Io)
}
