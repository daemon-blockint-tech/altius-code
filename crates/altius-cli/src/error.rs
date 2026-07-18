#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(
        "{0} is not a recognized SVM project (no Anchor.toml, no cargo-based program crate found)"
    )]
    NotAnSvmProject(String),
    #[error("project detection failed: {0}")]
    Detect(#[from] altius_svm_detect::DetectError),
    #[error(transparent)]
    Tool(#[from] altius_svm_tools::ToolError),
    #[error(transparent)]
    Guard(#[from] altius_txguard::GuardError),
    #[error(transparent)]
    Signer(#[from] altius_signer::SignerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("rpc request to {rpc_url} failed: {reason}")]
    Rpc { rpc_url: String, reason: String },
    #[error("no signer socket configured: pass --signer-socket or set ALTIUS_SIGNER_SOCKET")]
    MissingSignerSocket,
    #[error("invalid --cluster value: {0}")]
    InvalidCluster(#[from] altius_svm_detect::ParseClusterError),
}
