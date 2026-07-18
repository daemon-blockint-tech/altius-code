//! Multi-agent fleet for Altius Code, built on the `rust-langgraph`
//! graph runtime (Pregel-style state graph with prebuilt ReAct agents).
//!
//! This implements Phase A of the fleet plan: a supervisor pipeline of
//! specialist LLM agents — `explorer` → `coder` → `security` → `release`
//! — where each specialist is a ReAct agent (LLM + tools) and the
//! pipeline itself is a `StateGraph` with conditional routing (a failed
//! build short-circuits straight to the final report).
//!
//! ## Security invariants (non-negotiable, from the fleet plan)
//!
//! - **No signer access.** The fleet's tool plane wraps detect / build /
//!   test / lint / plan-deploy only. There is no tool that connects to
//!   `altius-signerd`, holds key material, or broadcasts a transaction —
//!   deploying for real remains `altius deploy`, which goes through
//!   `TxGuard`'s policy → simulate → diff → approve → audit pipeline
//!   with a human in the loop.
//! - **Plan, don't submit.** The `plan_deploy` tool builds a
//!   `DeploymentPlan` preview with a throwaway payer and placeholder
//!   blockhash; its output is a description of what *would* happen.
//! - **Tool results are data.** Specialist reports summarize tool
//!   output; nothing a model says can widen the tool plane.

mod agents;
mod error;
mod fleet;
mod state;
pub mod testing;
mod tools;

pub use agents::{system_prompt, Role};
pub use error::FleetError;
pub use fleet::{run_fleet, FleetConfig, FleetReport};
pub use state::{AgentReport, FleetState};
pub use tools::{tools_for_role, FAILURE_MARKER};

// Re-exported so callers (CLI, tests) can name model types without
// depending on rust-langgraph directly.
pub use rust_langgraph::llm::openrouter::OpenRouterAdapter;
pub use rust_langgraph::llm::{ChatModel, ToolInfo};
