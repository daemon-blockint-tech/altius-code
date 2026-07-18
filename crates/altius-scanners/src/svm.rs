use std::path::Path;

use altius_findings::{ChainFamily, ScanReport};
use altius_svm_detect::{detect, Framework};
use altius_svm_tools::{AnchorToolchain, CargoBuildSbfToolchain, SvmToolchain};

use crate::error::ScannerError;
use crate::scanner::Scanner;

pub struct SvmScanner;

impl Scanner for SvmScanner {
    fn name(&self) -> &'static str {
        "altius-svm-native"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Solana
    }

    fn scan(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let detected = detect(root).map_err(|e| ScannerError::Failed {
            scanner: self.name().into(),
            message: e.to_string(),
        })?;
        let Some(project) = detected else {
            return Ok(ScanReport::new(root.display().to_string()).with_chain(ChainFamily::Solana));
        };
        let toolchain: Box<dyn SvmToolchain> = match project.framework {
            Framework::Anchor => Box::new(AnchorToolchain::new(root)),
            Framework::Pinocchio | Framework::Native => Box::new(CargoBuildSbfToolchain::new(root)),
        };
        let lint = toolchain.lint().map_err(|e| ScannerError::Failed {
            scanner: self.name().into(),
            message: e.to_string(),
        })?;
        Ok(lint.to_scan_report(root.display().to_string()))
    }
}
