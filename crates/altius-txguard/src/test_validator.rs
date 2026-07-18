use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use solana_rpc_client::rpc_client::RpcClient;

use crate::error::GuardError;

/// Configures a [`TestValidator`] run.
pub struct TestValidatorConfig {
    /// `(cluster_rpc_url, pubkey)` accounts to clone into the fresh local
    /// ledger — mirrors `solana-test-validator --url <cluster> --clone
    /// <pubkey>`. This is the local-fork half of the mainnet dual
    /// simulation described in Phase 0 spec §6 stage 2: run a
    /// [`crate::RpcSimulator`] against this validator's RPC URL alongside
    /// one against the real mainnet RPC endpoint, and require both to
    /// succeed.
    pub clone_accounts: Vec<String>,
    /// Cluster RPC url `clone_accounts` are fetched from. Required
    /// whenever `clone_accounts` is non-empty.
    pub clone_from_url: Option<String>,
    pub startup_timeout: Duration,
}

impl Default for TestValidatorConfig {
    fn default() -> Self {
        TestValidatorConfig {
            clone_accounts: Vec::new(),
            clone_from_url: None,
            startup_timeout: Duration::from_secs(30),
        }
    }
}

/// Manages a `solana-test-validator` child process for the lifetime of
/// this value: picks free ports, launches it against a fresh ledger in a
/// temp directory, polls `getHealth` until it accepts RPC calls, and
/// kills it on drop.
///
/// Requires the `solana-test-validator` binary on `PATH` — check
/// `altius_svm_detect::Toolchain::probe().cargo_build_sbf_available` (or
/// just attempt [`TestValidator::start`] and handle
/// [`GuardError::MissingToolchain`]-shaped failure) before relying on
/// this in an environment that may not have the Solana CLI installed.
#[derive(Debug)]
pub struct TestValidator {
    child: Child,
    rpc_port: u16,
    _ledger_dir: tempfile::TempDir,
}

impl TestValidator {
    pub fn start(config: TestValidatorConfig) -> Result<TestValidator, GuardError> {
        let ledger_dir = tempfile::tempdir()?;
        // Binding to port 0 and reading back the assigned port, then
        // releasing it before solana-test-validator binds the same port,
        // is the standard "find a free port" pattern. It has an inherent
        // (very small) race if something else grabs the port in between;
        // acceptable for a local dev/test harness.
        let rpc_port = pick_free_port()?;
        let faucet_port = pick_free_port()?;

        let mut command = Command::new("solana-test-validator");
        command
            .arg("--ledger")
            .arg(ledger_dir.path())
            .arg("--rpc-port")
            .arg(rpc_port.to_string())
            .arg("--faucet-port")
            .arg(faucet_port.to_string())
            .arg("--reset")
            .arg("--quiet")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if !config.clone_accounts.is_empty() {
            let url = config
                .clone_from_url
                .as_deref()
                .ok_or_else(|| GuardError::Rpc {
                    rpc_url: String::new(),
                    reason: "clone_accounts was set without clone_from_url".to_string(),
                })?;
            command.arg("--url").arg(url);
            for pubkey in &config.clone_accounts {
                command.arg("--clone").arg(pubkey);
            }
        }

        let child = command.spawn().map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                GuardError::Rpc {
                    rpc_url: String::new(),
                    reason: "solana-test-validator was not found on PATH".to_string(),
                }
            } else {
                GuardError::Io(source)
            }
        })?;

        let validator = TestValidator {
            child,
            rpc_port,
            _ledger_dir: ledger_dir,
        };
        validator.wait_until_ready(config.startup_timeout)?;
        Ok(validator)
    }

    pub fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    fn wait_until_ready(&self, timeout: Duration) -> Result<(), GuardError> {
        let client = RpcClient::new(self.rpc_url());
        let deadline = Instant::now() + timeout;
        loop {
            if client.get_health().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(GuardError::Rpc {
                    rpc_url: self.rpc_url(),
                    reason: format!(
                        "solana-test-validator did not become healthy within {timeout:?}"
                    ),
                });
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for TestValidator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn pick_free_port() -> Result<u16, GuardError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// True if `solana-test-validator` is on `PATH`. Tests that need a real
/// validator should skip (not fail) when this is `false`, the same way
/// `altius_svm_detect::Toolchain::probe` treats missing tools as data
/// rather than an error.
pub fn is_available() -> bool {
    Command::new("solana-test-validator")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_cleanly_when_solana_test_validator_is_not_installed() {
        if is_available() {
            // This sandbox doesn't ship the Solana CLI, but if a future
            // environment does, exercise the real thing instead of
            // skipping silently.
            let validator = TestValidator::start(TestValidatorConfig::default()).unwrap();
            let client = RpcClient::new(validator.rpc_url());
            assert!(client.get_health().is_ok());
            return;
        }

        let err = TestValidator::start(TestValidatorConfig::default()).unwrap_err();
        assert!(matches!(err, GuardError::Rpc { .. }));
    }
}
