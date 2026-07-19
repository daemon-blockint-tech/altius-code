use altius_core::RunId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::Result;
use crate::limits;

/// Lifecycle states of a BeeAI ACP run.
///
/// Wire format uses the protocol's kebab-case names
/// (`created`, `in-progress`, `awaiting`, `completed`, `failed`, `cancelled`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Created,
    InProgress,
    Awaiting,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    /// Terminal states admit no further transitions.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Strict transition table for the run lifecycle.
    ///
    /// - `created` may start (`in-progress`), be `cancelled`, or `fail`
    ///   before starting.
    /// - `in-progress` may pause (`awaiting`) or reach any terminal state.
    /// - `awaiting` may resume (`in-progress`), be `cancelled`, or `fail`
    ///   (e.g. the await times out); it cannot complete without resuming.
    /// - terminal states transition nowhere.
    pub fn can_transition_to(self, next: RunStatus) -> bool {
        use RunStatus::*;
        matches!(
            (self, next),
            (Created, InProgress)
                | (Created, Cancelled)
                | (Created, Failed)
                | (InProgress, Awaiting)
                | (InProgress, Completed)
                | (InProgress, Failed)
                | (InProgress, Cancelled)
                | (Awaiting, InProgress)
                | (Awaiting, Failed)
                | (Awaiting, Cancelled)
        )
    }

    /// Protocol wire name (kebab-case), for error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::InProgress => "in-progress",
            Self::Awaiting => "awaiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Coarse approval kind surfaced when a run enters `awaiting`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalKind {
    /// Generic human-in-the-loop pause (tool, policy, graph interrupt).
    Generic,
    /// On-chain action preview (TxGuard / DiffReport semantics).
    Transaction,
}

/// Lamport balance change for one account in a transaction preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LamportDelta {
    pub account: String,
    /// Signed lamport delta as a decimal string (JSON-safe).
    pub delta_lamports: String,
}

/// Structured transaction preview (DiffReport-shaped, wire-safe).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TransactionPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lamport_deltas: Vec<LamportDelta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invoked_programs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_unit_limit: Option<u64>,
}

/// Typed approval payload attached to a run while `status == awaiting`.
///
/// Emitted on the run snapshot (`GET /runs/{id}`) and in SSE `event: run`
/// frames when the run pauses for human approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RunApproval {
    /// Human-readable headline for the approval card.
    pub summary: String,
    /// Longer explanation when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Graph node that raised the interrupt, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub kind: ApprovalKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionPreview>,
}

impl RunApproval {
    /// Generic HITL approval from an interrupt reason (no transaction preview).
    pub fn generic(summary: impl Into<String>, node: Option<String>) -> Self {
        let summary = summary.into();
        Self {
            reason: Some(summary.clone()),
            summary,
            node,
            kind: ApprovalKind::Generic,
            transaction: None,
        }
    }
}

/// Caller response when resuming an `awaiting` run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApprovalDecision {
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One typed content part of a [`Message`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessagePart {
    /// MIME type of the content (e.g. `text/plain`, `application/json`).
    pub content_type: String,
    /// The content itself. Treated as opaque untrusted data.
    pub content: String,
}

impl MessagePart {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content_type: "text/plain".to_owned(),
            content: content.into(),
        }
    }

    fn validate(&self) -> Result<()> {
        limits::bounded_string("content_type", &self.content_type, limits::MAX_NAME_LEN)?;
        limits::bounded_string("content", &self.content, limits::MAX_TEXT_LEN)
    }
}

/// A message exchanged with an agent as run input or output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Message {
    /// Originating role (e.g. `user`, `agent`).
    pub role: String,
    pub parts: Vec<MessagePart>,
}

impl Message {
    pub fn user_text(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            parts: vec![MessagePart::text(content)],
        }
    }

    /// Bounded validation for untrusted inbound messages.
    pub fn validate(&self) -> Result<()> {
        limits::bounded_string("role", &self.role, limits::MAX_NAME_LEN)?;
        limits::bounded_list("parts", self.parts.len(), limits::MAX_LIST_LEN)?;
        for part in &self.parts {
            part.validate()?;
        }
        Ok(())
    }
}

/// Standard protocol error envelope returned by BeeACP handlers.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ProtocolErrorBody {
    pub error: ProtocolErrorDetail,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ProtocolErrorDetail {
    pub code: String,
    pub message: String,
}

/// Probe responses (also documented in OpenAPI).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ReadyResponse {
    pub status: String,
}

/// A single run of an agent: the unit of the BeeAI ACP lifecycle.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Run {
    #[schema(value_type = String, example = "550e8400-e29b-41d4-a716-446655440000")]
    pub run_id: RunId,
    /// Name of the agent this run targets.
    pub agent_name: String,
    pub status: RunStatus,
    /// Input messages the run was created with.
    pub input: Vec<Message>,
    /// Output messages, populated when the run completes.
    pub output: Vec<Message>,
    /// Human-readable failure reason when `status == failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Structured approval payload when `status == awaiting`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<RunApproval>,
    pub created_at: DateTime<Utc>,
    /// Set when the run enters a terminal state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl Run {
    /// Create a new run in the `created` state.
    pub fn new(agent_name: impl Into<String>, input: Vec<Message>) -> Self {
        Self {
            run_id: RunId::new(),
            agent_name: agent_name.into(),
            status: RunStatus::Created,
            input,
            output: Vec::new(),
            error: None,
            approval: None,
            created_at: Utc::now(),
            finished_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&RunStatus::InProgress).unwrap(),
            "\"in-progress\""
        );
        let parsed: RunStatus = serde_json::from_str("\"cancelled\"").unwrap();
        assert_eq!(parsed, RunStatus::Cancelled);
    }

    #[test]
    fn transition_table_is_strict() {
        use RunStatus::*;
        // Allowed.
        assert!(Created.can_transition_to(InProgress));
        assert!(InProgress.can_transition_to(Awaiting));
        assert!(Awaiting.can_transition_to(InProgress));
        assert!(InProgress.can_transition_to(Completed));
        assert!(Awaiting.can_transition_to(Cancelled));
        // Forbidden.
        assert!(!Created.can_transition_to(Completed));
        assert!(!Created.can_transition_to(Awaiting));
        assert!(!Awaiting.can_transition_to(Completed));
        assert!(!Completed.can_transition_to(InProgress));
        assert!(!Cancelled.can_transition_to(InProgress));
        assert!(!Failed.can_transition_to(Created));
        for s in [Completed, Failed, Cancelled] {
            assert!(s.is_terminal());
            for next in [Created, InProgress, Awaiting, Completed, Failed, Cancelled] {
                assert!(!s.can_transition_to(next));
            }
        }
    }

    #[test]
    fn message_validation_bounds_untrusted_input() {
        assert!(Message::user_text("hello").validate().is_ok());
        let oversized = Message::user_text("x".repeat(crate::limits::MAX_TEXT_LEN + 1));
        assert!(oversized.validate().is_err());
    }
}
