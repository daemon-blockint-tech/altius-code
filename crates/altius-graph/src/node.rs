use async_trait::async_trait;

use crate::error::GraphResult;
use crate::state::State;

/// Result of executing a single graph node.
#[derive(Clone, Debug)]
pub enum NodeResult<S> {
    /// Continue along the configured edges from this node.
    Continue(S),
    /// Jump to a named node (overrides static edges for this step).
    Goto { next: String, state: S },
    /// Terminal success — stop the graph with this state.
    Finish(S),
    /// Human-in-the-loop pause. The executor checkpoints and returns
    /// [`crate::ExecutionOutcome::Interrupted`]. Call
    /// [`crate::GraphExecutor::resume`] after updating state.
    Interrupt { reason: String, state: S },
}

impl<S> NodeResult<S> {
    pub fn state(&self) -> &S {
        match self {
            Self::Continue(s)
            | Self::Goto { state: s, .. }
            | Self::Finish(s)
            | Self::Interrupt { state: s, .. } => s,
        }
    }

    pub fn into_state(self) -> S {
        match self {
            Self::Continue(s)
            | Self::Goto { state: s, .. }
            | Self::Finish(s)
            | Self::Interrupt { state: s, .. } => s,
        }
    }
}

/// A named async unit of work in the graph.
#[async_trait]
pub trait Node<S: State>: Send + Sync {
    /// Stable node name used by edges / Goto / checkpoints.
    fn name(&self) -> &str;

    /// Execute the node against a state snapshot.
    async fn run(&self, state: S) -> GraphResult<NodeResult<S>>;
}
