#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("policy rejected transaction: {0}")]
    PolicyRejected(String),
    #[error("simulation failed: {0}")]
    SimulationFailed(String),
    #[error("approval denied: {0}")]
    ApprovalDenied(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("audit log entry serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("policy config parse error: {0}")]
    PolicyConfigParse(#[from] toml::de::Error),
    #[error("audit log at {path} is not tamper-evident: {reason}")]
    AuditChainBroken { path: String, reason: String },
    #[error("signer error: {0}")]
    Signer(#[from] altius_signer::SignerError),
    #[error("transaction is missing required signatures from: {}", .0.join(", "))]
    IncompleteSignatures(Vec<String>),
    #[error("rpc request to {rpc_url} failed: {reason}")]
    Rpc { rpc_url: String, reason: String },
}
