//! Capability-limited WASM specialist host (Phase D stub).
//!
//! What ships now:
//!
//! - [`Capabilities`] — deny-by-default capability policy for a module.
//! - [`WasmAgentHost`] — registry that validates module bytes (wasm magic +
//!   version, size cap) and pins each module to its granted capabilities.
//!
//! What is an intentional stub: actual execution. `run_module` returns
//! [`WasmAgentError::NotImplemented`] until a runtime (wasmtime-class) is
//! chosen and wired with fuel metering. The host API is shaped so that
//! swap-in changes no callers. The Ontology-chain WASM CDT toolchain is a
//! later optional specialist on top of this host, per the fleet plan.
//!
//! Security posture: modules never get ambient authority. There is no
//! capability that exposes signing — on-chain actions always route back
//! through TxGuard on the host side.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// WASM binary magic (`\0asm`) followed by version 1.
const WASM_MAGIC: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

/// Hard cap on accepted module size (16 MiB).
pub const MAX_MODULE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum WasmAgentError {
    #[error("invalid wasm module: {0}")]
    InvalidModule(String),

    #[error("unknown module {0}")]
    UnknownModule(String),

    #[error("module {module} lacks capability {capability}")]
    CapabilityDenied { module: String, capability: String },

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

pub type WasmAgentResult<T> = Result<T, WasmAgentError>;

/// Capability grants for one module. Everything defaults to denied /
/// smallest; there is deliberately no signing capability at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Capabilities {
    /// Read files under the workspace root.
    pub fs_read: bool,
    /// Write files under the workspace root.
    pub fs_write: bool,
    /// Outbound HTTP (still subject to host-side allowlists).
    pub network: bool,
    /// Linear-memory cap in bytes.
    pub max_memory_bytes: u64,
    /// Execution fuel budget (0 = cannot run).
    pub max_fuel: u64,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            fs_read: false,
            fs_write: false,
            network: false,
            max_memory_bytes: 64 * 1024 * 1024,
            max_fuel: 0,
        }
    }
}

impl Capabilities {
    /// Read-only workspace access with a fuel budget — the sensible default
    /// for analysis specialists.
    pub fn read_only(max_fuel: u64) -> Self {
        Self {
            fs_read: true,
            max_fuel,
            ..Self::default()
        }
    }
}

/// A validated, registered module.
#[derive(Clone, Debug)]
pub struct WasmAgentModule {
    pub name: String,
    pub capabilities: Capabilities,
    bytes: Vec<u8>,
}

impl WasmAgentModule {
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

/// Registry of WASM specialist modules and their capability grants.
#[derive(Default)]
pub struct WasmAgentHost {
    modules: HashMap<String, WasmAgentModule>,
}

impl WasmAgentHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and register module bytes under `name`.
    pub fn register_module(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        capabilities: Capabilities,
    ) -> WasmAgentResult<()> {
        if bytes.len() > MAX_MODULE_BYTES {
            return Err(WasmAgentError::InvalidModule(format!(
                "module is {} bytes; cap is {MAX_MODULE_BYTES}",
                bytes.len()
            )));
        }
        if bytes.len() < WASM_MAGIC.len() || bytes[..WASM_MAGIC.len()] != WASM_MAGIC {
            return Err(WasmAgentError::InvalidModule(
                "missing \\0asm magic / version-1 header".into(),
            ));
        }
        self.modules.insert(
            name.to_owned(),
            WasmAgentModule {
                name: name.to_owned(),
                capabilities,
                bytes,
            },
        );
        Ok(())
    }

    pub fn module(&self, name: &str) -> Option<&WasmAgentModule> {
        self.modules.get(name)
    }

    /// Check a capability before any host function would be exposed.
    pub fn require_capability(&self, name: &str, capability: &str) -> WasmAgentResult<()> {
        let module = self
            .modules
            .get(name)
            .ok_or_else(|| WasmAgentError::UnknownModule(name.to_owned()))?;
        let granted = match capability {
            "fs_read" => module.capabilities.fs_read,
            "fs_write" => module.capabilities.fs_write,
            "network" => module.capabilities.network,
            _ => false,
        };
        if granted {
            Ok(())
        } else {
            Err(WasmAgentError::CapabilityDenied {
                module: name.to_owned(),
                capability: capability.to_owned(),
            })
        }
    }

    /// Execute a registered module. Intentional stub — see module docs.
    pub fn run_module(&self, name: &str, _input: &[u8]) -> WasmAgentResult<Vec<u8>> {
        let module = self
            .modules
            .get(name)
            .ok_or_else(|| WasmAgentError::UnknownModule(name.to_owned()))?;
        if module.capabilities.max_fuel == 0 {
            return Err(WasmAgentError::CapabilityDenied {
                module: name.to_owned(),
                capability: "max_fuel > 0".into(),
            });
        }
        Err(WasmAgentError::NotImplemented(
            "WASM execution runtime lands after Phase D; module is validated and registered".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_module() -> Vec<u8> {
        WASM_MAGIC.to_vec()
    }

    #[test]
    fn registers_a_valid_module() {
        let mut host = WasmAgentHost::new();
        host.register_module("explorer", minimal_module(), Capabilities::read_only(1_000))
            .unwrap();
        assert_eq!(host.module("explorer").unwrap().byte_len(), 8);
    }

    #[test]
    fn rejects_non_wasm_bytes() {
        let mut host = WasmAgentHost::new();
        let err = host
            .register_module("bad", b"#!/bin/sh".to_vec(), Capabilities::default())
            .unwrap_err();
        assert!(matches!(err, WasmAgentError::InvalidModule(_)));
    }

    #[test]
    fn capabilities_deny_by_default() {
        let mut host = WasmAgentHost::new();
        host.register_module("worker", minimal_module(), Capabilities::default())
            .unwrap();
        for capability in ["fs_read", "fs_write", "network", "sign"] {
            assert!(matches!(
                host.require_capability("worker", capability),
                Err(WasmAgentError::CapabilityDenied { .. })
            ));
        }
    }

    #[test]
    fn read_only_grants_fs_read_only() {
        let mut host = WasmAgentHost::new();
        host.register_module("analyst", minimal_module(), Capabilities::read_only(10))
            .unwrap();
        host.require_capability("analyst", "fs_read").unwrap();
        assert!(host.require_capability("analyst", "fs_write").is_err());
        assert!(host.require_capability("analyst", "network").is_err());
    }

    #[test]
    fn zero_fuel_modules_cannot_run() {
        let mut host = WasmAgentHost::new();
        host.register_module("idle", minimal_module(), Capabilities::default())
            .unwrap();
        assert!(matches!(
            host.run_module("idle", b""),
            Err(WasmAgentError::CapabilityDenied { .. })
        ));
    }

    #[test]
    fn execution_is_a_stub_for_now() {
        let mut host = WasmAgentHost::new();
        host.register_module("runner", minimal_module(), Capabilities::read_only(1_000))
            .unwrap();
        assert!(matches!(
            host.run_module("runner", b"{}"),
            Err(WasmAgentError::NotImplemented(_))
        ));
    }
}
