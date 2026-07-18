use thiserror::Error;

/// Errors from x402 challenge parsing and payment construction.
#[derive(Debug, Error)]
pub enum PaymentError {
    #[error("invalid 402 challenge: {0}")]
    InvalidChallenge(String),

    #[error("unsupported x402 version {0} (only version 1 is supported)")]
    UnsupportedVersion(u64),

    #[error("no payment requirement in the challenge is supported: {0}")]
    NoSupportedRequirement(String),

    #[error("unsupported payment network {0:?}")]
    UnsupportedNetwork(String),

    #[error("unsupported payment asset {0:?} (only native SOL is supported)")]
    UnsupportedAsset(String),

    #[error("invalid pay-to address: {0}")]
    InvalidPayTo(String),

    #[error("invalid payment amount: {0}")]
    InvalidAmount(String),

    #[error("invalid payment proof: {0}")]
    InvalidProof(String),

    /// The guardrail pipeline rejected or denied the payment. Nothing was
    /// signed; there is deliberately no way to retry around this.
    #[error("payment blocked by TxGuard: {0}")]
    Guard(#[from] altius_txguard::GuardError),

    /// TxGuard approved the payment but had no signer configured, so no
    /// settlement proof can be produced.
    #[error("payment approved but no signer configured; cannot produce settlement proof")]
    NoSigner,
}

pub type PaymentResult<T> = Result<T, PaymentError>;
