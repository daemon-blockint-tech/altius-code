use altius_svm_detect::Cluster;
use altius_txguard::TxRequest;

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

    /// Describes the deploy that *would* happen on `cluster` as a
    /// [`TxRequest`]. This method never runs `anchor deploy`, `solana
    /// program deploy`, or anything else that could submit a transaction
    /// — the only way to actually deploy is to pass the returned request
    /// through `altius_txguard::TxGuard::submit`, which simulates and
    /// requires approval first.
    fn deploy(&self, cluster: Cluster) -> Result<TxRequest, ToolError>;
}
