use std::path::Path;
use std::sync::Arc;

use altius_findings::{ChainFamily, ScanReport};

use crate::error::ScannerError;
use crate::scanner::Scanner;

#[derive(Default, Clone)]
pub struct ScannerRegistry {
    scanners: Vec<Arc<dyn Scanner>>,
}

impl ScannerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, scanner: Arc<dyn Scanner>) {
        self.scanners.push(scanner);
    }

    pub fn scanners(&self) -> &[Arc<dyn Scanner>] {
        &self.scanners
    }

    pub fn scan_all(&self, root: &Path) -> Result<ScanReport, ScannerError> {
        let mut report = ScanReport::new(root.display().to_string());
        for scanner in &self.scanners {
            let mut partial = scanner.scan(root)?;
            report.scanners.push(scanner.name().into());
            if report.chain.is_none() {
                report.chain = partial.chain;
            }
            report.extend(partial.findings.drain(..));
        }
        report.dedupe_by_fingerprint();
        Ok(report)
    }

    pub fn scan_chain(&self, root: &Path, chain: ChainFamily) -> Result<ScanReport, ScannerError> {
        let mut report = ScanReport::new(root.display().to_string()).with_chain(chain);
        for scanner in &self.scanners {
            if scanner.chain() != chain {
                continue;
            }
            let mut partial = scanner.scan(root)?;
            report.scanners.push(scanner.name().into());
            report.extend(partial.findings.drain(..));
        }
        report.dedupe_by_fingerprint();
        Ok(report)
    }
}

/// Registry with all enabled feature plugins.
pub fn default_registry() -> ScannerRegistry {
    let mut registry = ScannerRegistry::new();
    #[cfg(feature = "svm")]
    registry.register(Arc::new(crate::svm::SvmScanner));
    #[cfg(feature = "evm")]
    registry.register(Arc::new(crate::evm::EvmScanner));
    #[cfg(feature = "algorand")]
    registry.register(Arc::new(crate::algorand::AlgorandScanner));
    #[cfg(feature = "cairo")]
    registry.register(Arc::new(crate::cairo::CairoScanner));
    #[cfg(feature = "cosmos")]
    registry.register(Arc::new(crate::cosmos::CosmosScanner));
    #[cfg(feature = "ton")]
    registry.register(Arc::new(crate::ton::TonScanner));
    registry
}
