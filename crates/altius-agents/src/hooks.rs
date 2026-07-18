//! Deterministic PreToolUse / PostToolUse hooks around tool dispatch.
//!
//! Hooks never call TxGuard or the signer. They may deny a call or replace
//! a tool result before it re-enters the model context.

use std::sync::Arc;

use async_trait::async_trait;

use crate::llm::ToolCall;
use crate::tools::{envelope_err, ToolDispatcher};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookOutcome {
    Continue,
    Deny(String),
    /// Only meaningful for [`HookEvent::PostToolUse`].
    ReplaceResult(String),
}

pub trait ToolHook: Send + Sync {
    fn on_event(
        &self,
        event: HookEvent,
        call: &ToolCall,
        result: Option<&str>,
    ) -> HookOutcome;
}

/// Runs Pre hooks → inner dispatcher → Post hooks.
pub struct HookedDispatcher {
    hooks: Vec<Arc<dyn ToolHook>>,
    inner: Arc<dyn ToolDispatcher>,
}

impl HookedDispatcher {
    pub fn new(hooks: Vec<Arc<dyn ToolHook>>, inner: Arc<dyn ToolDispatcher>) -> Self {
        Self { hooks, inner }
    }

    pub fn wrap(inner: Arc<dyn ToolDispatcher>) -> Self {
        Self::new(Vec::new(), inner)
    }
}

#[async_trait]
impl ToolDispatcher for HookedDispatcher {
    async fn call(&self, call: &ToolCall) -> String {
        for hook in &self.hooks {
            match hook.on_event(HookEvent::PreToolUse, call, None) {
                HookOutcome::Continue => {}
                HookOutcome::Deny(reason) => return envelope_err(reason),
                HookOutcome::ReplaceResult(_) => {
                    return envelope_err("ReplaceResult is not valid for PreToolUse".to_owned());
                }
            }
        }
        let mut result = self.inner.call(call).await;
        for hook in &self.hooks {
            match hook.on_event(HookEvent::PostToolUse, call, Some(&result)) {
                HookOutcome::Continue => {}
                HookOutcome::Deny(reason) => return envelope_err(reason),
                HookOutcome::ReplaceResult(replacement) => result = replacement,
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::tools::LocalTools;
    use serde_json::json;

    struct DenyNamed {
        name: String,
        hits: AtomicUsize,
    }

    impl ToolHook for DenyNamed {
        fn on_event(
            &self,
            event: HookEvent,
            call: &ToolCall,
            _result: Option<&str>,
        ) -> HookOutcome {
            if event == HookEvent::PreToolUse && call.name == self.name {
                self.hits.fetch_add(1, Ordering::SeqCst);
                return HookOutcome::Deny(format!("blocked `{}`", call.name));
            }
            HookOutcome::Continue
        }
    }

    struct RecordingInner {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ToolDispatcher for RecordingInner {
        async fn call(&self, _call: &ToolCall) -> String {
            self.calls.fetch_add(1, Ordering::SeqCst);
            crate::tools::envelope_ok(json!({ "ran": true }))
        }
    }

    #[tokio::test]
    async fn pre_tool_use_deny_short_circuits() {
        let inner = Arc::new(RecordingInner {
            calls: AtomicUsize::new(0),
        });
        let hook = Arc::new(DenyNamed {
            name: "run_command".into(),
            hits: AtomicUsize::new(0),
        });
        let dispatcher = HookedDispatcher::new(vec![hook.clone()], inner.clone());
        let result = dispatcher
            .call(&ToolCall {
                id: "1".into(),
                name: "run_command".into(),
                arguments: json!({}),
            })
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(hook.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn empty_hooks_delegate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
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
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "solana_program::declare_id!(\"11111111111111111111111111111111\");",
        )
        .unwrap();
        let inner = Arc::new(LocalTools::new(dir.path()));
        let dispatcher = HookedDispatcher::wrap(inner);
        let result = dispatcher
            .call(&ToolCall {
                id: "1".into(),
                name: "detect_project".into(),
                arguments: json!({}),
            })
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["ok"], true);
    }
}
