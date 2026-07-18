use std::path::Path;

use altius_findings::{ChainFamily, ScanReport};

use crate::error::ScannerError;

/// Read-only scanner that emits canonical findings.
pub trait Scanner: Send + Sync {
    fn name(&self) -> &'static str;
    fn chain(&self) -> ChainFamily;
    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError>;
}
