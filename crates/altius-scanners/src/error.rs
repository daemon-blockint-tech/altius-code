use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScannerError {
    #[error("io error: {0}")]
    Io(String),
    #[error("scanner `{scanner}` failed: {message}")]
    Failed { scanner: String, message: String },
}
