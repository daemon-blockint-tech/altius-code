//! Altius fleet agents: provider-neutral LLM client, role prompts, supervisor graph.
//!
//! Prompts are Altius-authored. Do not copy third-party leaked system prompts.

mod error;
mod llm;
mod prompts;
mod roles;
mod supervisor;

pub use error::{AgentError, AgentResult};
pub use llm::{
    ChatMessage, LlmClient, OfflineLlmClient, OpenAiCompatibleClient, Role, ToolCall, ToolSpec,
};
pub use roles::{stub_roles, AgentRole};
pub use supervisor::{
    build_supervisor_graph, run_supervisor, run_supervisor_offline, run_supervisor_with,
    FleetRoute, FleetState,
};
