use std::path::Path;
use std::sync::Arc;

use crate::error::DetectError;
use crate::plugin::{DetectPlugin, DetectedProject};

/// Ordered registry of detection plugins.
#[derive(Default, Clone)]
pub struct DetectionRegistry {
    plugins: Vec<Arc<dyn DetectPlugin>>,
}

impl DetectionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Arc<dyn DetectPlugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugins(&self) -> &[Arc<dyn DetectPlugin>] {
        &self.plugins
    }

    /// Default registry with built-in plugins for enabled features.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        #[cfg(feature = "svm")]
        {
            registry.register(Arc::new(crate::svm::SvmDetectPlugin));
        }
        registry.register(Arc::new(crate::chains::EvmDetectPlugin));
        registry.register(Arc::new(crate::chains::AlgorandDetectPlugin));
        registry.register(Arc::new(crate::chains::CairoDetectPlugin));
        registry.register(Arc::new(crate::chains::CosmosDetectPlugin));
        registry.register(Arc::new(crate::chains::TonDetectPlugin));
        registry
    }
}

/// Run every plugin and return all non-empty detections, highest rank first.
pub fn detect_all(
    registry: &DetectionRegistry,
    root: &Path,
) -> Result<Vec<DetectedProject>, DetectError> {
    let mut hits = Vec::new();
    for plugin in registry.plugins() {
        match plugin.detect(root) {
            Ok(Some(project)) => hits.push(project),
            Ok(None) => {}
            Err(err) => {
                return Err(DetectError::Plugin {
                    plugin: plugin.name().into(),
                    message: err.to_string(),
                });
            }
        }
    }
    hits.sort_by(|a, b| b.rank.cmp(&a.rank).then(a.plugin.cmp(&b.plugin)));
    Ok(hits)
}

/// Return the single best detection, or `None` if no plugin matched.
pub fn detect_best(
    registry: &DetectionRegistry,
    root: &Path,
) -> Result<Option<DetectedProject>, DetectError> {
    Ok(detect_all(registry, root)?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::DetectionHint;
    use altius_findings::ChainFamily;
    use std::sync::Mutex;

    struct FakePlugin {
        name: &'static str,
        chain: ChainFamily,
        rank: u32,
        hit: bool,
    }

    impl DetectPlugin for FakePlugin {
        fn name(&self) -> &'static str {
            self.name
        }
        fn chain(&self) -> ChainFamily {
            self.chain
        }
        fn detect(&self, root: &Path) -> Result<Option<DetectedProject>, DetectError> {
            if !self.hit {
                return Ok(None);
            }
            Ok(Some(DetectedProject {
                chain: self.chain,
                root: root.display().to_string(),
                rank: self.rank,
                plugin: self.name.into(),
                hints: DetectionHint::default(),
            }))
        }
    }

    #[test]
    fn ranks_highest_first() {
        let mut registry = DetectionRegistry::new();
        registry.register(Arc::new(FakePlugin {
            name: "low",
            chain: ChainFamily::Evm,
            rank: 10,
            hit: true,
        }));
        registry.register(Arc::new(FakePlugin {
            name: "high",
            chain: ChainFamily::Solana,
            rank: 90,
            hit: true,
        }));
        let hits = detect_all(&registry, Path::new(".")).unwrap();
        assert_eq!(hits[0].plugin, "high");
        assert_eq!(hits[1].plugin, "low");
    }

    #[test]
    fn conflict_counter_tracks_multi_hits() {
        // Documented behavior: multiple hits are allowed and ranked; callers
        // decide whether to treat same-root multi-chain as a conflict.
        let mut registry = DetectionRegistry::new();
        registry.register(Arc::new(FakePlugin {
            name: "a",
            chain: ChainFamily::Solana,
            rank: 50,
            hit: true,
        }));
        registry.register(Arc::new(FakePlugin {
            name: "b",
            chain: ChainFamily::Evm,
            rank: 50,
            hit: true,
        }));
        let hits = detect_all(&registry, Path::new(".")).unwrap();
        assert_eq!(hits.len(), 2);
        let _ = Mutex::new(hits);
    }
}
