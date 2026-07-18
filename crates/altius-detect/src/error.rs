use thiserror::Error;

#[derive(Debug, Error)]
pub enum DetectError {
    #[error("io error: {0}")]
    Io(String),
    #[error("plugin `{plugin}` failed: {message}")]
    Plugin { plugin: String, message: String },
    #[error("conflicting detections for the same root")]
    Conflict,
}
