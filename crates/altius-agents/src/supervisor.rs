use std::sync::Arc;

use altius_core::{Budget, CorrelationId, RunId};
use altius_graph::{
    Checkpointer, ExecutionOutcome, Graph, GraphBuilder, GraphExecutor, InMemoryCheckpointer, Node,
    NodeResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};
use crate::llm::{ChatMessage, LlmClient, OfflineLlmClient};
use crate::prompts;
use crate::roles::AgentRole;

/// Which specialist path the router selected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FleetRoute {
    Explorer,
    Coder,
    #[default]
    Both,
}

impl FleetRoute {
    pub fn parse(raw: &str) -> Self {
        let lower = raw.to_ascii_lowercase();
        if lower.contains("explorer") && !lower.contains("coder") && !lower.contains("both") {
            Self::Explorer
        } else if lower.contains("coder") && !lower.contains("explorer") && !lower.contains("both")
        {
            Self::Coder
        } else {
            Self::Both
        }
    }
}

/// Shared supervisor graph state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FleetState {
    pub prompt: String,
    pub plan: Option<String>,
    pub route: FleetRoute,
    pub exploration: Option<String>,
    pub code_notes: Option<String>,
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
}

struct LlmNode {
    name: &'static str,
    system: &'static str,
    llm: Arc<dyn LlmClient>,
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
            after: Box::new(after),
        }
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
        let user_blob = format!(
            "User prompt:\n{}\n\nPlan:\n{}\n\nExploration:\n{}\n\nCode notes:\n{}\n\nCritique:\n{}",
            state.prompt,
            state.plan.as_deref().unwrap_or("(none)"),
            state.exploration.as_deref().unwrap_or("(none)"),
            state.code_notes.as_deref().unwrap_or("(none)"),
            state.critique.as_deref().unwrap_or("(none)"),
        );
        let text = self
            .llm
            .complete(&[
                ChatMessage::system(self.system),
                ChatMessage::user(user_blob),
            ])
            .await
            .map_err(|e| altius_graph::GraphError::node_failed(self.name, e.to_string()))?;
        Ok((self.after)(&text, state))
    }
}

/// Build the Phase A supervisor graph: router → explorer/coder → critic → finalize.
pub fn build_supervisor_graph(llm: Arc<dyn LlmClient>) -> AgentResult<Graph<FleetState>> {
    let router_llm = Arc::clone(&llm);
    let explorer_llm = Arc::clone(&llm);
    let coder_llm = Arc::clone(&llm);
    let critic_llm = Arc::clone(&llm);
    let finalize_llm = Arc::clone(&llm);

    let graph = GraphBuilder::new()
        .add_node(LlmNode::new(
            AgentRole::Router.as_str(),
            prompts::ROUTER_SYSTEM,
            router_llm,
            |text, mut state| {
                state.plan = Some(text.to_owned());
                state.route = parse_route_from_router(text);
                NodeResult::Continue(state)
            },
        ))
        .add_node(LlmNode::new(
            AgentRole::Explorer.as_str(),
            prompts::EXPLORER_SYSTEM,
            explorer_llm,
            |text, mut state| {
                state.exploration = Some(text.to_owned());
                NodeResult::Continue(state)
            },
        ))
        .add_node(LlmNode::new(
            AgentRole::Coder.as_str(),
            prompts::CODER_SYSTEM,
            coder_llm,
            |text, mut state| {
                state.code_notes = Some(text.to_owned());
                NodeResult::Continue(state)
            },
        ))
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
        .add_conditional_edge(AgentRole::Router.as_str(), |s: &FleetState| {
            // Always enter the workers stage via a synthetic fan-out handled below.
            // For explorer-only / coder-only we still use fan-out with one target
            // by routing through dedicated edges.
            match s.route {
                FleetRoute::Explorer => Some("workers_explorer".into()),
                FleetRoute::Coder => Some("workers_coder".into()),
                FleetRoute::Both => Some("workers_both".into()),
            }
        })
        // Internal dispatch nodes (no LLM) selected by the conditional edge.
        .add_node(DispatchNode::explorer_only())
        .add_node(DispatchNode::coder_only())
        .add_node(DispatchNode::both())
        .add_edge("workers_explorer", AgentRole::Explorer.as_str())
        .add_edge("workers_coder", AgentRole::Coder.as_str())
        .add_fanout_join(
            "workers_both",
            [AgentRole::Explorer.as_str(), AgentRole::Coder.as_str()],
            AgentRole::Critic.as_str(),
            merge_worker_states,
        )
        .add_edge(AgentRole::Explorer.as_str(), AgentRole::Critic.as_str())
        .add_edge(AgentRole::Coder.as_str(), AgentRole::Critic.as_str())
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

/// Run the supervisor with a caller-supplied LLM and checkpointer.
pub async fn run_supervisor_with(
    llm: Arc<dyn LlmClient>,
    checkpointer: Arc<dyn Checkpointer<FleetState>>,
    budget: Budget,
    prompt: impl Into<String>,
) -> AgentResult<(RunId, FleetState)> {
    let graph = Arc::new(build_supervisor_graph(llm)?);
    let executor = GraphExecutor::new(graph, checkpointer, budget);
    let run_id = RunId::new();
    let initial = FleetState::new(prompt);

    match executor.run(run_id, initial).await? {
        ExecutionOutcome::Finished { state, .. } => Ok((run_id, state)),
        ExecutionOutcome::Interrupted { reason, .. } => Err(AgentError::message(format!(
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
        Budget::unlimited().with_max_steps(32).with_max_parallel(4),
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

    #[test]
    fn route_parser() {
        assert_eq!(FleetRoute::parse("explorer"), FleetRoute::Explorer);
        assert_eq!(FleetRoute::parse("coder"), FleetRoute::Coder);
        assert_eq!(FleetRoute::parse("both"), FleetRoute::Both);
    }
}
