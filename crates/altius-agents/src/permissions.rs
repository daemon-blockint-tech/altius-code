//! Tool permission policy for fleet agents (filesystem / shell).
//!
//! Separate from TxGuard [`altius_txguard`] money policy. Optional `[tools]`
//! section in project `altius.toml` may tighten or (carefully) widen defaults.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::fs_tools::DEFAULT_BASH_ALLOWLIST;
use crate::llm::ToolCall;
use crate::tools::{envelope_err, ToolDispatcher};

/// Outcome of evaluating a tool call against [`ToolPolicy`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolDecision {
    Allow,
    Deny(String),
}

/// FailClosed-by-default policy for local coding tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolPolicy {
    pub allow_write: bool,
    pub allow_bash: bool,
    pub bash_allowlist: Vec<String>,
    pub deny_tools: Vec<String>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self::read_only()
    }
}

impl ToolPolicy {
    pub fn read_only() -> Self {
        Self {
            allow_write: false,
            allow_bash: false,
            bash_allowlist: DEFAULT_BASH_ALLOWLIST
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            deny_tools: Vec::new(),
        }
    }

    pub fn coder() -> Self {
        Self {
            allow_write: true,
            allow_bash: true,
            bash_allowlist: DEFAULT_BASH_ALLOWLIST
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            deny_tools: Vec::new(),
        }
    }

    /// Load `[tools]` from `project_root/altius.toml`, merging onto `base`.
    /// Missing file leaves `base` unchanged.
    pub fn load_from_project(project_root: &Path, mut base: Self) -> Self {
        let path = project_root.join("altius.toml");
        let Ok(raw) = std::fs::read_to_string(path) else {
            return base;
        };
        let Ok(file) = toml::from_str::<AltiusTomlFile>(&raw) else {
            return base;
        };
        if let Some(tools) = file.tools {
            if let Some(v) = tools.allow_write {
                base.allow_write = v;
            }
            if let Some(v) = tools.allow_bash {
                base.allow_bash = v;
            }
            if let Some(list) = tools.bash_allowlist {
                base.bash_allowlist = list;
            }
            if let Some(list) = tools.deny_tools {
                base.deny_tools = list;
            }
        }
        base
    }

    pub fn evaluate(&self, call: &ToolCall) -> ToolDecision {
        if self
            .deny_tools
            .iter()
            .any(|name| name == &call.name || call.name.starts_with(name))
        {
            return ToolDecision::Deny(format!("tool `{}` is denied by policy", call.name));
        }
        match call.name.as_str() {
            "write_file" | "edit_file" if !self.allow_write => {
                ToolDecision::Deny("write tools are disabled for this agent".into())
            }
            "run_command" if !self.allow_bash => {
                ToolDecision::Deny("run_command is disabled for this agent".into())
            }
            _ => ToolDecision::Allow,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AltiusTomlFile {
    #[serde(default)]
    tools: Option<ToolsToml>,
}

#[derive(Debug, Deserialize)]
struct ToolsToml {
    allow_write: Option<bool>,
    allow_bash: Option<bool>,
    bash_allowlist: Option<Vec<String>>,
    deny_tools: Option<Vec<String>>,
}

/// Wraps a [`ToolDispatcher`] and enforces [`ToolPolicy`] before each call.
pub struct PermissionedDispatcher {
    policy: ToolPolicy,
    inner: Arc<dyn ToolDispatcher>,
}

impl PermissionedDispatcher {
    pub fn new(policy: ToolPolicy, inner: Arc<dyn ToolDispatcher>) -> Self {
        Self { policy, inner }
    }

    pub fn policy(&self) -> &ToolPolicy {
        &self.policy
    }
}

#[async_trait]
impl ToolDispatcher for PermissionedDispatcher {
    async fn call(&self, call: &ToolCall) -> String {
        match self.policy.evaluate(call) {
            ToolDecision::Allow => self.inner.call(call).await,
            ToolDecision::Deny(reason) => envelope_err(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::LocalTools;
    use serde_json::json;

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: name.into(),
            arguments: json!({}),
        }
    }

    #[test]
    fn read_only_denies_write_and_bash() {
        let policy = ToolPolicy::read_only();
        assert!(matches!(
            policy.evaluate(&call("write_file")),
            ToolDecision::Deny(_)
        ));
        assert!(matches!(
            policy.evaluate(&call("run_command")),
            ToolDecision::Deny(_)
        ));
        assert_eq!(policy.evaluate(&call("read_file")), ToolDecision::Allow);
    }

    #[tokio::test]
    async fn permissioned_dispatcher_blocks_write() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let inner = Arc::new(LocalTools::new(dir.path()));
        let dispatcher = PermissionedDispatcher::new(ToolPolicy::read_only(), inner);
        let result = dispatcher
            .call(&ToolCall {
                id: "c1".into(),
                name: "write_file".into(),
                arguments: json!({"path": "a.txt", "content": "y"}),
            })
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "x");
    }

    #[test]
    fn altius_toml_tools_section_merges() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("altius.toml"),
            r#"
[tools]
allow_write = false
deny_tools = ["lint_project"]
"#,
        )
        .unwrap();
        let policy = ToolPolicy::load_from_project(dir.path(), ToolPolicy::coder());
        assert!(!policy.allow_write);
        assert!(matches!(
            policy.evaluate(&call("lint_project")),
            ToolDecision::Deny(_)
        ));
    }
}
