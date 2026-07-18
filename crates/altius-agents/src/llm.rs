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
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            name: None,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
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
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn complete(&self, messages: &[ChatMessage]) -> AgentResult<String> {
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
            .and_then(|c| c.message.content)
            .ok_or_else(|| AgentError::llm("empty completion choices"))
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
            let route = if prompt_section.contains("refactor")
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
