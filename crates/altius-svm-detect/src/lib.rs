//! Detects which SVM program framework (Anchor, Pinocchio, or native Rust)
//! a workspace uses, and what toolchain is available to build it.
//!
//! See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §3 in the repo root for
//! the design this crate implements.

mod anchor_manifest;
mod cluster;
mod detect;
mod error;
mod framework;
mod project;
mod toolchain;

pub use cluster::{Cluster, ParseClusterError};
pub use detect::detect;
pub use error::DetectError;
pub use framework::Framework;
pub use project::{ProgramInfo, SvmProject};
pub use toolchain::Toolchain;
