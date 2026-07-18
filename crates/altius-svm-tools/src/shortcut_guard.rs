//! Blocks the raw-shell shortcuts a project's `SvmToolchain::deploy`
//! deliberately doesn't take: an agent could still try to run
//! `solana program deploy ...` (or similar) directly as a shell command
//! instead of going through this crate's typed API. This module gives
//! whatever runs shell commands on the agent's behalf a cheap way to
//! recognize those commands and redirect them through
//! `altius-txguard::TxGuard` instead of executing them as-is.
//!
//! This is a defense-in-depth measure, not the guardrail itself — the
//! guardrail's real backstop is that `altius-signer` never hands out key
//! material to anything but its own IPC protocol (see Phase 0 spec §7).
//! Intercepting shell shortcuts here just means a bypass attempt gets a
//! clear redirect instead of silently reaching a CLI that could sign and
//! submit on its own.

use regex::Regex;
use std::sync::OnceLock;

use crate::error::ToolError;

/// Which kind of on-chain-affecting shell command was matched, and why it
/// is intercepted rather than run directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptedKind {
    SolanaProgramDeploy,
    SolanaTransfer,
    AnchorDeployOrUpgrade,
}

impl InterceptedKind {
    fn reason(self) -> &'static str {
        match self {
            InterceptedKind::SolanaProgramDeploy => "writes a program buffer or deploys it",
            InterceptedKind::SolanaTransfer => "moves funds between accounts",
            InterceptedKind::AnchorDeployOrUpgrade => {
                "deploys, upgrades, or migrates an Anchor program"
            }
        }
    }
}

struct Pattern {
    kind: InterceptedKind,
    regex: &'static OnceLock<Regex>,
    source: &'static str,
}

static SOLANA_PROGRAM_DEPLOY: OnceLock<Regex> = OnceLock::new();
static SOLANA_TRANSFER: OnceLock<Regex> = OnceLock::new();
static ANCHOR_DEPLOY: OnceLock<Regex> = OnceLock::new();

fn patterns() -> [Pattern; 3] {
    [
        Pattern {
            kind: InterceptedKind::SolanaProgramDeploy,
            regex: &SOLANA_PROGRAM_DEPLOY,
            source: r"\bsolana\s+program\s+(deploy|write-buffer)\b",
        },
        Pattern {
            kind: InterceptedKind::SolanaTransfer,
            regex: &SOLANA_TRANSFER,
            source: r"\bsolana\s+transfer\b",
        },
        Pattern {
            kind: InterceptedKind::AnchorDeployOrUpgrade,
            regex: &ANCHOR_DEPLOY,
            source: r"\banchor\s+(deploy|upgrade|migrate)\b",
        },
    ]
}

/// Returns the kind of intercepted command `command` matches, if any.
pub fn classify(command: &str) -> Option<InterceptedKind> {
    for pattern in patterns() {
        let regex = pattern
            .regex
            .get_or_init(|| Regex::new(pattern.source).unwrap());
        if regex.is_match(command) {
            return Some(pattern.kind);
        }
    }
    None
}

/// Convenience wrapper: returns `Err(ToolError::InterceptedShellCommand)`
/// if `command` should be redirected through `TxGuard`, `Ok(())`
/// otherwise. Callers that shell out on the agent's behalf should run
/// this check before `Command::new(...).spawn()`.
pub fn guard(command: &str) -> Result<(), ToolError> {
    if let Some(kind) = classify(command) {
        return Err(ToolError::InterceptedShellCommand {
            command: command.to_string(),
            reason: kind.reason().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intercepts_solana_program_deploy_and_write_buffer() {
        assert_eq!(
            classify("solana program deploy target/deploy/my_program.so"),
            Some(InterceptedKind::SolanaProgramDeploy)
        );
        assert_eq!(
            classify("solana program write-buffer target/deploy/my_program.so"),
            Some(InterceptedKind::SolanaProgramDeploy)
        );
    }

    #[test]
    fn intercepts_solana_transfer() {
        assert_eq!(
            classify("solana transfer --allow-unfunded-recipient DEST 1"),
            Some(InterceptedKind::SolanaTransfer)
        );
    }

    #[test]
    fn intercepts_anchor_deploy_upgrade_migrate() {
        assert_eq!(
            classify("anchor deploy --provider.cluster devnet"),
            Some(InterceptedKind::AnchorDeployOrUpgrade)
        );
        assert_eq!(
            classify("anchor upgrade target/deploy/my_program.so"),
            Some(InterceptedKind::AnchorDeployOrUpgrade)
        );
        assert_eq!(
            classify("anchor migrate"),
            Some(InterceptedKind::AnchorDeployOrUpgrade)
        );
    }

    #[test]
    fn does_not_intercept_benign_commands() {
        assert_eq!(classify("anchor build"), None);
        assert_eq!(classify("anchor test"), None);
        assert_eq!(classify("solana balance"), None);
        assert_eq!(classify("cargo build-sbf"), None);
    }

    #[test]
    fn guard_returns_actionable_error() {
        let err = guard("solana program deploy foo.so").unwrap_err();
        match err {
            ToolError::InterceptedShellCommand { command, reason } => {
                assert!(command.contains("deploy"));
                assert!(reason.contains("deploys"));
            }
            other => panic!("expected InterceptedShellCommand, got {other:?}"),
        }
    }
}
