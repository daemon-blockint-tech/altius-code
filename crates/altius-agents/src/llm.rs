use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};

/// Chat role for provider-neutral messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For [`Role::Tool`] messages: id of the tool call this result answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For [`Role::Assistant`] messages that requested tool calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl ChatMessage {
    fn plain(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::plain(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain(Role::Assistant, content)
    }

    /// Assistant turn that requested tool calls; echoed back to the provider
    /// so the following [`Role::Tool`] results have a valid parent.
    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls,
            ..Self::plain(Role::Assistant, content)
        }
    }

    /// Result of executing one tool call.
    pub fn tool(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            name: Some(name.into()),
            tool_call_id: Some(tool_call_id.into()),
            ..Self::plain(Role::Tool, content)
        }
    }
}

/// Optional tool definition for tool-capable completions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema object for parameters.
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Provider-neutral async LLM client.
///
/// Phase B+ adapters (Anthropic, local, etc.) implement this trait.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Complete a chat turn and return assistant text.
    async fn complete(&self, messages: &[ChatMessage]) -> AgentResult<String>;

    /// Tool-call capable completion. Default: ignore tools and call [`complete`].
    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolSpec],
    ) -> AgentResult<(String, Vec<ToolCall>)> {
        let text = self.complete(messages).await?;
        Ok((text, Vec::new()))
    }
}

/// OpenAI-compatible HTTP chat completions client (`/v1/chat/completions`).
///
/// Environment variables (read by [`OpenAiCompatibleClient::from_env`]):
/// - `ALTIUS_LLM_BASE_URL` (default `https://api.openai.com/v1`)
/// - `ALTIUS_LLM_API_KEY` or `OPENAI_API_KEY` (required)
/// - `ALTIUS_LLM_MODEL` (default `gpt-4o-mini`)
pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("ALTIUS_LLM_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| {
                AgentError::config("set ALTIUS_LLM_API_KEY or OPENAI_API_KEY (or use --offline)")
            })?;
        let base_url = std::env::var("ALTIUS_LLM_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned());
        let model = std::env::var("ALTIUS_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
        Ok(Self::new(base_url, api_key, model))
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ApiTool<'a>>,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ApiToolCall<'a>>,
}

/// OpenAI function-calling tool definition: `{"type":"function","function":{...}}`.
#[derive(Serialize)]
struct ApiTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: ApiFunctionSpec<'a>,
}

#[derive(Serialize)]
struct ApiFunctionSpec<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

#[derive(Serialize)]
struct ApiToolCall<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    function: ApiFunctionCall<'a>,
}

#[derive(Serialize)]
struct ApiFunctionCall<'a> {
    name: &'a str,
    /// The wire format carries arguments as a JSON-encoded string.
    arguments: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ResponseToolCall>,
}

#[derive(Deserialize)]
struct ResponseToolCall {
    id: String,
    function: ResponseFunctionCall,
}

#[derive(Deserialize)]
struct ResponseFunctionCall {
    name: String,
    /// JSON-encoded string on the wire; parsed leniently into a Value.
    arguments: String,
}

impl OpenAiCompatibleClient {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> AgentResult<ChoiceMessage> {
        let api_messages: Vec<ApiMessage<'_>> = messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                },
                content: &m.content,
                name: m.name.as_deref(),
                tool_call_id: m.tool_call_id.as_deref(),
                tool_calls: m
                    .tool_calls
                    .iter()
                    .map(|call| ApiToolCall {
                        id: &call.id,
                        kind: "function",
                        function: ApiFunctionCall {
                            name: &call.name,
                            arguments: call.arguments.to_string(),
                        },
                    })
                    .collect(),
            })
            .collect();
        let api_tools: Vec<ApiTool<'_>> = tools
            .iter()
            .map(|tool| ApiTool {
                kind: "function",
                function: ApiFunctionSpec {
                    name: &tool.name,
                    description: &tool.description,
                    parameters: &tool.parameters,
                },
            })
            .collect();

        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&ChatCompletionRequest {
                model: &self.model,
                messages: api_messages,
                tools: api_tools,
            })
            .send()
            .await
            .map_err(|e| AgentError::llm(format!("request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let safe = altius_core::redact_secrets(&body);
            return Err(AgentError::llm(format!("HTTP {status}: {safe}")));
        }

        let parsed: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| AgentError::llm(format!("decode failed: {e}")))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| AgentError::llm("empty completion choices"))
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn complete(&self, messages: &[ChatMessage]) -> AgentResult<String> {
        self.chat(messages, &[])
            .await?
            .content
            .ok_or_else(|| AgentError::llm("empty completion choices"))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> AgentResult<(String, Vec<ToolCall>)> {
        let message = self.chat(messages, tools).await?;
        let calls = message
            .tool_calls
            .into_iter()
            .map(|call| ToolCall {
                id: call.id,
                name: call.function.name,
                // Providers occasionally emit arguments that are not valid
                // JSON; keep the raw text so the tool can still see it.
                arguments: serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::String(call.function.arguments)),
            })
            .collect();
        Ok((message.content.unwrap_or_default(), calls))
    }
}

/// Deterministic offline LLM for tests and `altius fleet run --offline`.
///
/// Produces Altius-authored heuristic replies from the prompt — never calls the network.
#[derive(Debug, Default, Clone)]
pub struct OfflineLlmClient;

#[async_trait]
impl LlmClient for OfflineLlmClient {
    async fn complete(&self, messages: &[ChatMessage]) -> AgentResult<String> {
        let system = messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let user = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        // Prefer the "User prompt:" section so labels like "Code notes:" do not
        // poison offline routing heuristics.
        let prompt_section = user
            .split("User prompt:")
            .nth(1)
            .and_then(|rest| rest.split("\n\n").next())
            .unwrap_or(user)
            .to_ascii_lowercase();

        let reply = if system.contains("ROUTER") {
            let route = if prompt_section.contains("@security")
                || prompt_section.contains("audit")
                || prompt_section.contains("vulnerab")
                || prompt_section.contains("security scan")
                || (prompt_section.contains("lint") && prompt_section.contains("secur"))
            {
                "security"
            } else if prompt_section.contains("@browser") {
                "browser"
            } else if prompt_section.contains("refactor")
                || prompt_section.contains("implement")
                || prompt_section.contains("edit")
            {
                "coder"
            } else if prompt_section.contains("explore")
                || prompt_section.contains("find")
                || prompt_section.contains("search")
            {
                "explorer"
            } else {
                "both"
            };
            format!(
                "PLAN: Decompose the user request into explore + edit steps as needed.\nROUTE: {route}"
            )
        } else if system.contains("EXPLORER") {
            format!(
                "EXPLORATION: Reviewed request «{}». Identified relevant modules and read-only findings.",
                prompt_section.trim()
            )
        } else if system.contains("CODER") {
            format!(
                "CODE_NOTES: Proposed safe, non-signing edits for «{}». No transactions constructed.",
                prompt_section.trim()
            )
        } else if system.contains("BROWSER") {
            format!(
                "BROWSER: Offline pass for «{}». No live browser MCP tools were invoked.",
                prompt_section.trim()
            )
        } else if system.contains("SECURITY") {
            format!(
                "SECURITY: Offline audit pass for «{}». Detect/lint tools available; no signing.",
                prompt_section.trim()
            )
        } else if system.contains("CRITIC") {
            "CRITIQUE: Trajectory looks coherent. No policy violations detected. APPROVE".to_owned()
        } else if system.contains("FINALIZE") {
            format!(
                "FINAL: Completed fleet pass for «{}». No signing or broadcast was attempted.",
                prompt_section.trim()
            )
        } else {
            format!("ACK: {}", prompt_section.trim())
        };
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn offline_llm_routes_coder() {
        let llm = OfflineLlmClient;
        let text = llm
            .complete(&[
                ChatMessage::system("You are the ALTIUS ROUTER agent."),
                ChatMessage::user("please implement a helper"),
            ])
            .await
            .unwrap();
        assert!(text.contains("ROUTE: coder"));
    }
}
