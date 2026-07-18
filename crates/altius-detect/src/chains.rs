//! Lightweight chain detectors based on manifests / extensions.
//! Enabled always so multi-chain routing works without scanner features.

use std::path::Path;

use altius_findings::ChainFamily;

use crate::error::DetectError;
use crate::plugin::{DetectPlugin, DetectedProject, DetectionHint};

macro_rules! simple_plugin {
    ($name:ident, $plugin:expr, $chain:expr, $rank:expr, $probe:ident) => {
        pub struct $name;
        impl DetectPlugin for $name {
            fn name(&self) -> &'static str {
                $plugin
            }
            fn chain(&self) -> ChainFamily {
                $chain
            }
            fn detect(&self, root: &Path) -> Result<Option<DetectedProject>, DetectError> {
                if $probe(root) {
                    Ok(Some(DetectedProject {
                        chain: $chain,
                        root: root.display().to_string(),
                        rank: $rank,
                        plugin: $plugin.into(),
                        hints: DetectionHint::default(),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    };
}

fn has_any(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| root.join(name).exists())
}

fn has_ext(root: &Path, ext: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some(ext) {
            return true;
        }
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "src" | "contracts" | "programs") && has_ext(&path, ext) {
                return true;
            }
        }
    }
    false
}

fn probe_evm(root: &Path) -> bool {
    has_any(root, &["foundry.toml", "hardhat.config.js", "hardhat.config.ts"])
        || has_ext(root, "sol")
}

fn probe_algorand(root: &Path) -> bool {
    has_ext(root, "teal")
        || (has_ext(root, "py")
            && std::fs::read_to_string(root.join("requirements.txt"))
                .map(|c| c.contains("pyteal"))
                .unwrap_or(false))
}

fn probe_cairo(root: &Path) -> bool {
    has_any(root, &["Scarb.toml"]) || has_ext(root, "cairo")
}

fn probe_cosmos(root: &Path) -> bool {
    has_any(root, &["Cargo.toml"])
        && std::fs::read_to_string(root.join("Cargo.toml"))
            .map(|c| c.contains("cosmwasm") || c.contains("cw-storage"))
            .unwrap_or(false)
}

fn probe_ton(root: &Path) -> bool {
    has_ext(root, "fc") || has_ext(root, "func") || has_ext(root, "tact")
}

simple_plugin!(EvmDetectPlugin, "altius-evm-detect", ChainFamily::Evm, 60, probe_evm);
simple_plugin!(
    AlgorandDetectPlugin,
    "altius-algorand-detect",
    ChainFamily::Algorand,
    55,
    probe_algorand
);
simple_plugin!(
    CairoDetectPlugin,
    "altius-cairo-detect",
    ChainFamily::Cairo,
    55,
    probe_cairo
);
simple_plugin!(
    CosmosDetectPlugin,
    "altius-cosmos-detect",
    ChainFamily::Cosmos,
    55,
    probe_cosmos
);
simple_plugin!(TonDetectPlugin, "altius-ton-detect", ChainFamily::Ton, 55, probe_ton);
