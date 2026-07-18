use altius_core::AltiusError;
use altius_graph::GraphError;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error(transparent)]
    Graph(#[from] GraphError),

    #[error(transparent)]
    Core(#[from] AltiusError),

    #[error("{0}")]
    Message(String),
}

pub type AgentResult<T> = Result<T, AgentError>;

impl AgentError {
    pub fn llm(msg: impl Into<String>) -> Self {
        Self::Llm(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}
