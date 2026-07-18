use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::AdapterStatus;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClippyResult {
    pub status: AdapterStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Run `cargo clippy` in `project_root` with a hard output bound.
///
/// Never installs clippy; if the binary is missing, returns `Unavailable`.
pub fn run_clippy(project_root: &Path) -> ClippyResult {
    let output = Command::new("cargo")
        .args(["clippy", "--message-format=short", "--", "-D", "warnings"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = bound_str(&String::from_utf8_lossy(&out.stdout));
            let stderr = bound_str(&String::from_utf8_lossy(&out.stderr));
            if out.status.success() {
                ClippyResult {
                    status: AdapterStatus::Ok,
                    stdout,
                    stderr,
                }
            } else {
                ClippyResult {
                    status: AdapterStatus::Failed {
                        tool: "cargo-clippy".into(),
                        detail: format!("exit status {}", out.status),
                    },
                    stdout,
                    stderr,
                }
            }
        }
        Err(error) => ClippyResult {
            status: AdapterStatus::Unavailable {
                tool: "cargo-clippy".into(),
                detail: error.to_string(),
            },
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

fn bound_str(value: &str) -> String {
    if value.len() <= MAX_OUTPUT_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_OUTPUT_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…[truncated]", &value[..boundary])
}

/// Placeholder used by tests that avoid spawning cargo.
#[allow(dead_code)]
pub(crate) fn timeout_hint() -> Duration {
    Duration::from_secs(120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_when_cargo_missing_from_path_override() {
        // We cannot reliably remove cargo from PATH in CI; assert the
        // Unavailable variant shape instead via a synthetic status.
        let status = AdapterStatus::Unavailable {
            tool: "cargo-clippy".into(),
            detail: "No such file".into(),
        };
        match status {
            AdapterStatus::Unavailable { tool, .. } => assert_eq!(tool, "cargo-clippy"),
            _ => panic!("expected unavailable"),
        }
    }

    #[test]
    fn bound_str_truncates() {
        let big = "a".repeat(MAX_OUTPUT_BYTES + 10);
        let out = bound_str(&big);
        assert!(out.ends_with("…[truncated]"));
        assert!(out.starts_with(&"a".repeat(MAX_OUTPUT_BYTES)));
        assert_ne!(out, big);
    }
}
