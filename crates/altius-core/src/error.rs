/// Shared error type for fleet crates that do not need a domain-specific error.
#[derive(Debug, thiserror::Error)]
pub enum AltiusError {
    #[error("invalid {kind} id `{value}`: {source}")]
    InvalidId {
        kind: &'static str,
        value: String,
        #[source]
        source: uuid::Error,
    },

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("{0}")]
    Message(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl AltiusError {
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn budget(msg: impl Into<String>) -> Self {
        Self::BudgetExceeded(msg.into())
    }

    pub fn from_anyhow_like(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other(Box::new(err))
    }
}

/// Convenience alias used by fleet crates.
pub type Result<T> = std::result::Result<T, AltiusError>;

impl From<String> for AltiusError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for AltiusError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
