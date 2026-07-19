use std::sync::Arc;

use altius_core::{Budget, CorrelationId, RunId};
use altius_graph::{
    Checkpointer, ExecutionOutcome, Graph, GraphBuilder, GraphExecutor, InMemoryCheckpointer, Node,
    NodeResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};
use crate::hooks::{HookedDispatcher, ToolHook};
use crate::llm::{ChatMessage, LlmClient, OfflineLlmClient, ToolSpec};
use crate::permissions::{PermissionedDispatcher, ToolPolicy};
use crate::project_memory;
use crate::prompts;
use crate::roles::AgentRole;
use crate::tools::{self, LocalTools, ToolDispatcher};

/// Which specialist path the router selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FleetRoute {
    Explorer,
    Coder,
    Browser,
    GitHub,
    Security,
    #[default]
    Both,
}

impl FleetRoute {
    pub fn parse(raw: &str) -> Self {
        let lower = raw.to_ascii_lowercase();
        if lower.contains("security") {
            Self::Security
        } else if lower.contains("github") {
            Self::GitHub
        } else if lower.contains("browser") {
            Self::Browser
        } else if lower.contains("explorer") && !lower.contains("coder") && !lower.contains("both")
        {
            Self::Explorer
        } else if lower.contains("coder") && !lower.contains("explorer") && !lower.contains("both")
        {
            Self::Coder
        } else {
            Self::Both
        }
    }
}

/// Optional browser MCP tooling injected at graph-build time.
#[derive(Clone)]
pub struct BrowserTooling {
    pub tools: Vec<ToolSpec>,
    pub dispatcher: Arc<dyn ToolDispatcher>,
}

/// Optional GitHub MCP tooling injected at graph-build time.
#[derive(Clone)]
pub struct GitHubTooling {
    pub tools: Vec<ToolSpec>,
    pub dispatcher: Arc<dyn ToolDispatcher>,
}

/// Knobs for a supervisor pass (agent routing + optional browser attach).
#[derive(Clone, Default)]
pub struct SupervisorOptions {
    /// BeeAI ACP / CLI agent name (e.g. `"browser"`). Forces a route when set.
    pub agent_name: Option<String>,
    /// Live browser MCP tools. When `None`, the browser node runs as plain LLM.
    pub browser: Option<BrowserTooling>,
    /// Live GitHub MCP tools. When `None`, the GitHub node runs as plain LLM.
    pub github: Option<GitHubTooling>,
    /// Deterministic Pre/Post tool hooks applied to every tool dispatcher.
    pub hooks: Vec<Arc<dyn ToolHook>>,
}

/// Build Hooked → Permissioned → LocalTools for a role policy.
fn harness_dispatcher(
    project_root: &std::path::Path,
    base_policy: ToolPolicy,
    hooks: &[Arc<dyn ToolHook>],
) -> Arc<dyn ToolDispatcher> {
    let policy = ToolPolicy::load_from_project(project_root, base_policy);
    let local = Arc::new(LocalTools::with_policy(project_root, &policy));
    let permissioned = Arc::new(PermissionedDispatcher::new(policy, local));
    Arc::new(HookedDispatcher::new(hooks.to_vec(), permissioned))
}

fn wrap_with_hooks(
    inner: Arc<dyn ToolDispatcher>,
    hooks: &[Arc<dyn ToolHook>],
) -> Arc<dyn ToolDispatcher> {
    Arc::new(HookedDispatcher::new(hooks.to_vec(), inner))
}

/// Shared supervisor graph state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FleetState {
    pub prompt: String,
    pub plan: Option<String>,
    pub route: FleetRoute,
    /// When set (e.g. by `agent_name=browser` or `@Browser` in the prompt),
    /// the router preserves this route instead of re-parsing its reply.
    pub forced_route: Option<FleetRoute>,
    pub exploration: Option<String>,
    pub code_notes: Option<String>,
    pub browser_notes: Option<String>,
    pub github_notes: Option<String>,
    pub security_notes: Option<String>,
    pub critique: Option<String>,
    pub final_answer: Option<String>,
    pub trace: Vec<String>,
    pub correlation_id: Option<CorrelationId>,
}

impl FleetState {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            correlation_id: Some(CorrelationId::new()),
            ..Self::default()
        }
    }

    pub fn with_options(mut self, options: &SupervisorOptions) -> Self {
        if let Some(route) = resolve_forced_route(options.agent_name.as_deref(), &self.prompt) {
            self.forced_route = Some(route);
            self.route = route;
        }
        self
    }
}

/// Map an agent name / `@Browser` / `@GitHub` / `@Security` / slash skill
/// onto a forced route.
pub fn resolve_forced_route(agent_name: Option<&str>, prompt: &str) -> Option<FleetRoute> {
    if let Some(name) = agent_name {
        let lower = name.trim().trim_start_matches('@').to_ascii_lowercase();
        if lower == "security" {
            return Some(FleetRoute::Security);
        }
        if lower == "browser" {
            return Some(FleetRoute::Browser);
        }
        if lower == "github" {
            return Some(FleetRoute::GitHub);
        }
    }
    if let Some(skill) = crate::skills::parse_slash_skill(prompt) {
        return Some(skill.route);
    }
    let prompt_lower = prompt.to_ascii_lowercase();
    if prompt_lower.contains("@security") {
        return Some(FleetRoute::Security);
    }
    if prompt_lower.contains("@browser") {
        return Some(FleetRoute::Browser);
    }
    if prompt_lower.contains("@github") {
        return Some(FleetRoute::GitHub);
    }
    None
}

struct LlmNode {
    name: &'static str,
    system: &'static str,
    llm: Arc<dyn LlmClient>,
    /// Tools offered to this node; empty means plain chat.
    tools: Vec<ToolSpec>,
    /// When set, rebuild Hooked→Permissioned→LocalTools per run from the
    /// prompt's `[project_path=…]` (or cwd).
    local_policy: Option<ToolPolicy>,
    /// External dispatcher (e.g. browser MCP), already hook-wrapped.
    dispatcher: Option<Arc<dyn ToolDispatcher>>,
    hooks: Vec<Arc<dyn ToolHook>>,
    after: AfterFn,
}

type AfterFn = Box<dyn Fn(&str, FleetState) -> NodeResult<FleetState> + Send + Sync>;

impl LlmNode {
    fn new(
        name: &'static str,
        system: &'static str,
        llm: Arc<dyn LlmClient>,
        after: impl Fn(&str, FleetState) -> NodeResult<FleetState> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            system,
            llm,
            tools: Vec::new(),
            local_policy: None,
            dispatcher: None,
            hooks: Vec::new(),
            after: Box::new(after),
        }
    }

    fn with_local_tools(
        mut self,
        tools: Vec<ToolSpec>,
        policy: ToolPolicy,
        hooks: Vec<Arc<dyn ToolHook>>,
    ) -> Self {
        self.tools = tools;
        self.local_policy = Some(policy);
        self.hooks = hooks;
        self
    }

    fn with_dispatcher(
        mut self,
        tools: Vec<ToolSpec>,
        dispatcher: Arc<dyn ToolDispatcher>,
    ) -> Self {
        self.tools = tools;
        self.dispatcher = Some(dispatcher);
        self
    }
}

#[async_trait]
impl Node<FleetState> for LlmNode {
    fn name(&self) -> &str {
        self.name
    }

    async fn run(
        &self,
        mut state: FleetState,
    ) -> altius_graph::GraphResult<NodeResult<FleetState>> {
        state.trace.push(self.name.to_owned());
        let project_root = tools::project_root_from_prompt(&state.prompt);
        let rails = crate::guardrails::GuardrailsPipeline::from_project(&project_root);

        // Input rail — only on the entry router so specialists inherit a
        // sanitized prompt without re-blocking security-domain language mid-graph.
        if self.name == AgentRole::Router.as_str() {
            let decision = rails.validate_input(&state.prompt);
            if !decision.safe {
                state.final_answer = Some(
                    crate::guardrails::GuardrailsPipeline::blocked_user_message(&decision),
                );
                return Ok(NodeResult::Finish(state));
            }
            state.prompt = decision.sanitized;
        }

        let system = match project_memory::load(&project_root) {
            Some(memory) => format!(
                "{}\n\n{}",
                self.system,
                project_memory::format_for_system(&memory)
            ),
            None => self.system.to_owned(),
        };
        let user_blob = format!(
            "User prompt:\n{}\n\nPlan:\n{}\n\nExploration:\n{}\n\nCode notes:\n{}\n\nBrowser notes:\n{}\n\nGitHub notes:\n{}\n\nSecurity notes:\n{}\n\nCritique:\n{}",
            state.prompt,
            state.plan.as_deref().unwrap_or("(none)"),
            state.exploration.as_deref().unwrap_or("(none)"),
            state.code_notes.as_deref().unwrap_or("(none)"),
            state.browser_notes.as_deref().unwrap_or("(none)"),
            state.github_notes.as_deref().unwrap_or("(none)"),
            state.security_notes.as_deref().unwrap_or("(none)"),
            state.critique.as_deref().unwrap_or("(none)"),
        );
        let messages = vec![ChatMessage::system(system), ChatMessage::user(user_blob)];
        let text = if self.tools.is_empty() {
            self.llm.complete(&messages).await
        } else if let Some(policy) = &self.local_policy {
            let dispatcher = harness_dispatcher(&project_root, policy.clone(), &self.hooks);
            tools::tool_loop(
                self.llm.as_ref(),
                &self.tools,
                dispatcher.as_ref(),
                messages,
            )
            .await
        } else if let Some(dispatcher) = &self.dispatcher {
            tools::tool_loop(
                self.llm.as_ref(),
                &self.tools,
                dispatcher.as_ref(),
                messages,
            )
            .await
        } else {
            let local = LocalTools::new(&project_root);
            tools::tool_loop(self.llm.as_ref(), &self.tools, &local, messages).await
        }
        .map_err(|e| altius_graph::GraphError::node_failed(self.name, e.to_string()))?;

        let out = rails.validate_output(&text);
        let text = if out.safe {
            out.sanitized
        } else {
            crate::guardrails::GuardrailsPipeline::blocked_user_message(&out)
        };
        Ok((self.after)(&text, state))
    }
}

/// Build the supervisor graph:
/// router → explorer/coder/browser/github/security → critic → finalize.
pub fn build_supervisor_graph(llm: Arc<dyn LlmClient>) -> AgentResult<Graph<FleetState>> {
    build_supervisor_graph_with(llm, &SupervisorOptions::default())
}

/// Build the supervisor graph with optional browser MCP tooling.
pub fn build_supervisor_graph_with(
    llm: Arc<dyn LlmClient>,
    options: &SupervisorOptions,
) -> AgentResult<Graph<FleetState>> {
    let router_llm = Arc::clone(&llm);
    let explorer_llm = Arc::clone(&llm);
    let coder_llm = Arc::clone(&llm);
    let browser_llm = Arc::clone(&llm);
    let github_llm = Arc::clone(&llm);
    let security_llm = Arc::clone(&llm);
    let critic_llm = Arc::clone(&llm);
    let finalize_llm = Arc::clone(&llm);

    let mut hooks = crate::guardrails::default_guardrail_hooks();
    hooks.extend(options.hooks.iter().cloned());

    let mut browser_node = LlmNode::new(
        AgentRole::Browser.as_str(),
        prompts::BROWSER_SYSTEM,
        browser_llm,
        |text, mut state| {
            state.browser_notes = Some(text.to_owned());
            NodeResult::Continue(state)
        },
    );
    if let Some(browser) = &options.browser {
        let browser_disp = wrap_with_hooks(Arc::clone(&browser.dispatcher), &hooks);
        browser_node = browser_node.with_dispatcher(browser.tools.clone(), browser_disp);
    }

    let mut github_node = LlmNode::new(
        AgentRole::GitHub.as_str(),
        prompts::GITHUB_SYSTEM,
        github_llm,
        |text, mut state| {
            state.github_notes = Some(text.to_owned());
            NodeResult::Continue(state)
        },
    );
    if let Some(github) = &options.github {
        let github_disp = wrap_with_hooks(Arc::clone(&github.dispatcher), &hooks);
        github_node = github_node.with_dispatcher(github.tools.clone(), github_disp);
    }

    let security_node = LlmNode::new(
        AgentRole::Security.as_str(),
        prompts::SECURITY_SYSTEM,
        security_llm,
        |text, mut state| {
            state.security_notes = Some(text.to_owned());
            NodeResult::Continue(state)
        },
    )
    .with_local_tools(
        tools::security_tools(),
        ToolPolicy::read_only(),
        hooks.clone(),
    );

    let graph = GraphBuilder::new()
        .add_node(LlmNode::new(
            AgentRole::Router.as_str(),
            prompts::ROUTER_SYSTEM,
            router_llm,
            |text, mut state| {
                state.plan = Some(text.to_owned());
                if let Some(forced) = state.forced_route {
                    state.route = forced;
                } else {
                    state.route = parse_route_from_router(text);
                }
                NodeResult::Continue(state)
            },
        ))
        .add_node(
            LlmNode::new(
                AgentRole::Explorer.as_str(),
                prompts::EXPLORER_SYSTEM,
                explorer_llm,
                |text, mut state| {
                    state.exploration = Some(text.to_owned());
                    NodeResult::Continue(state)
                },
            )
            // Read-only FS + detect/lint; offline clients that never emit tool
            // calls are unaffected.
            .with_local_tools(
                tools::explorer_tools(),
                ToolPolicy::read_only(),
                hooks.clone(),
            ),
        )
        .add_node(
            LlmNode::new(
                AgentRole::Coder.as_str(),
                prompts::CODER_SYSTEM,
                coder_llm,
                |text, mut state| {
                    state.code_notes = Some(text.to_owned());
                    NodeResult::Continue(state)
                },
            )
            .with_local_tools(tools::coder_tools(), ToolPolicy::coder(), hooks.clone()),
        )
        .add_node(browser_node)
        .add_node(github_node)
        .add_node(security_node)
        .add_node(LlmNode::new(
            AgentRole::Critic.as_str(),
            prompts::CRITIC_SYSTEM,
            critic_llm,
            |text, mut state| {
                state.critique = Some(text.to_owned());
                NodeResult::Continue(state)
            },
        ))
        .add_node(LlmNode::new(
            "finalize",
            prompts::FINALIZE_SYSTEM,
            finalize_llm,
            |text, mut state| {
                state.final_answer = Some(text.to_owned());
                NodeResult::Finish(state)
            },
        ))
        .set_entry(AgentRole::Router.as_str())
        .add_conditional_edge(AgentRole::Router.as_str(), |s: &FleetState| match s.route {
            FleetRoute::Explorer => Some("workers_explorer".into()),
            FleetRoute::Coder => Some("workers_coder".into()),
            FleetRoute::Both => Some("workers_both".into()),
            FleetRoute::Browser => Some("workers_browser".into()),
            FleetRoute::GitHub => Some("workers_github".into()),
            FleetRoute::Security => Some("workers_security".into()),
        })
        .add_node(DispatchNode::explorer_only())
        .add_node(DispatchNode::coder_only())
        .add_node(DispatchNode::both())
        .add_node(DispatchNode::browser_only())
        .add_node(DispatchNode::github_only())
        .add_node(DispatchNode::security_only())
        .add_edge("workers_explorer", AgentRole::Explorer.as_str())
        .add_edge("workers_coder", AgentRole::Coder.as_str())
        .add_edge("workers_browser", AgentRole::Browser.as_str())
        .add_edge("workers_github", AgentRole::GitHub.as_str())
        .add_edge("workers_security", AgentRole::Security.as_str())
        .add_fanout_join(
            "workers_both",
            [AgentRole::Explorer.as_str(), AgentRole::Coder.as_str()],
            AgentRole::Critic.as_str(),
            merge_worker_states,
        )
        .add_edge(AgentRole::Explorer.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::Coder.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::Browser.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::GitHub.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::Security.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::Critic.as_str(), "finalize")
        .build()
        .map_err(AgentError::from)?;

    Ok(graph)
}

fn parse_route_from_router(text: &str) -> FleetRoute {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("ROUTE:")
            .or_else(|| trimmed.strip_prefix("Route:"))
        {
            return FleetRoute::parse(rest.trim());
        }
    }
    FleetRoute::parse(text)
}

fn merge_worker_states(branches: Vec<FleetState>) -> FleetState {
    let mut out = branches.first().cloned().unwrap_or_default();
    for b in branches {
        if b.exploration.is_some() {
            out.exploration = b.exploration;
        }
        if b.code_notes.is_some() {
            out.code_notes = b.code_notes;
        }
        if b.browser_notes.is_some() {
            out.browser_notes = b.browser_notes;
        }
        if b.github_notes.is_some() {
            out.github_notes = b.github_notes;
        }
        if b.security_notes.is_some() {
            out.security_notes = b.security_notes;
        }
        for t in b.trace {
            if !out.trace.contains(&t) {
                out.trace.push(t);
            }
        }
    }
    out
}

/// Tiny non-LLM node that immediately continues (dispatch hop).
struct DispatchNode {
    name: &'static str,
}

impl DispatchNode {
    fn explorer_only() -> Self {
        Self {
            name: "workers_explorer",
        }
    }
    fn coder_only() -> Self {
        Self {
            name: "workers_coder",
        }
    }
    fn both() -> Self {
        Self {
            name: "workers_both",
        }
    }
    fn browser_only() -> Self {
        Self {
            name: "workers_browser",
        }
    }
    fn github_only() -> Self {
        Self {
            name: "workers_github",
        }
    }
    fn security_only() -> Self {
        Self {
            name: "workers_security",
        }
    }
}

#[async_trait]
impl Node<FleetState> for DispatchNode {
    fn name(&self) -> &str {
        self.name
    }

    async fn run(
        &self,
        mut state: FleetState,
    ) -> altius_graph::GraphResult<NodeResult<FleetState>> {
        state.trace.push(self.name.to_owned());
        Ok(NodeResult::Continue(state))
    }
}

/// How one supervisor pass ended.
#[derive(Clone, Debug)]
pub enum SupervisorOutcome {
    /// The graph ran to completion.
    Finished(FleetState),
    /// The graph paused on a human-in-the-loop interrupt; callers decide
    /// whether to surface this as an awaiting run or an error.
    Awaiting {
        reason: String,
        node: String,
        state: FleetState,
    },
}

fn default_budget() -> Budget {
    Budget::unlimited().with_max_steps(32).with_max_parallel(4)
}

/// Run the supervisor and surface HITL interrupts as
/// [`SupervisorOutcome::Awaiting`] instead of an error.
pub async fn run_supervisor_outcome_with(
    llm: Arc<dyn LlmClient>,
    checkpointer: Arc<dyn Checkpointer<FleetState>>,
    budget: Budget,
    prompt: impl Into<String>,
) -> AgentResult<(RunId, SupervisorOutcome)> {
    run_supervisor_outcome_with_options(
        llm,
        checkpointer,
        budget,
        prompt,
        SupervisorOptions::default(),
    )
    .await
}

/// Like [`run_supervisor_outcome_with`] but accepts agent-name / browser options.
pub async fn run_supervisor_outcome_with_options(
    llm: Arc<dyn LlmClient>,
    checkpointer: Arc<dyn Checkpointer<FleetState>>,
    budget: Budget,
    prompt: impl Into<String>,
    options: SupervisorOptions,
) -> AgentResult<(RunId, SupervisorOutcome)> {
    let graph = Arc::new(build_supervisor_graph_with(llm, &options)?);
    let executor = GraphExecutor::new(graph, checkpointer, budget);
    let run_id = RunId::new();
    let initial = FleetState::new(prompt).with_options(&options);

    let outcome = match executor.run(run_id, initial).await? {
        ExecutionOutcome::Finished { state, .. } => SupervisorOutcome::Finished(state),
        ExecutionOutcome::Interrupted {
            reason,
            node,
            state,
            ..
        } => SupervisorOutcome::Awaiting {
            reason,
            node,
            state,
        },
    };
    Ok((run_id, outcome))
}

/// [`run_supervisor_outcome_with`] with an in-memory checkpointer and the
/// default budget.
pub async fn run_supervisor_outcome(
    llm: Arc<dyn LlmClient>,
    prompt: impl Into<String>,
) -> AgentResult<(RunId, SupervisorOutcome)> {
    run_supervisor_outcome_with(
        llm,
        Arc::new(InMemoryCheckpointer::<FleetState>::new()),
        default_budget(),
        prompt,
    )
    .await
}

/// In-memory checkpointer + options (agent name / browser tooling).
pub async fn run_supervisor_outcome_for(
    llm: Arc<dyn LlmClient>,
    prompt: impl Into<String>,
    options: SupervisorOptions,
) -> AgentResult<(RunId, SupervisorOutcome)> {
    run_supervisor_outcome_with_options(
        llm,
        Arc::new(InMemoryCheckpointer::<FleetState>::new()),
        default_budget(),
        prompt,
        options,
    )
    .await
}

/// Run the supervisor with a caller-supplied LLM and checkpointer.
///
/// A HITL interrupt is an error here; use [`run_supervisor_outcome_with`]
/// when the caller can pause and resume.
pub async fn run_supervisor_with(
    llm: Arc<dyn LlmClient>,
    checkpointer: Arc<dyn Checkpointer<FleetState>>,
    budget: Budget,
    prompt: impl Into<String>,
) -> AgentResult<(RunId, FleetState)> {
    match run_supervisor_outcome_with(llm, checkpointer, budget, prompt).await? {
        (run_id, SupervisorOutcome::Finished(state)) => Ok((run_id, state)),
        (_, SupervisorOutcome::Awaiting { reason, .. }) => Err(AgentError::message(format!(
            "supervisor interrupted (HITL): {reason}"
        ))),
    }
}

/// Headless helper used by the CLI: in-memory checkpointer + provided LLM.
pub async fn run_supervisor(
    llm: Arc<dyn LlmClient>,
    prompt: impl Into<String>,
) -> AgentResult<(RunId, FleetState)> {
    run_supervisor_with(
        llm,
        Arc::new(InMemoryCheckpointer::<FleetState>::new()),
        default_budget(),
        prompt,
    )
    .await
}

/// Convenience for tests / `--offline`.
pub async fn run_supervisor_offline(prompt: impl Into<String>) -> AgentResult<(RunId, FleetState)> {
    run_supervisor(Arc::new(OfflineLlmClient), prompt).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn supervisor_golden_path_offline() {
        let (run_id, state) = run_supervisor_offline("please find the detect module")
            .await
            .unwrap();
        assert!(!run_id.to_string().is_empty());
        assert!(state.plan.is_some());
        assert!(state.exploration.is_some());
        assert!(state.critique.is_some());
        assert!(state.final_answer.is_some());
        assert!(state.trace.iter().any(|t| t == "router"));
        assert!(state.trace.iter().any(|t| t == "critic"));
        assert!(state.trace.iter().any(|t| t == "finalize"));
        // "find" routes to explorer-only
        assert_eq!(state.route, FleetRoute::Explorer);
        assert!(state.code_notes.is_none());
    }

    #[tokio::test]
    async fn supervisor_both_route_fanout() {
        let (_run_id, state) = run_supervisor_offline("summarize the workspace briefly")
            .await
            .unwrap();
        assert_eq!(state.route, FleetRoute::Both);
        assert!(state.exploration.is_some());
        assert!(state.code_notes.is_some());
        assert!(state.final_answer.unwrap().contains("FINAL"));
    }

    #[tokio::test]
    async fn outcome_entry_point_finishes_offline() {
        let (_run_id, outcome) =
            run_supervisor_outcome(Arc::new(OfflineLlmClient), "find the detect module")
                .await
                .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => assert!(state.final_answer.is_some()),
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline run should not await: {reason}")
            }
        }
    }

    #[tokio::test]
    async fn agent_name_browser_forces_browser_route_offline() {
        let (_run_id, outcome) = run_supervisor_outcome_for(
            Arc::new(OfflineLlmClient),
            "open https://example.com and summarize the title",
            SupervisorOptions {
                agent_name: Some("browser".into()),
                ..SupervisorOptions::default()
            },
        )
        .await
        .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => {
                assert_eq!(state.route, FleetRoute::Browser);
                assert_eq!(state.forced_route, Some(FleetRoute::Browser));
                assert!(state.browser_notes.is_some());
                assert!(state.trace.iter().any(|t| t == "browser"));
                assert!(state.exploration.is_none());
            }
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline browser run should not await: {reason}")
            }
        }
    }

    #[tokio::test]
    async fn at_browser_mention_forces_browser_route_offline() {
        let (_run_id, outcome) = run_supervisor_outcome_for(
            Arc::new(OfflineLlmClient),
            "@Browser check https://example.com",
            SupervisorOptions::default(),
        )
        .await
        .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => {
                assert_eq!(state.route, FleetRoute::Browser);
                assert!(state.browser_notes.is_some());
            }
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline @Browser run should not await: {reason}")
            }
        }
    }

    #[test]
    fn route_parser() {
        assert_eq!(FleetRoute::parse("explorer"), FleetRoute::Explorer);
        assert_eq!(FleetRoute::parse("coder"), FleetRoute::Coder);
        assert_eq!(FleetRoute::parse("both"), FleetRoute::Both);
        assert_eq!(FleetRoute::parse("browser"), FleetRoute::Browser);
        assert_eq!(FleetRoute::parse("github"), FleetRoute::GitHub);
        assert_eq!(FleetRoute::parse("security"), FleetRoute::Security);
    }

    #[test]
    fn resolve_forced_route_from_agent_name_and_mention() {
        assert_eq!(
            resolve_forced_route(Some("browser"), "anything"),
            Some(FleetRoute::Browser)
        );
        assert_eq!(
            resolve_forced_route(Some("@Browser"), "anything"),
            Some(FleetRoute::Browser)
        );
        assert_eq!(
            resolve_forced_route(None, "please @Browser this"),
            Some(FleetRoute::Browser)
        );
        assert_eq!(
            resolve_forced_route(Some("github"), "anything"),
            Some(FleetRoute::GitHub)
        );
        assert_eq!(
            resolve_forced_route(None, "please @GitHub inspect this"),
            Some(FleetRoute::GitHub)
        );
        assert_eq!(
            resolve_forced_route(Some("security"), "anything"),
            Some(FleetRoute::Security)
        );
        assert_eq!(
            resolve_forced_route(None, "@Security audit this program"),
            Some(FleetRoute::Security)
        );
        assert_eq!(resolve_forced_route(Some("altius"), "find module"), None);
    }

    #[tokio::test]
    async fn agent_name_security_forces_security_route_offline() {
        let (_run_id, outcome) = run_supervisor_outcome_for(
            Arc::new(OfflineLlmClient),
            "audit this Anchor project for missing signer checks",
            SupervisorOptions {
                agent_name: Some("security".into()),
                ..SupervisorOptions::default()
            },
        )
        .await
        .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => {
                assert_eq!(state.route, FleetRoute::Security);
                assert_eq!(state.forced_route, Some(FleetRoute::Security));
                assert!(state.security_notes.is_some());
                assert!(state.trace.iter().any(|t| t == "security"));
                assert!(state.exploration.is_none());
            }
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline security run should not await: {reason}")
            }
        }
    }

    #[tokio::test]
    async fn agent_name_github_forces_github_route_offline() {
        let (_run_id, outcome) = run_supervisor_outcome_for(
            Arc::new(OfflineLlmClient),
            "inspect open pull requests",
            SupervisorOptions {
                agent_name: Some("github".into()),
                ..SupervisorOptions::default()
            },
        )
        .await
        .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => {
                assert_eq!(state.route, FleetRoute::GitHub);
                assert_eq!(state.forced_route, Some(FleetRoute::GitHub));
                assert!(state.github_notes.is_some());
                assert!(state.trace.iter().any(|t| t == "github"));
            }
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline GitHub run should not await: {reason}")
            }
        }
    }

    #[tokio::test]
    async fn at_security_mention_forces_security_route_offline() {
        let (_run_id, outcome) = run_supervisor_outcome_for(
            Arc::new(OfflineLlmClient),
            "@Security scan for arbitrary CPI",
            SupervisorOptions::default(),
        )
        .await
        .unwrap();
        match outcome {
            SupervisorOutcome::Finished(state) => {
                assert_eq!(state.route, FleetRoute::Security);
                assert!(state.security_notes.is_some());
            }
            SupervisorOutcome::Awaiting { reason, .. } => {
                panic!("offline @Security run should not await: {reason}")
            }
        }
    }
}
