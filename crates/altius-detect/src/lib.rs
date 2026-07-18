//! Chain-agnostic project detection for the Altius security fleet.
//!
//! Plugins return ranked [`DetectedProject`] values. The default registry
//! includes the SVM adapter (`altius-svm-detect`) when the `svm` feature is on.

mod error;
mod plugin;
mod registry;
#[cfg(feature = "svm")]
mod svm;

pub use error::DetectError;
pub use plugin::{DetectPlugin, DetectedProject, DetectionHint};
pub use registry::{detect_all, detect_best, DetectionRegistry};
