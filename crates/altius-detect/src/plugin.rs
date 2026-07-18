use std::path::Path;

use altius_findings::ChainFamily;
use serde::{Deserialize, Serialize};

use crate::error::DetectError;

/// Extra structured hints produced by a detection plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DetectionHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Ranked detection result from a single plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedProject {
    pub chain: ChainFamily,
    pub root: String,
    /// Higher is better. Typical native detectors use 50–100.
    pub rank: u32,
    pub plugin: String,
    #[serde(default)]
    pub hints: DetectionHint,
}

/// Read-only detection plugin.
pub trait DetectPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn chain(&self) -> ChainFamily;
    fn detect(&self, root: &Path) -> Result<Option<DetectedProject>, DetectError>;
}
