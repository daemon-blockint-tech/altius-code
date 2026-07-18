use std::path::PathBuf;

use crate::cluster::Cluster;
use crate::framework::Framework;
use crate::toolchain::Toolchain;

/// A single on-chain program found in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    pub name: String,
    pub path: PathBuf,
    /// Present when the manifest declares a program id (Anchor.toml, or a
    /// `declare_id!` scrape for native/Pinocchio — see `detect.rs`).
    pub program_id: Option<String>,
}

/// The result of running [`crate::detect`] against a directory: what kind
/// of SVM project it is, which programs it contains, what tooling is
/// available to build/test it, and which cluster it targets by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvmProject {
    pub framework: Framework,
    pub programs: Vec<ProgramInfo>,
    pub toolchain: Toolchain,
    pub default_cluster: Cluster,
}
