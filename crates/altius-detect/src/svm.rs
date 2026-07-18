use std::path::Path;

use altius_findings::ChainFamily;

use crate::error::DetectError;
use crate::plugin::{DetectPlugin, DetectedProject, DetectionHint};

/// Adapter over [`altius_svm_detect::detect`].
pub struct SvmDetectPlugin;

impl DetectPlugin for SvmDetectPlugin {
    fn name(&self) -> &'static str {
        "altius-svm-detect"
    }

    fn chain(&self) -> ChainFamily {
        ChainFamily::Solana
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedProject>, DetectError> {
        match altius_svm_detect::detect(root) {
            Ok(None) => Ok(None),
            Ok(Some(project)) => {
                let rank = match project.framework {
                    altius_svm_detect::Framework::Anchor => 90,
                    altius_svm_detect::Framework::Pinocchio => 80,
                    altius_svm_detect::Framework::Native => 70,
                };
                Ok(Some(DetectedProject {
                    chain: ChainFamily::Solana,
                    root: root.display().to_string(),
                    rank,
                    plugin: self.name().into(),
                    hints: DetectionHint {
                        framework: Some(project.framework.to_string()),
                        toolchain: Some(format!("{:?}", project.toolchain)),
                        cluster: Some(project.default_cluster.to_string()),
                        programs: project.programs.iter().map(|p| p.name.clone()).collect(),
                        detail: Some(serde_json::json!({
                            "program_ids": project.programs.iter().map(|p| {
                                serde_json::json!({
                                    "name": p.name,
                                    "program_id": p.program_id,
                                })
                            }).collect::<Vec<_>>(),
                        })),
                    },
                }))
            }
            Err(err) => Err(DetectError::Plugin {
                plugin: self.name().into(),
                message: err.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn no_project_returns_none() {
        let dir = tempdir().unwrap();
        let hit = SvmDetectPlugin.detect(dir.path()).unwrap();
        assert!(hit.is_none());
    }
}
