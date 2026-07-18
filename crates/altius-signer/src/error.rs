#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed request/response: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("keypair file {path} does not contain 64 bytes (got {len})")]
    MalformedKeypairFile { path: String, len: usize },
    #[error("keypair file {path} does not contain a valid Ed25519 keypair: {reason}")]
    InvalidKeypairBytes { path: String, reason: String },
    #[error("connection closed before a full message was received")]
    ConnectionClosed,
    #[error("message of {0} bytes exceeds the maximum frame size")]
    MessageTooLarge(usize),
    #[error("signer backend rejected the request: {0}")]
    Backend(String),
}
