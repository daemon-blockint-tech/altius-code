//! Plugin pack v0: a small JSON manifest that bundles slash skills + MCP attach.
//!
//! This is not a marketplace. Install = place a file and point
//! `altius fleet serve --plugin <path>` at it.

use std::path::{Path, PathBuf};

use altius_mcp::McpAttachConfig;
use serde::{Deserialize, Serialize};

use crate::error::CliError;

/// Versioned plugin pack (v0).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginPack {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// Slash skill names this pack advertises (`scan`, `browser`, …).
    #[serde(default)]
    pub skills: Vec<String>,
    /// Optional MCP child-process attachments.
    #[serde(default)]
    pub mcp: Vec<PluginMcpAttach>,
    /// Optional path to an ontology fragment (reserved; unused in v0).
    #[serde(default)]
    pub ontology: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginMcpAttach {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_version() -> String {
    "0.1.0".into()
}

impl PluginPack {
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| CliError::message(format!("read plugin {}: {e}", path.display())))?;
        let pack: Self = serde_json::from_str(&raw)
            .map_err(|e| CliError::message(format!("invalid plugin JSON: {e}")))?;
        if pack.name.trim().is_empty() {
            return Err(CliError::message("plugin.name must be non-empty"));
        }
        Ok(pack)
    }

    pub fn mcp_configs(&self) -> Vec<McpAttachConfig> {
        self.mcp
            .iter()
            .map(|m| McpAttachConfig {
                name: m.name.clone(),
                command: m.cmd.clone(),
                args: m.args.clone(),
                working_directory: None,
                env_extras: Vec::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[test]
    fn loads_minimal_pack() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{"name":"demo","skills":["scan"],"mcp":[{{"name":"browser","cmd":"npx","args":["@playwright/mcp@latest"]}}]}}"#
        )
        .unwrap();
        let pack = PluginPack::load(file.path()).unwrap();
        assert_eq!(pack.name, "demo");
        assert_eq!(pack.skills, vec!["scan"]);
        assert_eq!(pack.mcp_configs().len(), 1);
        assert_eq!(pack.mcp_configs()[0].name, "browser");
    }

    #[test]
    fn loads_web3_starter_example() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/plugins/web3-starter.json");
        let pack = PluginPack::load(&path).unwrap();
        assert_eq!(pack.name, "altius-web3-starter");
        assert!(pack.skills.contains(&"scan".into()));
        assert!(pack.mcp.is_empty());
    }
}
