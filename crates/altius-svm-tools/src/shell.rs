use std::path::Path;
use std::process::Command;

use crate::error::ToolError;

pub(crate) struct CommandOutput {
    pub success: bool,
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program args...` in `cwd`, capturing output as text. Does not
/// interpret exit status as an error itself — callers decide whether a
/// non-zero exit is fatal for the operation they're performing (a lint
/// run, for instance, may still want the output from a failing command).
pub(crate) fn run(program: &str, args: &[&str], cwd: &Path) -> Result<CommandOutput, ToolError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ToolError::MissingToolchain(program.to_string())
            } else {
                ToolError::Io(source)
            }
        })?;

    Ok(CommandOutput {
        success: output.status.success(),
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub(crate) fn require_success(
    program: &str,
    args: &[&str],
    output: CommandOutput,
) -> Result<CommandOutput, ToolError> {
    if !output.success {
        return Err(ToolError::CommandFailed {
            program: program.to_string(),
            args: args.join(" "),
            status: output.status_code,
            stderr: output.stderr,
        });
    }
    Ok(output)
}
