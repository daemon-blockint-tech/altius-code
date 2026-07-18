//! Altius fleet agents: provider-neutral LLM client, role prompts, supervisor graph.
//!
//! Prompts are Altius-authored. Do not copy third-party leaked system prompts.

mod error;
mod fs_tools;
mod hooks;
mod llm;
mod permissions;
mod project_memory;
mod prompts;
mod roles;
mod supervisor;
mod tools;

pub use error::{AgentError, AgentResult};
pub use hooks::{HookEvent, HookOutcome, HookedDispatcher, ToolHook};
pub use llm::{
    ChatMessage, LlmClient, OfflineLlmClient, OpenAiCompatibleClient, Role, ToolCall, ToolSpec,
};
pub use permissions::{PermissionedDispatcher, ToolDecision, ToolPolicy};
pub use project_memory::{format_for_system as format_project_memory, load as load_project_memory};
pub use roles::{stub_roles, AgentRole};
pub use supervisor::{
    build_supervisor_graph, build_supervisor_graph_with, resolve_forced_route, run_supervisor,
    run_supervisor_offline, run_supervisor_outcome, run_supervisor_outcome_for,
    run_supervisor_outcome_with, run_supervisor_outcome_with_options, run_supervisor_with,
    BrowserTooling, FleetRoute, FleetState, SupervisorOptions, SupervisorOutcome,
};
pub use tools::{
    project_root_from_prompt, tool_specs_from_discovered, LocalTools, McpTools, ToolDispatcher,
    BROWSER_TOOL_PREFIX,
};
