use std::path::PathBuf;
use std::sync::Arc;

use rust_langgraph::config::Config;
use rust_langgraph::errors::Error as GraphError;
use rust_langgraph::graph::StateGraph;
use rust_langgraph::llm::ChatModel;
use rust_langgraph::nodes::Node as _;
use rust_langgraph::prebuilt::ToolNode;
use rust_langgraph::state::{Message, MessagesState};

use crate::agents::{system_prompt, Role};
use crate::error::FleetError;
use crate::state::{AgentReport, FleetState};
use crate::tools::{tools_for_role, FAILURE_MARKER};

/// Configuration for one fleet run.
#[derive(Debug, Clone)]
pub struct FleetConfig {
    /// What the human asked the fleet to accomplish.
    pub goal: String,
    /// Root of the SVM project the fleet works on.
    pub project: PathBuf,
    /// Per-specialist ReAct step budget (one step = one model call,
    /// optionally followed by tool execution) — the "max steps" budget
    /// from the fleet plan. Small on purpose.
    pub max_steps: usize,
}

impl FleetConfig {
    pub fn new(goal: impl Into<String>, project: impl Into<PathBuf>) -> FleetConfig {
        FleetConfig {
            goal: goal.into(),
            project: project.into(),
            max_steps: 12,
        }
    }
}

/// The outcome of a fleet run: each specialist's report, in pipeline
/// order, plus whether any stage's tools reported failure (in which
/// case later stages were skipped).
#[derive(Debug, Clone)]
pub struct FleetReport {
    pub reports: Vec<AgentReport>,
    pub failed: bool,
}

/// Runs the supervisor pipeline explorer → coder → security → release
/// as a `rust-langgraph` `StateGraph`, short-circuiting the moment a
/// stage's tools report failure. `model_for` supplies the LLM for each
/// role, already bound to the role's tool schemas (for OpenRouter that
/// means `OpenRouterAdapter::bind_tools`; test doubles may ignore the
/// infos).
///
/// Two upstream limitations of `rust-langgraph` v0.1.1 shape this
/// implementation, both verified against its source:
///
/// - `StateGraph::compile` drops `add_conditional_edges` routing on the
///   floor (the branches never reach the Pregel engine), so the
///   failure gate lives *inside* each node: once `FleetState::failed`
///   is latched, later nodes pass the state through untouched.
/// - Node outputs replace, rather than merge into, the flowing state
///   (`LastValue` channels), so each node returns the full accumulated
///   state — input plus its own report — not a partial update.
pub async fn run_fleet<M, F>(config: FleetConfig, model_for: F) -> Result<FleetReport, FleetError>
where
    M: ChatModel + 'static,
    F: Fn(Role, &[rust_langgraph::llm::ToolInfo]) -> M + Send + Sync + 'static,
{
    let model_for = Arc::new(model_for);
    let max_steps = config.max_steps;
    let mut graph: StateGraph<FleetState> = StateGraph::new();

    for role in Role::PIPELINE {
        let model_for = Arc::clone(&model_for);
        graph.add_node(role.name(), move |state: FleetState, _config: &Config| {
            let model_for = Arc::clone(&model_for);
            async move {
                // The in-node failure gate (see the function docs).
                if state.failed {
                    return Ok(state);
                }
                run_specialist(role, state, max_steps, model_for.as_ref()).await
            }
        });
    }

    graph.set_entry_point(Role::Explorer.name());
    for pair in Role::PIPELINE.windows(2) {
        graph.add_edge(pair[0].name(), pair[1].name());
    }
    graph.set_finish_point(Role::Release.name());

    let mut app = graph.compile(None)?;
    let initial = FleetState {
        goal: config.goal.clone(),
        project: config.project.display().to_string(),
        reports: vec![],
        failed: false,
    };
    let final_state = app.invoke(initial, Config::default()).await?;

    Ok(FleetReport {
        failed: final_state.failed,
        reports: final_state.reports,
    })
}

/// Runs one specialist as a ReAct loop (model ⇄ tools) and appends its
/// distilled report to the accumulated state.
///
/// The loop is written out by hand instead of using the crate's
/// `create_react_agent`, because that prebuilt relies on the
/// conditional-edge routing `compile` currently drops (see
/// [`run_fleet`]) — through the prebuilt graph, tools never actually
/// execute. The hand-rolled loop also keeps the full transcript in
/// every model call, which the prebuilt's replace-semantics state flow
/// would lose.
async fn run_specialist<M, F>(
    role: Role,
    mut state: FleetState,
    max_steps: usize,
    model_for: &F,
) -> rust_langgraph::errors::Result<FleetState>
where
    M: ChatModel + 'static,
    F: Fn(Role, &[rust_langgraph::llm::ToolInfo]) -> M,
{
    let project = PathBuf::from(&state.project);
    let tools = tools_for_role(role, &project);
    let tool_infos: Vec<_> = tools.iter().map(|t| t.to_tool_info()).collect();
    let model = model_for(role, &tool_infos);

    let seed = vec![
        Message::system(system_prompt(role)),
        Message::user(format!(
            "Project root: {}\nGoal: {}",
            state.project, state.goal
        )),
    ];
    let transcript = react_loop(&model, tools, seed, max_steps)
        .await
        .map_err(|e| GraphError::ExecutionError(format!("{role} specialist failed: {e}")))?;

    let report = distill(role, &transcript);
    state.failed |= report.tool_failure;
    state.reports.push(report);
    Ok(state)
}

/// The ReAct loop: call the model; if it requested tool calls, execute
/// them via the crate's `ToolNode` and go around again with the tool
/// results appended; otherwise its reply is final. Budgeted by
/// `max_steps` model calls.
async fn react_loop<M: ChatModel>(
    model: &M,
    tools: Vec<rust_langgraph::prebuilt::Tool>,
    mut messages: Vec<Message>,
    max_steps: usize,
) -> rust_langgraph::errors::Result<MessagesState> {
    let tool_node = ToolNode::new(tools);
    for _ in 0..max_steps {
        let response = model.invoke(&messages).await?;
        let has_tool_calls = response
            .tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty());
        messages.push(response);

        if !has_tool_calls {
            return Ok(MessagesState { messages });
        }
        let tool_results = tool_node
            .invoke(
                MessagesState {
                    messages: messages.clone(),
                },
                &Config::default(),
            )
            .await?;
        messages.extend(tool_results.messages);
    }
    Err(GraphError::ExecutionError(format!(
        "specialist exceeded its step budget of {max_steps} model calls"
    )))
}

/// Turns a specialist's finished transcript into its report: the last
/// non-empty assistant message is the summary, and any tool message
/// carrying [`FAILURE_MARKER`] marks the stage as failed.
fn distill(role: Role, transcript: &MessagesState) -> AgentReport {
    let summary = transcript
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.is_empty())
        .map(|m| m.content.clone())
        .unwrap_or_else(|| format!("{role} produced no summary"));
    let tool_failure = transcript
        .messages
        .iter()
        .any(|m| m.role == "tool" && m.content.contains(FAILURE_MARKER));

    AgentReport {
        role: role.name().to_string(),
        summary,
        tool_failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedModel;
    use rust_langgraph::state::ToolCall;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;

    fn anchor_fixture_with_artifact() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Anchor.toml"),
            "[programs.localnet]\nmy_program = \"Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS\"\n",
        )
        .unwrap();
        let deploy_dir = dir.path().join("target").join("deploy");
        fs::create_dir_all(&deploy_dir).unwrap();
        fs::write(deploy_dir.join("my_program.so"), vec![0u8; 64]).unwrap();
        dir
    }

    fn scripted(
        map: HashMap<Role, ScriptedModel>,
    ) -> impl Fn(Role, &[rust_langgraph::llm::ToolInfo]) -> ScriptedModel {
        move |role, _infos| {
            map.get(&role)
                .unwrap_or_else(|| panic!("no script for {role}; this stage should not have run"))
                .clone()
        }
    }

    #[tokio::test]
    async fn happy_path_runs_all_four_specialists_in_order() {
        let dir = anchor_fixture_with_artifact();
        let mut scripts = HashMap::new();
        scripts.insert(
            Role::Explorer,
            ScriptedModel::new(
                "explorer",
                vec![
                    Message::assistant("").with_tool_calls(vec![ToolCall::new(
                        "1",
                        "detect_project",
                        json!({}),
                    )]),
                    Message::assistant("Anchor project with one program."),
                ],
            ),
        );
        scripts.insert(
            Role::Coder,
            ScriptedModel::new(
                "coder",
                vec![Message::assistant("Build not attempted in test.")],
            ),
        );
        scripts.insert(
            Role::Security,
            ScriptedModel::new(
                "security",
                vec![Message::assistant("No blocking findings.")],
            ),
        );
        scripts.insert(
            Role::Release,
            ScriptedModel::new(
                "release",
                vec![
                    Message::assistant("").with_tool_calls(vec![ToolCall::new(
                        "1",
                        "plan_deploy",
                        json!({"cluster": "devnet"}),
                    )]),
                    Message::assistant("Plan: create buffer, one write, deploy."),
                ],
            ),
        );

        let report = run_fleet(
            FleetConfig::new("audit and preview deploy", dir.path()),
            scripted(scripts),
        )
        .await
        .unwrap();

        assert!(!report.failed);
        let roles: Vec<_> = report.reports.iter().map(|r| r.role.as_str()).collect();
        assert_eq!(roles, vec!["explorer", "coder", "security", "release"]);
        assert!(report.reports[3].summary.contains("Plan"));
    }

    #[tokio::test]
    async fn a_failed_build_short_circuits_the_pipeline() {
        // No anchor CLI exists in this sandbox, so build_program's tool
        // output carries the failure marker for real.
        let dir = anchor_fixture_with_artifact();
        let mut scripts = HashMap::new();
        scripts.insert(
            Role::Explorer,
            ScriptedModel::new("explorer", vec![Message::assistant("Anchor project.")]),
        );
        scripts.insert(
            Role::Coder,
            ScriptedModel::new(
                "coder",
                vec![
                    Message::assistant("").with_tool_calls(vec![ToolCall::new(
                        "1",
                        "build_program",
                        json!({}),
                    )]),
                    Message::assistant("Build failed: anchor CLI is not installed."),
                ],
            ),
        );
        // Deliberately no scripts for security/release: if the gate
        // failed to skip them, the factory panics and the test fails.

        let report = run_fleet(FleetConfig::new("build it", dir.path()), scripted(scripts))
            .await
            .unwrap();

        assert!(report.failed);
        let roles: Vec<_> = report.reports.iter().map(|r| r.role.as_str()).collect();
        assert_eq!(roles, vec!["explorer", "coder"]);
        assert!(report.reports[1].tool_failure);
    }

    #[tokio::test]
    async fn react_loop_feeds_tool_results_back_and_respects_the_budget() {
        let dir = anchor_fixture_with_artifact();
        let model = ScriptedModel::new(
            "m",
            vec![
                Message::assistant("").with_tool_calls(vec![ToolCall::new(
                    "1",
                    "detect_project",
                    json!({}),
                )]),
                Message::assistant("done"),
            ],
        );
        let transcript = react_loop(
            &model,
            tools_for_role(Role::Explorer, dir.path()),
            vec![Message::user("go")],
            3,
        )
        .await
        .unwrap();

        // user, assistant(tool_calls), tool result, assistant(final)
        assert_eq!(transcript.messages.len(), 4);
        assert_eq!(transcript.messages[2].role, "tool");
        assert!(transcript.messages[2]
            .content
            .contains("\"framework\":\"Anchor\""));
        assert_eq!(model.remaining(), 0);

        // A model that never stops calling tools must hit the budget.
        let looping = ScriptedModel::new(
            "loop",
            (0..5)
                .map(|i| {
                    Message::assistant("").with_tool_calls(vec![ToolCall::new(
                        i.to_string(),
                        "detect_project",
                        json!({}),
                    )])
                })
                .collect(),
        );
        let err = react_loop(
            &looping,
            tools_for_role(Role::Explorer, dir.path()),
            vec![Message::user("go")],
            3,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("step budget"));
    }

    #[test]
    fn distill_reads_the_last_nonempty_assistant_message() {
        let transcript = MessagesState {
            messages: vec![
                Message::system("s"),
                Message::user("u"),
                Message::assistant("").with_tool_calls(vec![ToolCall::new("1", "t", json!({}))]),
                Message::tool("{\"ok\":true}", "1"),
                Message::assistant("final answer"),
            ],
        };
        let report = distill(Role::Explorer, &transcript);
        assert_eq!(report.summary, "final answer");
        assert!(!report.tool_failure);
    }
}
