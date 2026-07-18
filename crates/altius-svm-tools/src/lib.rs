//! Uniform build/test/lint/deploy adapters for Anchor, Pinocchio, and
//! native Rust SVM programs, plus defense-in-depth: a shell-shortcut
//! interceptor toward `altius-txguard`, native heuristic security lints
//! (with evidence spans), optional clippy/cargo-audit adapters, and a
//! local-only dynamic validation interface.
//!
//! See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §4 in the repo root
//! for the design this crate implements.

pub mod adapters;
mod anchor;
mod deploy_plan;
pub mod dynamic;
mod error;
mod lints;
mod native;
mod report;
mod shell;
pub mod shortcut_guard;
mod toolchain_trait;

pub use adapters::{run_cargo_audit, run_clippy, AdapterStatus, CargoAuditResult, ClippyResult};
pub use anchor::AnchorToolchain;
pub use deploy_plan::{
    build_deployment_plan, load_or_generate_program_keypair, DeploymentPlan, WRITE_CHUNK_SIZE,
};
pub use dynamic::{
    DynamicCase, DynamicCaseResult, DynamicError, DynamicReport, DynamicScanner,
    LocalSequenceHarness, OptionalTridentAdapter,
};
pub use error::ToolError;
pub use native::CargoBuildSbfToolchain;
pub use report::{
    parse_cargo_test_output, BuildArtifacts, LintFinding, LintReport, Severity, TestCaseResult,
    TestReport,
};
pub use toolchain_trait::SvmToolchain;
