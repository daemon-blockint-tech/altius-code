//! Altius fleet agents: provider-neutral LLM client, role prompts, supervisor graph.
//!
//! Prompts are Altius-authored. Do not copy third-party leaked system prompts.

mod error;
mod llm;
mod prompts;
mod roles;
mod supervisor;
mod tools;

pub use error::{AgentError, AgentResult};
pub use llm::{
    ChatMessage, LlmClient, OfflineLlmClient, OpenAiCompatibleClient, Role, ToolCall, ToolSpec,
};
pub use roles::{stub_roles, AgentRole};
pub use supervisor::{
    build_supervisor_graph, build_supervisor_graph_with, resolve_forced_route, run_supervisor,
    run_supervisor_offline, run_supervisor_outcome, run_supervisor_outcome_for,
    run_supervisor_outcome_with, run_supervisor_outcome_with_options, run_supervisor_with,
    BrowserTooling, FleetRoute, FleetState, SupervisorOptions, SupervisorOutcome,
};
pub use tools::{
    tool_specs_from_discovered, LocalTools, McpTools, ToolDispatcher, BROWSER_TOOL_PREFIX,
};
