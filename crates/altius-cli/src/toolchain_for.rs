use std::path::Path;

use altius_svm_detect::Framework;
use altius_svm_tools::{AnchorToolchain, CargoBuildSbfToolchain, SvmToolchain};

/// Picks the `SvmToolchain` adapter matching a detected project's
/// framework. Pinocchio and native crates share the same
/// `cargo build-sbf`-based adapter — see Phase 0 spec §4.
pub fn toolchain_for(framework: Framework, project_root: &Path) -> Box<dyn SvmToolchain> {
    match framework {
        Framework::Anchor => Box::new(AnchorToolchain::new(project_root)),
        Framework::Pinocchio | Framework::Native => {
            Box::new(CargoBuildSbfToolchain::new(project_root))
        }
    }
}
