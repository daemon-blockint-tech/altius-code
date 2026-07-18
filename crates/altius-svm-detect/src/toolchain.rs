use std::process::Command;

/// Versions of the external tools an SVM project may need, as observed on
/// the current `PATH`. Any field left `None` means the tool was not found;
/// callers are expected to surface an install hint to the user rather than
/// attempt to install it silently (see `install_hint`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toolchain {
    pub solana_cli_version: Option<String>,
    pub anchor_cli_version: Option<String>,
    pub rustc_version: Option<String>,
    pub cargo_build_sbf_available: bool,
}

impl Toolchain {
    /// Probe the host for `solana`, `anchor`, `rustc`, and `cargo build-sbf`.
    /// Never fails: a missing tool is recorded as `None`/`false`, not an
    /// error, since detection must still work in a repo before its
    /// toolchain is installed.
    pub fn probe() -> Toolchain {
        Toolchain {
            solana_cli_version: version_of("solana", &["--version"]),
            anchor_cli_version: version_of("anchor", &["--version"]),
            rustc_version: version_of("rustc", &["--version"]),
            cargo_build_sbf_available: command_succeeds("cargo", &["build-sbf", "--version"]),
        }
    }

    /// Human-readable install instructions for whatever is missing, so the
    /// agent can tell the user exactly what to run instead of guessing.
    pub fn missing_tool_hints(&self) -> Vec<&'static str> {
        let mut hints = Vec::new();
        if self.solana_cli_version.is_none() {
            hints.push(
                "Solana CLI (Agave) not found. Install with: \
                 sh -c \"$(curl -sSfL https://release.anza.xyz/stable/install)\"",
            );
        }
        if self.anchor_cli_version.is_none() {
            hints.push("Anchor CLI not found. Install with: avm install latest && avm use latest");
        }
        if !self.cargo_build_sbf_available {
            hints.push(
                "`cargo build-sbf` not available. It ships with the Solana CLI install above; \
                 ensure `~/.local/share/solana/install/active_release/bin` is on PATH.",
            );
        }
        hints
    }
}

fn version_of(bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = if text.trim().is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        text.into_owned()
    };
    Some(text.trim().to_string())
}

fn command_succeeds(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_panics_when_tools_are_missing() {
        // This just proves probe() degrades gracefully; it does not assert
        // on actual tool presence since CI environments vary.
        let toolchain = Toolchain::probe();
        let _ = toolchain.missing_tool_hints();
    }

    #[test]
    fn missing_hints_are_empty_when_everything_present() {
        let toolchain = Toolchain {
            solana_cli_version: Some("1.18.0".into()),
            anchor_cli_version: Some("0.30.0".into()),
            rustc_version: Some("1.94.0".into()),
            cargo_build_sbf_available: true,
        };
        assert!(toolchain.missing_tool_hints().is_empty());
    }
}
