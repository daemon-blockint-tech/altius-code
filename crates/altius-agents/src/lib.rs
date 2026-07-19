//! Altius fleet agents: provider-neutral LLM client, role prompts, supervisor graph.
//!
//! Prompts are Altius-authored. Do not copy third-party leaked system prompts.

mod error;
mod evidence;
mod fs_tools;
mod guardrails;
mod hooks;
mod learning;
mod llm;
mod permissions;
mod project_memory;
mod prompts;
mod roles;
mod routing;
mod skills;
mod supervisor;
mod tools;

pub use error::{AgentError, AgentResult};
pub use evidence::{ground_final_answer, EvidenceEntry, EvidenceStatus};
pub use guardrails::{
    default_guardrail_hooks, GuardrailPolicy, GuardrailsPipeline, IndirectInjectionHook,
    RailDecision, DEFAULT_MAX_OUTPUT_CHARS,
};
pub use hooks::{HookEvent, HookOutcome, HookedDispatcher, ToolHook};
pub use learning::{LearningKind, LearningMemory, LearningRecord};
pub use llm::{
    llm_from_env, ChatMessage, InferencePolicy, LlmClient, ModelCapability, OfflineLlmClient,
    OpenAiCompatibleClient, PolicyLlmClient, ProviderCandidate, Role, TaskClass, ToolCall,
    ToolSpec,
};
pub use permissions::{PermissionedDispatcher, ToolDecision, ToolPolicy};
pub use project_memory::{format_for_system as format_project_memory, load as load_project_memory};
pub use roles::{stub_roles, AgentRole};
pub use routing::{classify_route, RiskLevel, RouteDecision, TaskIntent};
pub use skills::{agent_name_for_route, known_skills, parse_slash_skill, SlashSkill};
pub use supervisor::{
    build_supervisor_graph, build_supervisor_graph_with, resolve_forced_route, run_supervisor,
    run_supervisor_offline, run_supervisor_outcome, run_supervisor_outcome_for,
    run_supervisor_outcome_with, run_supervisor_outcome_with_options, run_supervisor_with,
    BrowserTooling, FleetRoute, FleetState, GitHubTooling, SupervisorOptions, SupervisorOutcome,
};
pub use tools::{
    project_root_from_prompt, tool_specs_from_discovered, GitHubAccess, LocalTools, McpTools,
    ToolDispatcher, BROWSER_TOOL_PREFIX,
};
