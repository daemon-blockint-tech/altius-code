#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("required tool `{0}` was not found on PATH")]
    MissingToolchain(String),
    #[error("`{program} {args}` exited with status {status:?}:\n{stderr}")]
    CommandFailed {
        program: String,
        args: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error(
        "refusing to run `{command}` directly: it {reason}. \
         Route it through altius-txguard::TxGuard::submit instead so it is \
         simulated and approved before anything is signed."
    )]
    InterceptedShellCommand { command: String, reason: String },
    #[error("no build artifacts found under {0}; run build() first")]
    NoBuildArtifacts(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
