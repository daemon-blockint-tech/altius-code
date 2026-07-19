use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskClass {
    Routing,
    Exploration,
    Coding,
    Security,
    Browser,
    Critique,
    #[default]
    General,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelCapability {
    Fast,
    Reasoning,
    Coding,
    ToolUse,
    #[default]
    General,
}

#[derive(Clone, Debug)]
pub struct InferencePolicy {
    pub timeout: Duration,
    pub max_retries_per_provider: usize,
    pub backoff: Duration,
    pub max_estimated_tokens: usize,
    /// Optional fail-closed estimated cost ceiling in micro-USD.
    pub max_estimated_cost_microusd: Option<usize>,
    /// Configured estimate used when the provider does not return usage.
    pub cost_per_1k_tokens_microusd: usize,
}

impl Default for InferencePolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_retries_per_provider: 2,
            backoff: Duration::from_millis(250),
            max_estimated_tokens: 64_000,
            max_estimated_cost_microusd: None,
            cost_per_1k_tokens_microusd: 0,
        }
    }
}

impl InferencePolicy {
    pub fn from_env() -> Self {
        let mut policy = Self::default();
        if let Ok(value) = std::env::var("ALTIUS_LLM_TIMEOUT_MS") {
            if let Ok(ms) = value.parse::<u64>() {
                policy.timeout = Duration::from_millis(ms.max(1));
            }
        }
        if let Ok(value) = std::env::var("ALTIUS_LLM_MAX_RETRIES") {
            if let Ok(retries) = value.parse::<usize>() {
                policy.max_retries_per_provider = retries.min(8);
            }
        }
        if let Ok(value) = std::env::var("ALTIUS_LLM_BACKOFF_MS") {
            if let Ok(ms) = value.parse::<u64>() {
                policy.backoff = Duration::from_millis(ms);
            }
        }
        if let Ok(value) = std::env::var("ALTIUS_LLM_TOKEN_BUDGET") {
            if let Ok(tokens) = value.parse::<usize>() {
                policy.max_estimated_tokens = tokens.max(1);
            }
        }
        if let Ok(value) = std::env::var("ALTIUS_LLM_COST_BUDGET_MICROUSD") {
            policy.max_estimated_cost_microusd = value.parse::<usize>().ok();
        }
        if let Ok(value) = std::env::var("ALTIUS_LLM_COST_PER_1K_TOKENS_MICROUSD") {
            if let Ok(cost) = value.parse::<usize>() {
                policy.cost_per_1k_tokens_microusd = cost;
            }
        }
        policy
    }
}

#[derive(Clone)]
pub struct ProviderCandidate {
    pub name: String,
    pub capability: ModelCapability,
    pub client: Arc<dyn LlmClient>,
}

/// Provider-neutral timeout/retry/fallback and run-budget wrapper.
pub struct PolicyLlmClient {
    providers: Vec<ProviderCandidate>,
    task: TaskClass,
    policy: InferencePolicy,
    estimated_tokens: AtomicUsize,
    estimated_cost_microusd: AtomicUsize,
}

impl PolicyLlmClient {
    pub fn new(
        providers: Vec<ProviderCandidate>,
        task: TaskClass,
        policy: InferencePolicy,
    ) -> AgentResult<Self> {
        if providers.is_empty() {
            return Err(AgentError::config("inference fallback chain is empty"));
        }
        Ok(Self {
            providers,
            task,
            policy,
            estimated_tokens: AtomicUsize::new(0),
            estimated_cost_microusd: AtomicUsize::new(0),
        })
    }

    pub fn single(client: Arc<dyn LlmClient>, task: TaskClass, policy: InferencePolicy) -> Self {
        Self::new(
            vec![ProviderCandidate {
                name: "configured".into(),
                capability: capability_for_task(task),
                client,
            }],
            task,
            policy,
        )
        .expect("single provider is non-empty")
    }

    fn ordered_providers(&self) -> Vec<&ProviderCandidate> {
        let preferred = capability_for_task(self.task);
        let mut providers: Vec<_> = self.providers.iter().collect();
        providers.sort_by_key(|provider| provider.capability != preferred);
        providers
    }

    fn reserve_input(&self, messages: &[ChatMessage]) -> AgentResult<()> {
        self.reserve(messages.iter().map(|message| message.content.len()).sum())
    }

    fn reserve(&self, chars: usize) -> AgentResult<()> {
        let tokens = chars.div_ceil(4);
        let previous = self.estimated_tokens.fetch_add(tokens, Ordering::SeqCst);
        if previous.saturating_add(tokens) > self.policy.max_estimated_tokens {
            self.estimated_tokens.fetch_sub(tokens, Ordering::SeqCst);
            return Err(AgentError::llm("inference token budget exhausted"));
        }
        let estimated_cost = tokens
            .saturating_mul(self.policy.cost_per_1k_tokens_microusd)
            .div_ceil(1_000);
        if let Some(limit) = self.policy.max_estimated_cost_microusd {
            let previous_cost = self
                .estimated_cost_microusd
                .fetch_add(estimated_cost, Ordering::SeqCst);
            if previous_cost.saturating_add(estimated_cost) > limit {
                self.estimated_cost_microusd
                    .fetch_sub(estimated_cost, Ordering::SeqCst);
                self.estimated_tokens.fetch_sub(tokens, Ordering::SeqCst);
                return Err(AgentError::llm("inference cost budget exhausted"));
            }
        }
        Ok(())
    }

    async fn retry_delay(&self, attempt: usize) {
        let factor = 1u32.checked_shl(attempt.min(8) as u32).unwrap_or(256);
        tokio::time::sleep(self.policy.backoff.saturating_mul(factor)).await;
    }
}

fn capability_for_task(task: TaskClass) -> ModelCapability {
    match task {
        TaskClass::Routing => ModelCapability::Fast,
        TaskClass::Coding => ModelCapability::Coding,
        TaskClass::Security | TaskClass::Critique => ModelCapability::Reasoning,
        TaskClass::Browser => ModelCapability::ToolUse,
        TaskClass::Exploration | TaskClass::General => ModelCapability::General,
    }
}

fn retryable(error: &AgentError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    let generally_retryable = !text.contains("unauthorized")
        && !text.contains("forbidden")
        && !text.contains("invalid api")
        && !text.contains("budget exhausted")
        && !text.contains("http 4");
    generally_retryable || text.contains("http 408") || text.contains("http 429")
}

#[async_trait]
impl LlmClient for PolicyLlmClient {
    async fn complete(&self, messages: &[ChatMessage]) -> AgentResult<String> {
        self.reserve_input(messages)?;
        let mut last = None;
        for provider in self.ordered_providers() {
            for attempt in 0..=self.policy.max_retries_per_provider {
                match tokio::time::timeout(self.policy.timeout, provider.client.complete(messages))
                    .await
                {
                    Ok(Ok(text)) => {
                        self.reserve(text.len())?;
                        return Ok(text);
                    }
                    Ok(Err(error)) => {
                        let can_retry = retryable(&error);
                        last = Some(error);
                        if !can_retry {
                            break;
                        }
                    }
                    Err(_) => {
                        last = Some(AgentError::llm(format!(
                            "provider `{}` timed out",
                            provider.name
                        )));
                    }
                }
                if attempt < self.policy.max_retries_per_provider {
                    self.retry_delay(attempt).await;
                }
            }
        }
        Err(last.unwrap_or_else(|| AgentError::llm("inference fallback chain exhausted")))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> AgentResult<(String, Vec<ToolCall>)> {
        self.reserve_input(messages)?;
        let mut last = None;
        for provider in self.ordered_providers() {
            for attempt in 0..=self.policy.max_retries_per_provider {
                match tokio::time::timeout(
                    self.policy.timeout,
                    provider.client.complete_with_tools(messages, tools),
                )
                .await
                {
                    Ok(Ok((text, calls))) => {
                        self.reserve(text.len())?;
                        return Ok((text, calls));
                    }
                    Ok(Err(error)) => {
                        let can_retry = retryable(&error);
                        last = Some(error);
                        if !can_retry {
                            break;
                        }
                    }
                    Err(_) => {
                        last = Some(AgentError::llm(format!(
                            "provider `{}` timed out",
                            provider.name
                        )));
                    }
                }
                if attempt < self.policy.max_retries_per_provider {
                    self.retry_delay(attempt).await;
                }
            }
        }
        Err(last.unwrap_or_else(|| AgentError::llm("inference fallback chain exhausted")))
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
    ) -> AgentResult<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| AgentError::config(format!("build LLM HTTP client: {error}")))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
        })
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
        Self::new(base_url, api_key, model)
    }
}

/// Build a model-fallback chain from the existing OpenAI-compatible env.
///
/// `ALTIUS_LLM_FALLBACK_MODELS` is an optional comma-separated list. All
/// models use the same configured endpoint and credential; providers with
/// different endpoints can construct [`PolicyLlmClient`] directly.
pub fn llm_from_env(task: TaskClass) -> AgentResult<Arc<dyn LlmClient>> {
    let api_key = std::env::var("ALTIUS_LLM_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| {
            AgentError::config("set ALTIUS_LLM_API_KEY or OPENAI_API_KEY (or use --offline)")
        })?;
    let base_url = std::env::var("ALTIUS_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned());
    let primary = std::env::var("ALTIUS_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
    let mut models = vec![primary];
    if let Ok(fallbacks) = std::env::var("ALTIUS_LLM_FALLBACK_MODELS") {
        models.extend(
            fallbacks
                .split(',')
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_owned),
        );
    }
    models.dedup();
    let capability = capability_for_task(task);
    let providers = models
        .into_iter()
        .map(|model| {
            let client =
                OpenAiCompatibleClient::new(base_url.clone(), api_key.clone(), model.clone())?;
            Ok(ProviderCandidate {
                name: model,
                capability,
                client: Arc::new(client),
            })
        })
        .collect::<AgentResult<Vec<_>>>()?;
    Ok(Arc::new(PolicyLlmClient::new(
        providers,
        task,
        InferencePolicy::from_env(),
    )?))
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
            } else if prompt_section.contains("@github")
                || prompt_section.contains("pull request")
                || prompt_section.contains("github issue")
            {
                "github"
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
        } else if system.contains("GITHUB") {
            format!(
                "GITHUB: Offline pass for «{}». No live GitHub MCP tools were invoked.",
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    struct FakeClient {
        calls: AtomicUsize,
        failures: usize,
        error: &'static str,
        delay: Duration,
        answer: &'static str,
    }

    #[async_trait]
    impl LlmClient for FakeClient {
        async fn complete(&self, _messages: &[ChatMessage]) -> AgentResult<String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if call < self.failures {
                Err(AgentError::llm(self.error))
            } else {
                Ok(self.answer.into())
            }
        }
    }

    fn candidate(name: &str, client: Arc<dyn LlmClient>) -> ProviderCandidate {
        ProviderCandidate {
            name: name.into(),
            capability: ModelCapability::General,
            client,
        }
    }

    #[tokio::test]
    async fn retries_retryable_then_succeeds() {
        let fake = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: 1,
            error: "HTTP 503",
            delay: Duration::ZERO,
            answer: "ok",
        });
        let policy = PolicyLlmClient::new(
            vec![candidate("first", fake.clone())],
            TaskClass::General,
            InferencePolicy {
                backoff: Duration::ZERO,
                ..InferencePolicy::default()
            },
        )
        .unwrap();
        assert_eq!(
            policy.complete(&[ChatMessage::user("x")]).await.unwrap(),
            "ok"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn timeout_and_nonretryable_errors_fall_back() {
        let slow = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: 0,
            error: "",
            delay: Duration::from_millis(50),
            answer: "late",
        });
        let auth = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: usize::MAX,
            error: "HTTP 401 unauthorized",
            delay: Duration::ZERO,
            answer: "",
        });
        let fallback = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: 0,
            error: "",
            delay: Duration::ZERO,
            answer: "fallback",
        });
        let policy = PolicyLlmClient::new(
            vec![
                candidate("slow", slow),
                candidate("auth", auth.clone()),
                candidate("fallback", fallback),
            ],
            TaskClass::General,
            InferencePolicy {
                timeout: Duration::from_millis(5),
                max_retries_per_provider: 0,
                backoff: Duration::ZERO,
                ..InferencePolicy::default()
            },
        )
        .unwrap();
        assert_eq!(
            policy.complete(&[ChatMessage::user("x")]).await.unwrap(),
            "fallback"
        );
        assert_eq!(auth.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn budget_exhaustion_fails_closed() {
        let fake = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: 0,
            error: "",
            delay: Duration::ZERO,
            answer: "ok",
        });
        let policy = PolicyLlmClient::new(
            vec![candidate("one", fake.clone())],
            TaskClass::General,
            InferencePolicy {
                max_estimated_tokens: 1,
                ..InferencePolicy::default()
            },
        )
        .unwrap();
        let error = policy
            .complete(&[ChatMessage::user("this request exceeds budget")])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("budget exhausted"));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn estimated_cost_budget_fails_closed() {
        let fake = Arc::new(FakeClient {
            calls: AtomicUsize::new(0),
            failures: 0,
            error: "",
            delay: Duration::ZERO,
            answer: "ok",
        });
        let policy = PolicyLlmClient::new(
            vec![candidate("one", fake.clone())],
            TaskClass::General,
            InferencePolicy {
                max_estimated_cost_microusd: Some(1),
                cost_per_1k_tokens_microusd: 10_000,
                ..InferencePolicy::default()
            },
        )
        .unwrap();
        let error = policy
            .complete(&[ChatMessage::user("costly")])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("cost budget exhausted"));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }
}
