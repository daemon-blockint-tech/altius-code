use rust_langgraph::errors::Result;
use rust_langgraph::state::State;
use serde::{Deserialize, Serialize};

/// One specialist's finished contribution to a fleet run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentReport {
    /// Role name (`explorer`, `coder`, `security`, `release`).
    pub role: String,
    /// The specialist's final answer, verbatim.
    pub summary: String,
    /// True if any tool this specialist ran reported a failure.
    pub tool_failure: bool,
}

/// Shared state threaded through the fleet pipeline graph. Each
/// specialist node contributes a partial state containing only its own
/// report; `merge` appends reports and latches `failed`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FleetState {
    pub goal: String,
    pub project: String,
    pub reports: Vec<AgentReport>,
    /// Latched true as soon as any specialist's tools report a failure;
    /// the pipeline uses it to short-circuit the remaining stages.
    pub failed: bool,
}

impl State for FleetState {
    fn merge(&mut self, other: Self) -> Result<()> {
        if !other.goal.is_empty() {
            self.goal = other.goal;
        }
        if !other.project.is_empty() {
            self.project = other.project;
        }
        self.reports.extend(other.reports);
        self.failed |= other.failed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_appends_reports_and_latches_failure() {
        let mut state = FleetState {
            goal: "ship it".into(),
            project: "/tmp/p".into(),
            reports: vec![],
            failed: false,
        };
        state
            .merge(FleetState {
                goal: String::new(),
                project: String::new(),
                reports: vec![AgentReport {
                    role: "coder".into(),
                    summary: "build broke".into(),
                    tool_failure: true,
                }],
                failed: true,
            })
            .unwrap();
        state
            .merge(FleetState {
                failed: false, // a later success must NOT clear the latch
                ..FleetState::default()
            })
            .unwrap();

        assert_eq!(state.goal, "ship it");
        assert_eq!(state.reports.len(), 1);
        assert!(state.failed);
    }
}
