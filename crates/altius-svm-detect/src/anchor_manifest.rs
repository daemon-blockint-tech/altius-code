//! Minimal parsing of `Anchor.toml`, covering only the fields Altius needs
//! to build an [`crate::SvmProject`]. Unknown sections are ignored rather
//! than rejected so this stays forward-compatible with newer Anchor.toml
//! layouts.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct AnchorToml {
    #[serde(default)]
    pub programs: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub provider: Option<Provider>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Provider {
    pub cluster: Option<String>,
    /// Recorded but intentionally never opened as a file anywhere in this
    /// crate — see altius-signer's key isolation guardrail. Kept `pub`
    /// (not just parsed and discarded) so callers can pass the path along
    /// without this crate ever reading it.
    #[allow(dead_code)]
    pub wallet: Option<String>,
}

impl AnchorToml {
    pub(crate) fn parse(contents: &str) -> Result<AnchorToml, toml::de::Error> {
        toml::from_str(contents)
    }

    /// Program entries for the currently configured cluster's section, or
    /// the first section found if the manifest doesn't key programs by
    /// cluster (older Anchor.toml layout: `[programs.localnet]` etc. is one
    /// level, but some manifests key directly under `[programs]`).
    pub(crate) fn program_entries(&self) -> Vec<(String, String)> {
        // `[programs.<cluster>]` nests one level; `self.programs` here maps
        // cluster name -> (program name -> program id). Flatten across all
        // clusters, de-duplicating by program name (first occurrence wins).
        let mut seen = BTreeMap::new();
        for cluster_table in self.programs.values() {
            for (name, id) in cluster_table {
                seen.entry(name.clone()).or_insert_with(|| id.clone());
            }
        }
        seen.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_programs_and_provider() {
        let toml = r#"
            [features]
            seeds = false

            [programs.localnet]
            my_program = "Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"

            [provider]
            cluster = "localnet"
            wallet = "~/.config/solana/id.json"
        "#;
        let parsed = AnchorToml::parse(toml).unwrap();
        assert_eq!(
            parsed.program_entries(),
            vec![(
                "my_program".to_string(),
                "Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS".to_string()
            )]
        );
        assert_eq!(
            parsed.provider.unwrap().cluster.as_deref(),
            Some("localnet")
        );
    }

    #[test]
    fn tolerates_missing_sections() {
        let parsed = AnchorToml::parse("").unwrap();
        assert!(parsed.program_entries().is_empty());
        assert!(parsed.provider.is_none());
    }
}
