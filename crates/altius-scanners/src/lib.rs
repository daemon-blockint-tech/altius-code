//! Native multi-chain security scanners for Altius.
//!
//! Each chain plugin is feature-gated so default CI stays offline and fast.
//! External tools (Wake, Tealer, Caracal, Trident) are optional executables —
//! never source dependencies.

mod error;
mod registry;
mod scanner;
mod util;

#[cfg(feature = "svm")]
pub mod svm;
#[cfg(feature = "evm")]
pub mod evm;
#[cfg(feature = "algorand")]
pub mod algorand;
#[cfg(feature = "cairo")]
pub mod cairo;
#[cfg(feature = "cosmos")]
pub mod cosmos;
#[cfg(feature = "ton")]
pub mod ton;

pub use error::ScannerError;
pub use registry::{default_registry, ScannerRegistry};
pub use scanner::Scanner;
