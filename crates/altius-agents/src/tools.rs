//! Tools exposed to fleet nodes.
//!
//! Local tools include sandboxed filesystem / allowlisted commands plus SVM
//! detect/lint. External MCP tools (browser, …) are dispatched through
//! [`ToolDispatcher`] implementations that bound and redact results before
//! they re-enter the conversation. Tool arguments come from LLM output and
//! are treated as untrusted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use altius_detect::{detect_best, DetectionRegistry};
use altius_mcp::{AttachedMcp, DiscoveredTool};
use altius_svm_detect::{detect, Framework};
use altius_svm_tools::{AnchorToolchain, CargoBuildSbfToolchain, SvmToolchain};
use async_trait::async_trait;
use serde_json::json;

use crate::error::AgentResult;
use crate::fs_tools::{self, DEFAULT_BASH_ALLOWLIST};
use crate::llm::{ChatMessage, LlmClient, ToolCall, ToolSpec};
use crate::permissions::ToolPolicy;

/// Maximum model↔tool round trips before we force a final answer.
const MAX_TOOL_ROUNDS: usize = 12;
/// Upper bound on a single tool result fed back to the model.
const MAX_TOOL_RESULT_BYTES: usize = 16 * 1024;

/// Default allowlist prefix for browser MCP tools (Playwright-style).
pub const BROWSER_TOOL_PREFIX: &str = "browser_";

/// GitHub connector capability exposed to the GitHub specialist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitHubAccess {
    /// Repository, issue, workflow, and pull-request inspection only.
    #[default]
    ReadOnly,
    /// Read access plus branch/file writes and pull-request creation/update.
    /// Merge, delete, release, workflow-dispatch, and administration tools
    /// remain unavailable.
    PullRequests,
}

/// Execute one tool call and return a bounded, redacted result string.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    async fn call(&self, call: &ToolCall) -> String;
}

/// Extract the project root from a `[project_path=...]` marker in the user
/// prompt, defaulting to the current directory.
pub fn project_root_from_prompt(prompt: &str) -> PathBuf {
    prompt
        .split("[project_path=")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn path_tool_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path relative to the project root (default: the root itself)."
            }
        },
        "additionalProperties": false
    })
}

fn read_only_fs_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file".into(),
            description: "Read a UTF-8 text file under the project root. Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "grep".into(),
            description: "Search file contents under the project root with a regex. Read-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": {
                        "type": "string",
                        "description": "Optional subdirectory or file relative to the root."
                    }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "glob".into(),
            description: "List files under the project root matching a glob pattern. Read-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob such as `src/**/*.rs`."
                    }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
    ]
}

fn detect_lint_tools() -> Vec<ToolSpec> {
    let parameters = path_tool_parameters();
    vec![
        ToolSpec {
            name: "detect_project".into(),
            description:
                "Detect an SVM project's framework, programs, toolchain, and default cluster. \
                 Read-only; never deploys or signs."
                    .into(),
            parameters: parameters.clone(),
        },
        ToolSpec {
            name: "lint_project".into(),
            description: "Run Altius heuristic security lints against an SVM project. \
                 Read-only; never deploys or signs."
                .into(),
            parameters,
        },
    ]
}

/// Tool specs available to the explorer node (read-only).
pub(crate) fn explorer_tools() -> Vec<ToolSpec> {
    let mut tools = detect_lint_tools();
    tools.extend(read_only_fs_tools());
    tools
}

/// Tool specs available to the security node (read-only).
pub(crate) fn security_tools() -> Vec<ToolSpec> {
    let mut tools = detect_lint_tools();
    tools.push(ToolSpec {
        name: "scan_project".into(),
        description: "Run Altius native multi-chain security scanners and return \
             canonical findings. Read-only; never deploys or signs."
            .into(),
        parameters: path_tool_parameters(),
    });
    tools.extend(read_only_fs_tools());
    tools
}

/// Tool specs for the coder node (read + write + allowlisted commands).
pub(crate) fn coder_tools() -> Vec<ToolSpec> {
    let mut tools = explorer_tools();
    tools.push(ToolSpec {
        name: "write_file".into(),
        description: "Create or overwrite a UTF-8 file under the project root. Never signs.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
    });
    tools.push(ToolSpec {
        name: "edit_file".into(),
        description: "Replace one unique occurrence of old_string with new_string in a file."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        }),
    });
    tools.push(ToolSpec {
        name: "run_command".into(),
        description: "Run an allowlisted build/test command (cargo, anchor, forge, …). \
             FailClosed argv; never signs or deploys via this tool."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "argv": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Argument vector, e.g. [\"cargo\", \"test\"]."
                }
            },
            "required": ["argv"],
            "additionalProperties": false
        }),
    });
    tools
}

/// Convert discovered MCP tools into [`ToolSpec`]s, keeping only names that
/// match `prefix` (empty prefix keeps all).
pub fn tool_specs_from_discovered(tools: &[DiscoveredTool], prefix: &str) -> Vec<ToolSpec> {
    tools
        .iter()
        .filter(|tool| prefix.is_empty() || tool.name.starts_with(prefix))
        .map(|tool| ToolSpec {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        })
        .collect()
}

/// Bounded tool-use loop: call the model with tools, execute any returned
/// calls via `dispatcher`, feed results back, and return the final assistant
/// text.
///
/// Clients whose `complete_with_tools` never returns tool calls (e.g. the
/// offline client) fall through on the first iteration, so offline behavior
/// stays deterministic.
pub(crate) async fn tool_loop(
    llm: &dyn LlmClient,
    tools: &[ToolSpec],
    dispatcher: &dyn ToolDispatcher,
    mut messages: Vec<ChatMessage>,
) -> AgentResult<String> {
    for _ in 0..MAX_TOOL_ROUNDS {
        let (text, calls) = llm.complete_with_tools(&messages, tools).await?;
        if calls.is_empty() {
            return Ok(text);
        }
        messages.push(ChatMessage::assistant_tool_calls(text, calls.clone()));
        for call in &calls {
            let result = dispatcher.call(call).await;
            messages.push(ChatMessage::tool(&call.id, &call.name, result));
        }
    }
    // Round cap reached: ask for a plain final answer without tools.
    llm.complete(&messages).await
}

/// Local tools (detect / lint / FS / allowlisted commands), path-confined.
pub struct LocalTools {
    project_root: PathBuf,
    bash_allowlist: Vec<String>,
}

impl LocalTools {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            bash_allowlist: DEFAULT_BASH_ALLOWLIST
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }

    pub fn with_policy(project_root: impl Into<PathBuf>, policy: &ToolPolicy) -> Self {
        Self {
            project_root: project_root.into(),
            bash_allowlist: policy.bash_allowlist.clone(),
        }
    }
}

#[async_trait]
impl ToolDispatcher for LocalTools {
    async fn call(&self, call: &ToolCall) -> String {
        execute_local_tool(&self.project_root, &self.bash_allowlist, call).await
    }
}

/// External MCP tools with a name-prefix allowlist.
///
/// A rogue MCP cannot surface a dangerous tool that Altius will blindly call:
/// only names starting with `allowed_prefix` are forwarded.
pub struct McpTools {
    attachment: Arc<AttachedMcp>,
    allowed_prefix: String,
    allowed_names: Option<HashSet<String>>,
}

impl McpTools {
    pub fn new(attachment: Arc<AttachedMcp>, allowed_prefix: impl Into<String>) -> Self {
        Self {
            attachment,
            allowed_prefix: allowed_prefix.into(),
            allowed_names: None,
        }
    }

    pub fn browser(attachment: Arc<AttachedMcp>) -> Self {
        Self::new(attachment, BROWSER_TOOL_PREFIX)
    }

    /// GitHub MCP tools filtered through a capability allowlist.
    pub fn github(attachment: Arc<AttachedMcp>, access: GitHubAccess) -> Self {
        let allowed_names = attachment
            .tools()
            .iter()
            .filter(|tool| github_tool_allowed(&tool.name, access))
            .map(|tool| tool.name.clone())
            .collect();
        Self {
            attachment,
            allowed_prefix: String::new(),
            allowed_names: Some(allowed_names),
        }
    }

    pub fn allowed_prefix(&self) -> &str {
        &self.allowed_prefix
    }

    /// Specs the LLM may see — already filtered by the allowlist prefix.
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.attachment
            .tools()
            .iter()
            .filter(|tool| {
                self.allowed_names
                    .as_ref()
                    .is_none_or(|names| names.contains(&tool.name))
                    && (self.allowed_prefix.is_empty()
                        || tool.name.starts_with(&self.allowed_prefix))
            })
            .map(|tool| ToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect()
    }
}

#[async_trait]
impl ToolDispatcher for McpTools {
    async fn call(&self, call: &ToolCall) -> String {
        if self
            .allowed_names
            .as_ref()
            .is_some_and(|names| !names.contains(&call.name))
        {
            return envelope_err(format!(
                "GitHub tool `{}` rejected by connector capability policy",
                call.name
            ));
        }
        if !self.allowed_prefix.is_empty() && !call.name.starts_with(&self.allowed_prefix) {
            return envelope_err(format!(
                "tool `{}` rejected: name must start with `{}`",
                call.name, self.allowed_prefix
            ));
        }
        match self
            .attachment
            .call_tool(&call.name, call.arguments.clone())
            .await
        {
            Ok(data) => envelope_ok(data),
            Err(error) => envelope_err(error.to_string()),
        }
    }
}

fn github_tool_allowed(name: &str, access: GitHubAccess) -> bool {
    let base = name
        .rsplit_once("__")
        .map(|(_, suffix)| suffix)
        .unwrap_or(name);
    let read_only = ["get_", "list_", "search_", "read_", "whoami"]
        .iter()
        .any(|prefix| base.starts_with(prefix));
    if read_only {
        return true;
    }
    access == GitHubAccess::PullRequests
        && matches!(
            base,
            "create_branch"
                | "create_or_update_file"
                | "push_files"
                | "create_pull_request"
                | "update_pull_request"
                | "update_pull_request_branch"
                | "add_issue_comment"
                | "add_pull_request_review_comment"
                | "request_copilot_review"
        )
}

/// Execute one local tool call. Failures are reported inside the result
/// envelope (never as an `Err`) so the model can react to them.
pub(crate) async fn execute_local_tool(
    project_root: &Path,
    bash_allowlist: &[String],
    call: &ToolCall,
) -> String {
    let name = call.name.clone();
    let args = call.arguments.clone();
    let root = project_root.to_path_buf();
    let allowlist = bash_allowlist.to_vec();
    let outcome = tokio::task::spawn_blocking(move || {
        execute_local_tool_sync(&root, &allowlist, &name, &args)
    })
    .await
    .unwrap_or_else(|error| Err(format!("tool worker failed: {error}")));
    match outcome {
        Ok(data) => envelope_ok(data),
        Err(error) => envelope_err(error),
    }
}

fn execute_local_tool_sync(
    project_root: &Path,
    bash_allowlist: &[String],
    name: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match name {
        "detect_project" => {
            let target = resolve_detect_path(project_root, arguments)?;
            detect_output(&target)
        }
        "lint_project" => {
            let target = resolve_detect_path(project_root, arguments)?;
            lint_output(&target)
        }
        "scan_project" => {
            let target = resolve_detect_path(project_root, arguments)?;
            scan_output(&target)
        }
        "read_file" => {
            let path = require_str(arguments, "path")?;
            fs_tools::read_file(project_root, path)
        }
        "write_file" => {
            let path = require_str(arguments, "path")?;
            let content = require_str(arguments, "content")?;
            fs_tools::write_file(project_root, path, content)
        }
        "edit_file" => {
            let path = require_str(arguments, "path")?;
            let old_string = require_str(arguments, "old_string")?;
            let new_string = require_str(arguments, "new_string")?;
            fs_tools::edit_file(project_root, path, old_string, new_string)
        }
        "grep" => {
            let pattern = require_str(arguments, "pattern")?;
            let path = arguments.get("path").and_then(|v| v.as_str());
            fs_tools::grep(project_root, pattern, path)
        }
        "glob" => {
            let pattern = require_str(arguments, "pattern")?;
            fs_tools::glob_files(project_root, pattern)
        }
        "run_command" => {
            let argv = arguments
                .get("argv")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "`argv` must be an array of strings".to_owned())?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_owned())
                        .ok_or_else(|| "`argv` entries must be strings".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            fs_tools::run_command(project_root, &argv, bash_allowlist)
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn require_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("`{key}` is required"))
}

/// Resolve optional `path` for detect/lint (default `.`); must exist.
fn resolve_detect_path(
    project_root: &Path,
    arguments: &serde_json::Value,
) -> Result<PathBuf, String> {
    let requested = arguments
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    if requested == "." {
        return project_root
            .canonicalize()
            .map_err(|error| format!("cannot resolve project root: {error}"));
    }
    fs_tools::resolve_existing_path(project_root, requested)
}

pub fn envelope_ok(data: serde_json::Value) -> String {
    bounded_redacted(&json!({ "ok": true, "data": data }).to_string())
}

pub fn envelope_err(error: impl Into<String>) -> String {
    bounded_redacted(&json!({ "ok": false, "error": error.into() }).to_string())
}

fn detect_output(project: &Path) -> Result<serde_json::Value, String> {
    let registry = DetectionRegistry::with_defaults();
    match detect_best(&registry, project).map_err(|error| error.to_string())? {
        None => Ok(json!({ "svm_project": false, "detected": false })),
        Some(detected) => {
            // Preserve the historical SVM-shaped payload when the winner is Solana.
            if detected.chain == altius_findings::ChainFamily::Solana {
                if let Ok(Some(svm)) = detect(project) {
                    return Ok(json!({
                        "svm_project": true,
                        "detected": true,
                        "chain": detected.chain.as_str(),
                        "plugin": detected.plugin,
                        "rank": detected.rank,
                        "framework": svm.framework.to_string(),
                        "default_cluster": svm.default_cluster.to_string(),
                        "toolchain": format!("{:?}", svm.toolchain),
                        "programs": svm
                            .programs
                            .iter()
                            .map(|program| json!({
                                "name": program.name,
                                "program_id": program.program_id,
                            }))
                            .collect::<Vec<_>>(),
                    }));
                }
            }
            Ok(json!({
                "svm_project": false,
                "detected": true,
                "chain": detected.chain.as_str(),
                "plugin": detected.plugin,
                "rank": detected.rank,
                "hints": detected.hints,
            }))
        }
    }
}

fn lint_output(project: &Path) -> Result<serde_json::Value, String> {
    let detected = detect(project)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no supported SVM project found at the requested path".to_owned())?;
    let toolchain: Box<dyn SvmToolchain> = match detected.framework {
        Framework::Anchor => Box::new(AnchorToolchain::new(project)),
        Framework::Pinocchio | Framework::Native => Box::new(CargoBuildSbfToolchain::new(project)),
    };
    let report = toolchain.lint().map_err(|error| error.to_string())?;
    Ok(report
        .to_scan_report(project.display().to_string())
        .to_lint_compat_json())
}

fn scan_output(project: &Path) -> Result<serde_json::Value, String> {
    let registry = altius_scanners::default_registry();
    let report = registry
        .scan_all(project)
        .map_err(|error| error.to_string())?;
    Ok(report.to_lint_compat_json())
}

fn bounded_redacted(value: &str) -> String {
    if altius_core::contains_probable_private_key(value) {
        return "[REDACTED: probable private-key material withheld]".to_owned();
    }
    let redacted = altius_core::redact_secrets(value);
    if redacted.len() <= MAX_TOOL_RESULT_BYTES {
        return redacted;
    }
    let mut boundary = MAX_TOOL_RESULT_BYTES;
    while !redacted.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…[truncated]", &redacted[..boundary])
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::error::AgentResult;

    #[test]
    fn github_tool_policy_is_least_privilege() {
        assert!(github_tool_allowed(
            "get_file_contents",
            GitHubAccess::ReadOnly
        ));
        assert!(github_tool_allowed("search_code", GitHubAccess::ReadOnly));
        assert!(!github_tool_allowed(
            "create_pull_request",
            GitHubAccess::ReadOnly
        ));
        assert!(github_tool_allowed(
            "create_pull_request",
            GitHubAccess::PullRequests
        ));
        assert!(github_tool_allowed(
            "github__push_files",
            GitHubAccess::PullRequests
        ));
        assert!(!github_tool_allowed(
            "merge_pull_request",
            GitHubAccess::PullRequests
        ));
        assert!(!github_tool_allowed(
            "delete_repository",
            GitHubAccess::PullRequests
        ));
    }

    #[test]
    fn project_root_marker_is_parsed() {
        assert_eq!(
            project_root_from_prompt("lint this [project_path=/tmp/demo] please"),
            PathBuf::from("/tmp/demo")
        );
        assert_eq!(
            project_root_from_prompt("no marker at all"),
            PathBuf::from(".")
        );
        assert_eq!(
            project_root_from_prompt("[project_path=]"),
            PathBuf::from(".")
        );
    }

    fn native_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "fixture"
version = "0.1.0"
[lib]
crate-type = ["cdylib", "lib"]
[dependencies]
solana-program = "2"
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "solana_program::declare_id!(\"11111111111111111111111111111111\");",
        )
        .unwrap();
        dir
    }

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            name: name.into(),
            arguments,
        }
    }

    #[tokio::test]
    async fn detect_tool_reports_native_project() {
        let project = native_project();
        let dispatcher = LocalTools::new(project.path());
        let result = dispatcher.call(&call("detect_project", json!({}))).await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["data"]["framework"], "native");
    }

    #[tokio::test]
    async fn tool_paths_cannot_escape_project_root() {
        let project = native_project();
        let dispatcher = LocalTools::new(project.path());
        for path in ["..", "/etc"] {
            let result = dispatcher
                .call(&call("detect_project", json!({ "path": path })))
                .await;
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            assert_eq!(parsed["ok"], false, "path {path} should be rejected");
        }
    }

    #[tokio::test]
    async fn write_and_read_file_tools() {
        let project = native_project();
        let dispatcher = LocalTools::new(project.path());
        let written = dispatcher
            .call(&call(
                "write_file",
                json!({"path": "notes.txt", "content": "hello fleet"}),
            ))
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed["ok"], true);
        let read = dispatcher
            .call(&call("read_file", json!({"path": "notes.txt"})))
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&read).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["data"]["content"], "hello fleet");
    }

    #[tokio::test]
    async fn unknown_tool_is_reported_in_envelope() {
        let project = native_project();
        let dispatcher = LocalTools::new(project.path());
        let result = dispatcher.call(&call("deploy_program", json!({}))).await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["ok"], false);
    }

    #[test]
    fn discovered_tools_are_prefix_filtered() {
        let tools = vec![
            DiscoveredTool {
                name: "browser_navigate".into(),
                description: "go".into(),
                parameters: json!({"type": "object"}),
            },
            DiscoveredTool {
                name: "shell_exec".into(),
                description: "danger".into(),
                parameters: json!({"type": "object"}),
            },
        ];
        let specs = tool_specs_from_discovered(&tools, BROWSER_TOOL_PREFIX);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "browser_navigate");
    }

    #[test]
    fn coder_tools_include_write_and_bash() {
        let names: Vec<_> = coder_tools().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"write_file".into()));
        assert!(names.contains(&"run_command".into()));
        assert!(names.contains(&"read_file".into()));
    }

    /// Client that requests one `detect_project` call, then answers with text.
    struct ScriptedLlm {
        rounds: AtomicUsize,
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn complete(&self, _messages: &[ChatMessage]) -> AgentResult<String> {
            Ok("fallback".into())
        }

        async fn complete_with_tools(
            &self,
            messages: &[ChatMessage],
            _tools: &[ToolSpec],
        ) -> AgentResult<(String, Vec<ToolCall>)> {
            if self.rounds.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok((
                    String::new(),
                    vec![ToolCall {
                        id: "call_1".into(),
                        name: "detect_project".into(),
                        arguments: json!({}),
                    }],
                ));
            }
            let tool_result = messages
                .iter()
                .rev()
                .find(|m| m.role == crate::llm::Role::Tool)
                .expect("tool result should be in the transcript");
            assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
            assert!(tool_result.content.contains("\"framework\":\"native\""));
            Ok(("EXPLORATION: found a native project".into(), Vec::new()))
        }
    }

    #[tokio::test]
    async fn tool_loop_executes_calls_and_returns_final_text() {
        let project = native_project();
        let llm = ScriptedLlm {
            rounds: AtomicUsize::new(0),
        };
        let dispatcher = LocalTools::new(project.path());
        let text = tool_loop(
            &llm,
            &explorer_tools(),
            &dispatcher,
            vec![
                ChatMessage::system("You are the ALTIUS EXPLORER agent."),
                ChatMessage::user("detect this project"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(text, "EXPLORATION: found a native project");
        assert_eq!(llm.rounds.load(Ordering::SeqCst), 2);
    }

    /// Dispatcher that records the tool name and rejects non-browser prefixes.
    struct RecordingDispatcher {
        seen: std::sync::Mutex<Vec<String>>,
        prefix: String,
    }

    #[async_trait]
    impl ToolDispatcher for RecordingDispatcher {
        async fn call(&self, call: &ToolCall) -> String {
            self.seen.lock().unwrap().push(call.name.clone());
            if !call.name.starts_with(&self.prefix) {
                return envelope_err(format!("rejected `{}`", call.name));
            }
            envelope_ok(json!({ "navigated": true }))
        }
    }

    #[tokio::test]
    async fn dispatcher_allowlist_rejects_non_prefixed_tools() {
        let dispatcher = RecordingDispatcher {
            seen: std::sync::Mutex::new(Vec::new()),
            prefix: BROWSER_TOOL_PREFIX.into(),
        };
        let rejected = dispatcher.call(&call("shell_exec", json!({}))).await;
        let parsed: serde_json::Value = serde_json::from_str(&rejected).unwrap();
        assert_eq!(parsed["ok"], false);

        let allowed = dispatcher
            .call(&call(
                "browser_navigate",
                json!({"url": "https://example.com"}),
            ))
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&allowed).unwrap();
        assert_eq!(parsed["ok"], true);
    }
}
