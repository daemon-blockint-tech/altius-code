use altius_svm_detect::Cluster;
use solana_hash::Hash;
use solana_pubkey::Pubkey;

use crate::deploy_plan::DeploymentPlan;
use crate::error::ToolError;
use crate::report::{BuildArtifacts, LintReport, TestReport};

/// Uniform build/test/lint/deploy surface across SVM frameworks (Anchor,
/// Pinocchio, native Rust), so the agent drives any of them the same way
/// regardless of which framework a project uses.
///
/// See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §4 in the repo root.
pub trait SvmToolchain {
    fn build(&self) -> Result<BuildArtifacts, ToolError>;
    fn unit_test(&self) -> Result<TestReport, ToolError>;
    fn integration_test(&self) -> Result<TestReport, ToolError>;
    fn lint(&self) -> Result<LintReport, ToolError>;

    /// Builds the ordered sequence of transactions that *would* deploy
    /// (or, if `is_upgrade`, redeploy) the program on `cluster` as a
    /// [`DeploymentPlan`]. This method never runs `anchor deploy`,
    /// `solana program deploy`, or anything else that could submit a
    /// transaction — every entry in the plan still has to pass through
    /// `altius_txguard::TxGuard::submit`, in order, which simulates and
    /// requires approval before anything is signed and sent.
    fn deploy(
        &self,
        cluster: Cluster,
        payer: Pubkey,
        recent_blockhash: Hash,
        is_upgrade: bool,
    ) -> Result<DeploymentPlan, ToolError>;
}
