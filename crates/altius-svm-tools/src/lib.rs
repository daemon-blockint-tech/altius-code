//! Uniform build/test/lint/deploy adapters for Anchor, Pinocchio, and
//! native Rust SVM programs, plus two defense-in-depth pieces: a
//! shell-shortcut interceptor that redirects raw deploy/transfer-shaped
//! commands toward `altius-txguard`, and six heuristic security lints.
//!
//! See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §4 in the repo root
//! for the design this crate implements.

mod anchor;
mod deploy_plan;
mod error;
mod lints;
mod native;
mod report;
mod shell;
pub mod shortcut_guard;
mod toolchain_trait;

pub use anchor::AnchorToolchain;
pub use deploy_plan::{
    build_deployment_plan, load_or_generate_program_keypair, DeploymentPlan, WRITE_CHUNK_SIZE,
};
pub use error::ToolError;
pub use native::CargoBuildSbfToolchain;
pub use report::{
    parse_cargo_test_output, BuildArtifacts, LintFinding, LintReport, Severity, TestCaseResult,
    TestReport,
};
pub use toolchain_trait::SvmToolchain;
