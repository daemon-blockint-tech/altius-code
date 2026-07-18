use std::fs;
use std::path::PathBuf;

use altius_svm_detect::Cluster;
use solana_hash::Hash;
use solana_pubkey::Pubkey;

use crate::deploy_plan::{build_deployment_plan, load_or_generate_program_keypair, DeploymentPlan};
use crate::error::ToolError;
use crate::lints;
use crate::report::{parse_cargo_test_output, BuildArtifacts, LintReport, TestReport};
use crate::shell::{require_success, run};
use crate::toolchain_trait::SvmToolchain;

/// Drives an Anchor workspace: `anchor build`/`anchor test`, plus this
/// crate's own lints. See Phase 0 spec §4.
pub struct AnchorToolchain {
    project_root: PathBuf,
}

impl AnchorToolchain {
    pub fn new(project_root: impl Into<PathBuf>) -> AnchorToolchain {
        AnchorToolchain {
            project_root: project_root.into(),
        }
    }

    fn deploy_dir(&self) -> PathBuf {
        self.project_root.join("target").join("deploy")
    }

    fn idl_dir(&self) -> PathBuf {
        self.project_root.join("target").join("idl")
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

        let idl_dir = self.idl_dir();
        let idl_path = if idl_dir.is_dir() {
            fs::read_dir(&idl_dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .find(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        } else {
            None
        };

        Ok(BuildArtifacts {
            program_paths,
            idl_path,
        })
    }
}

impl SvmToolchain for AnchorToolchain {
    fn build(&self) -> Result<BuildArtifacts, ToolError> {
        let output = run("anchor", &["build"], &self.project_root)?;
        require_success("anchor", &["build"], output)?;
        self.collect_artifacts()
    }

    /// Fast, no-validator tests: only the workspace's own `#[cfg(test)]`
    /// unit tests (`cargo test --lib`), the kind LiteSVM/Mollusk-style
    /// tests are written as.
    fn unit_test(&self) -> Result<TestReport, ToolError> {
        let output = run("cargo", &["test", "--lib"], &self.project_root)?;
        Ok(TestReport {
            cases: parse_cargo_test_output(&output.stdout),
            logs: vec![],
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        })
    }

    /// `anchor test` manages its own localnet validator lifecycle.
    fn integration_test(&self) -> Result<TestReport, ToolError> {
        let output = run("anchor", &["test"], &self.project_root)?;
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

    fn deploy(
        &self,
        cluster: Cluster,
        payer: Pubkey,
        recent_blockhash: Hash,
        is_upgrade: bool,
    ) -> Result<DeploymentPlan, ToolError> {
        let artifacts = self.collect_artifacts()?;
        let program_path = artifacts
            .program_paths
            .first()
            .ok_or_else(|| ToolError::NoBuildArtifacts(self.deploy_dir().display().to_string()))?;

        let program_bytes = fs::read(program_path)?;
        let program_name = program_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("program");
        let program_keypair = load_or_generate_program_keypair(&self.deploy_dir(), program_name)?;

        build_deployment_plan(
            &program_bytes,
            payer,
            &program_keypair,
            program_bytes.len() * 2,
            cluster,
            recent_blockhash,
            is_upgrade,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_fails_clearly_without_prior_build() {
        let dir = tempfile::tempdir().unwrap();
        let toolchain = AnchorToolchain::new(dir.path());
        let err = toolchain
            .deploy(
                Cluster::Devnet,
                Pubkey::new_unique(),
                Hash::default(),
                false,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NoBuildArtifacts(_)));
    }

    #[test]
    fn deploy_describes_the_first_artifact_once_built() {
        let dir = tempfile::tempdir().unwrap();
        let deploy_dir = dir.path().join("target").join("deploy");
        fs::create_dir_all(&deploy_dir).unwrap();
        fs::write(deploy_dir.join("my_program.so"), vec![0u8; 128]).unwrap();

        let toolchain = AnchorToolchain::new(dir.path());
        let payer = Pubkey::new_unique();
        let plan = toolchain
            .deploy(Cluster::Devnet, payer, Hash::default(), false)
            .unwrap();

        assert!(plan.create_buffer.description.contains("buffer"));
        assert!(plan.finalize.description.contains("devnet"));
        assert!(!plan.write_chunks.is_empty());

        // Deploying the same program again should reuse the persisted
        // program keypair rather than minting a new program address.
        let plan_again = toolchain
            .deploy(Cluster::Devnet, payer, Hash::default(), true)
            .unwrap();
        assert_eq!(plan.program_pubkey, plan_again.program_pubkey);
    }
}
