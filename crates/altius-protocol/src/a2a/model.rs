//! Standards-shaped A2A models (custom serde, interoperable wire format).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Result;
use crate::limits;

/// One skill an agent advertises on its card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}

/// Optional protocol capabilities an agent supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
    pub state_transition_history: bool,
}

/// The A2A Agent Card: the self-describing document served at
/// `/.well-known/agent-card.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    /// A2A protocol version this agent speaks.
    pub protocol_version: String,
    pub name: String,
    pub description: String,
    /// Base URL where the agent's A2A service is reachable.
    pub url: String,
    /// Version of the agent itself.
    pub version: String,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_input_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
}

impl AgentCard {
    /// Bounded validation, applied both to our own card at construction
    /// time and to any remote card we ingest.
    pub fn validate(&self) -> Result<()> {
        limits::bounded_string("protocolVersion", &self.protocol_version, limits::MAX_NAME_LEN)?;
        limits::bounded_string("name", &self.name, limits::MAX_NAME_LEN)?;
        limits::bounded_string("description", &self.description, limits::MAX_DESCRIPTION_LEN)?;
        limits::bounded_url("url", &self.url)?;
        limits::bounded_string("version", &self.version, limits::MAX_NAME_LEN)?;
        limits::bounded_list("skills", self.skills.len(), limits::MAX_LIST_LEN)?;
        limits::bounded_list(
            "defaultInputModes",
            self.default_input_modes.len(),
            limits::MAX_LIST_LEN,
        )?;
        limits::bounded_list(
            "defaultOutputModes",
            self.default_output_modes.len(),
            limits::MAX_LIST_LEN,
        )?;
        for skill in &self.skills {
            limits::bounded_string("skill.id", &skill.id, limits::MAX_NAME_LEN)?;
            limits::bounded_string("skill.name", &skill.name, limits::MAX_NAME_LEN)?;
            limits::bounded_string(
                "skill.description",
                &skill.description,
                limits::MAX_DESCRIPTION_LEN,
            )?;
            limits::bounded_list("skill.tags", skill.tags.len(), limits::MAX_LIST_LEN)?;
            limits::bounded_list("skill.examples", skill.examples.len(), limits::MAX_LIST_LEN)?;
        }
        Ok(())
    }
}

/// Lifecycle states of an A2A task (kebab-case on the wire, matching the
/// spec's `TaskState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Canceled,
    Failed,
    Rejected,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Canceled | Self::Failed | Self::Rejected)
    }
}

/// Current status of a task, with an optional agent message explaining it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<A2aMessage>,
}

impl TaskStatus {
    pub fn now(state: TaskState) -> Self {
        Self {
            state,
            timestamp: Utc::now(),
            message: None,
        }
    }
}

/// One part of an A2A message or artifact, tagged by `kind`.
///
/// `Data` is the opaque payload channel: arbitrary JSON that Altius passes
/// through without interpreting, subject only to a size bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Part {
    Text { text: String },
    Data { data: Value },
}

impl Part {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Text { text } => limits::bounded_string("part.text", text, limits::MAX_TEXT_LEN),
            Self::Data { data } => limits::bounded_opaque_json("part.data", data),
        }
    }
}

/// A message within a task ("user" = the calling agent, "agent" = us).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aMessage {
    pub role: String,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl A2aMessage {
    pub fn agent_text(text: impl Into<String>) -> Self {
        Self {
            role: "agent".to_owned(),
            parts: vec![Part::Text { text: text.into() }],
            message_id: Some(Uuid::new_v4().to_string()),
        }
    }

    /// Bounded validation for untrusted inbound messages.
    pub fn validate(&self) -> Result<()> {
        limits::bounded_string("role", &self.role, limits::MAX_NAME_LEN)?;
        limits::bounded_opt_string("messageId", self.message_id.as_deref(), limits::MAX_NAME_LEN)?;
        limits::bounded_list("parts", self.parts.len(), limits::MAX_LIST_LEN)?;
        for part in &self.parts {
            part.validate()?;
        }
        Ok(())
    }
}

/// An output produced by a task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parts: Vec<Part>,
}

/// An A2A task: the unit of delegated work between agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// Groups related tasks belonging to one logical conversation.
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<A2aMessage>,
}

impl Task {
    /// Create a new task in the `submitted` state, recording the inbound
    /// message in its history.
    pub fn submitted(message: A2aMessage) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            context_id: Uuid::new_v4().to_string(),
            status: TaskStatus::now(TaskState::Submitted),
            artifacts: Vec::new(),
            history: vec![message],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card() -> AgentCard {
        AgentCard {
            protocol_version: "0.3.0".into(),
            name: "altius".into(),
            description: "Altius SVM fleet agent".into(),
            url: "https://agents.example.com/a2a".into(),
            version: "0.1.0".into(),
            capabilities: AgentCapabilities::default(),
            default_input_modes: vec!["text/plain".into()],
            default_output_modes: vec!["text/plain".into()],
            skills: vec![AgentSkill {
                id: "svm-detect".into(),
                name: "SVM project detection".into(),
                description: "Detect Solana project frameworks".into(),
                tags: vec!["solana".into()],
                examples: vec![],
            }],
        }
    }

    #[test]
    fn agent_card_serializes_camel_case() {
        let value = serde_json::to_value(card()).unwrap();
        assert_eq!(value["protocolVersion"], "0.3.0");
        assert_eq!(value["defaultInputModes"][0], "text/plain");
        assert_eq!(value["skills"][0]["id"], "svm-detect");
        let back: AgentCard = serde_json::from_value(value).unwrap();
        assert_eq!(back, card());
    }

    #[test]
    fn agent_card_validation_rejects_bad_url() {
        let mut bad = card();
        bad.url = "ftp://example.com".into();
        assert!(bad.validate().is_err());
        assert!(card().validate().is_ok());
    }

    #[test]
    fn task_state_uses_kebab_case() {
        assert_eq!(
            serde_json::to_string(&TaskState::InputRequired).unwrap(),
            "\"input-required\""
        );
        assert!(TaskState::Rejected.is_terminal());
        assert!(!TaskState::Working.is_terminal());
    }

    #[test]
    fn parts_are_kind_tagged_and_opaque_data_round_trips() {
        let message = A2aMessage {
            role: "user".into(),
            parts: vec![
                Part::Text { text: "run".into() },
                Part::Data {
                    data: json!({ "anything": [1, 2, 3], "nested": { "ok": true } }),
                },
            ],
            message_id: None,
        };
        message.validate().unwrap();
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["parts"][0]["kind"], "text");
        assert_eq!(value["parts"][1]["kind"], "data");
        let back: A2aMessage = serde_json::from_value(value).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn oversized_opaque_payload_is_rejected() {
        let message = A2aMessage {
            role: "user".into(),
            parts: vec![Part::Data {
                data: json!({ "blob": "x".repeat(limits::MAX_OPAQUE_JSON_BYTES + 1) }),
            }],
            message_id: None,
        };
        assert!(message.validate().is_err());
    }
}
