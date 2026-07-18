use std::path::Path;
use std::process::Command;

use super::AdapterStatus;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoAuditResult {
    pub status: AdapterStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Run `cargo audit` when installed. Missing binary → `Unavailable`.
pub fn run_cargo_audit(project_root: &Path) -> CargoAuditResult {
    let output = Command::new("cargo")
        .args(["audit", "--json"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = bound_str(&String::from_utf8_lossy(&out.stdout));
            let stderr = bound_str(&String::from_utf8_lossy(&out.stderr));
            // cargo-audit exits non-zero when vulns are found; that is still Ok
            // from an adapter perspective (tool ran). Distinguish launch failure
            // via stderr containing "no such command".
            let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
            if combined.contains("no such command") || combined.contains("is not installed") {
                return CargoAuditResult {
                    status: AdapterStatus::Unavailable {
                        tool: "cargo-audit".into(),
                        detail: "cargo audit subcommand not installed".into(),
                    },
                    stdout,
                    stderr,
                };
            }
            CargoAuditResult {
                status: AdapterStatus::Ok,
                stdout,
                stderr,
            }
        }
        Err(error) => CargoAuditResult {
            status: AdapterStatus::Unavailable {
                tool: "cargo-audit".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_shape() {
        let r = CargoAuditResult {
            status: AdapterStatus::Unavailable {
                tool: "cargo-audit".into(),
                detail: "missing".into(),
            },
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(matches!(r.status, AdapterStatus::Unavailable { .. }));
    }
}
