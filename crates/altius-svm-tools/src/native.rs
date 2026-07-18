use std::fs;
use std::path::PathBuf;

use altius_svm_detect::Cluster;
use altius_txguard::{TxKind, TxRequest};

use crate::error::ToolError;
use crate::lints;
use crate::report::{parse_cargo_test_output, BuildArtifacts, LintReport, TestReport};
use crate::shell::{require_success, run};
use crate::toolchain_trait::SvmToolchain;

/// Drives Pinocchio and plain native (`solana-program`) crates. Both
/// build via `cargo build-sbf` and have no Anchor-style IDL, so they
/// share one implementation. See Phase 0 spec §4.
pub struct CargoBuildSbfToolchain {
    project_root: PathBuf,
}

impl CargoBuildSbfToolchain {
    pub fn new(project_root: impl Into<PathBuf>) -> CargoBuildSbfToolchain {
        CargoBuildSbfToolchain {
            project_root: project_root.into(),
        }
    }

    fn deploy_dir(&self) -> PathBuf {
        self.project_root.join("target").join("deploy")
    }

    fn collect_artifacts(&self) -> Result<BuildArtifacts, ToolError> {
        let deploy_dir = self.deploy_dir();
        let mut program_paths = Vec::new();
        if deploy_dir.is_dir() {
            for entry in fs::read_dir(&deploy_dir)? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("so") {
                    program_paths.push(path);
                }
            }
        }
        if program_paths.is_empty() {
            return Err(ToolError::NoBuildArtifacts(
                deploy_dir.display().to_string(),
            ));
        }
        program_paths.sort();
        Ok(BuildArtifacts {
            program_paths,
            idl_path: None,
        })
    }
}

impl SvmToolchain for CargoBuildSbfToolchain {
    fn build(&self) -> Result<BuildArtifacts, ToolError> {
        let output = run("cargo", &["build-sbf"], &self.project_root)?;
        require_success("cargo", &["build-sbf"], output)?;
        self.collect_artifacts()
    }

    /// `cargo test --lib`: only the crate's own inline `#[cfg(test)]`
    /// unit tests, no validator involved.
    fn unit_test(&self) -> Result<TestReport, ToolError> {
        let output = run("cargo", &["test", "--lib"], &self.project_root)?;
        Ok(TestReport {
            cases: parse_cargo_test_output(&output.stdout),
            logs: vec![],
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        })
    }

    /// `cargo test --tests`: runs the integration test binaries under
    /// `tests/` (typically built on `solana-program-test`/bankrun),
    /// excluding the crate's own lib unit tests.
    fn integration_test(&self) -> Result<TestReport, ToolError> {
        let output = run("cargo", &["test", "--tests"], &self.project_root)?;
        Ok(TestReport {
            cases: parse_cargo_test_output(&output.stdout),
            logs: vec![],
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        })
    }

    fn lint(&self) -> Result<LintReport, ToolError> {
        lints::run_all(&self.project_root)
    }

    fn deploy(&self, cluster: Cluster) -> Result<TxRequest, ToolError> {
        let artifacts = self.collect_artifacts()?;
        let program_path = artifacts
            .program_paths
            .first()
            .ok_or_else(|| ToolError::NoBuildArtifacts(self.deploy_dir().display().to_string()))?;

        Ok(TxRequest {
            description: format!("deploy {} to {cluster}", program_path.display()),
            cluster,
            kind: TxKind::Deploy,
            // See the same note in anchor.rs: a real transaction payload
            // is follow-up work once solana-sdk is wired in.
            unsigned_transaction: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_fails_clearly_without_prior_build() {
        let dir = tempfile::tempdir().unwrap();
        let toolchain = CargoBuildSbfToolchain::new(dir.path());
        let err = toolchain.deploy(Cluster::Devnet).unwrap_err();
        assert!(matches!(err, ToolError::NoBuildArtifacts(_)));
    }

    #[test]
    fn deploy_describes_the_first_artifact_once_built() {
        let dir = tempfile::tempdir().unwrap();
        let deploy_dir = dir.path().join("target").join("deploy");
        fs::create_dir_all(&deploy_dir).unwrap();
        fs::write(deploy_dir.join("native_program.so"), b"not a real elf").unwrap();

        let toolchain = CargoBuildSbfToolchain::new(dir.path());
        let tx = toolchain.deploy(Cluster::Localnet).unwrap();
        assert_eq!(tx.kind, TxKind::Deploy);
        assert!(tx.description.contains("native_program.so"));
    }
}
