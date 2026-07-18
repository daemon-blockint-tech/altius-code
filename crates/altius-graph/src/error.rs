use altius_core::AltiusError;

/// Errors produced by the graph runtime and memory adapters.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph build error: {0}")]
    Build(String),

    #[error("unknown node `{0}`")]
    UnknownNode(String),

    #[error("no entry node configured")]
    NoEntry,

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("node `{node}` failed: {message}")]
    NodeFailed { node: String, message: String },

    #[error("checkpoint error: {0}")]
    Checkpoint(String),

    #[error("memory error: {0}")]
    Memory(String),

    #[error("resume error: {0}")]
    Resume(String),

    #[error(transparent)]
    Core(#[from] AltiusError),
}

pub type GraphResult<T> = Result<T, GraphError>;

impl GraphError {
    pub fn build(msg: impl Into<String>) -> Self {
        Self::Build(msg.into())
    }

    pub fn node_failed(node: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NodeFailed {
            node: node.into(),
            message: message.into(),
        }
    }

    pub fn checkpoint(msg: impl Into<String>) -> Self {
        Self::Checkpoint(msg.into())
    }

    pub fn memory(msg: impl Into<String>) -> Self {
        Self::Memory(msg.into())
    }

    pub fn resume(msg: impl Into<String>) -> Self {
        Self::Resume(msg.into())
    }
}
