//! Offline test double for [`ChatModel`]: replays a fixed script of
//! messages, one per `invoke` call. Lets the fleet's graph wiring, tool
//! execution, and failure routing be tested end-to-end with zero
//! network access or API keys.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_langgraph::errors::{Error, Result};
use rust_langgraph::llm::ChatModel;
use rust_langgraph::state::Message;

/// A [`ChatModel`] that pops the next message off a pre-written script
/// on every `invoke`. Clones share the same script queue, matching how
/// the graph runtime clones models between node executions.
#[derive(Clone)]
pub struct ScriptedModel {
    name: String,
    script: Arc<Mutex<VecDeque<Message>>>,
}

impl ScriptedModel {
    pub fn new(name: impl Into<String>, script: Vec<Message>) -> ScriptedModel {
        ScriptedModel {
            name: name.into(),
            script: Arc::new(Mutex::new(script.into())),
        }
    }

    /// Messages not yet consumed — assert this is 0 at the end of a
    /// test to prove the whole script ran.
    pub fn remaining(&self) -> usize {
        self.script.lock().expect("script lock").len()
    }
}

#[async_trait]
impl ChatModel for ScriptedModel {
    async fn invoke(&self, _messages: &[Message]) -> Result<Message> {
        self.script
            .lock()
            .expect("script lock")
            .pop_front()
            .ok_or_else(|| {
                Error::ExecutionError(format!("scripted model {:?} ran out of script", self.name))
            })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn clone_box(&self) -> Box<dyn ChatModel> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replays_in_order_then_errors() {
        let model = ScriptedModel::new(
            "m",
            vec![Message::assistant("one"), Message::assistant("two")],
        );
        assert_eq!(model.invoke(&[]).await.unwrap().content, "one");
        assert_eq!(model.invoke(&[]).await.unwrap().content, "two");
        assert!(model.invoke(&[]).await.is_err());
        assert_eq!(model.remaining(), 0);
    }

    #[tokio::test]
    async fn clones_share_one_script() {
        let model = ScriptedModel::new("m", vec![Message::assistant("only")]);
        let clone = model.clone_box();
        assert_eq!(clone.invoke(&[]).await.unwrap().content, "only");
        assert!(model.invoke(&[]).await.is_err());
    }
}
