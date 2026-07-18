//! Capability-limited WASM specialist host.
//!
//! What ships now:
//!
//! - [`Capabilities`] — deny-by-default capability policy for a module.
//! - [`WasmAgentHost`] — registry that validates module bytes (wasm magic +
//!   version, size cap) and pins each module to its granted capabilities.
//! - Fuel- and memory-metered execution behind the `wasmtime` feature:
//!   [`WasmAgentHost::run_module`] instantiates the module with **no host
//!   imports** (pure compute), caps linear memory at
//!   [`Capabilities::max_memory_bytes`], and spends
//!   [`Capabilities::max_fuel`] units before trapping.
//!
//! # Guest ABI
//!
//! A runnable module exports its linear memory as `memory` plus two functions:
//!
//! - `alloc(len: i32) -> i32` — reserve `len` bytes, return the pointer.
//! - `run(ptr: i32, len: i32) -> i64` — process the input bytes at
//!   `[ptr, ptr + len)` and return a packed result where the high 32 bits are
//!   the output pointer and the low 32 bits are the output length.
//!
//! When the `wasmtime` feature is off, `run_module` returns
//! [`WasmAgentError::NotImplemented`] so offline builds stay dependency-light.
//! The Ontology-chain WASM CDT toolchain is a later optional specialist on top
//! of this host, per the fleet plan.
//!
//! # Security posture
//!
//! Modules never get ambient authority: no WASI, no host imports, and no
//! signing capability exists at all. A module that imports anything fails to
//! instantiate. On-chain actions always route back through TxGuard on the host
//! side, never from inside a guest.

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

    #[error("module execution failed: {0}")]
    Execution(String),

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

    #[cfg(feature = "wasmtime")]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
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

    /// Execute a registered module against `input`, returning its output bytes.
    ///
    /// Requires a module with `max_fuel > 0`. With the `wasmtime` feature
    /// enabled this runs the guest under fuel and memory limits (see the
    /// module-level ABI docs); without it, it returns
    /// [`WasmAgentError::NotImplemented`].
    #[cfg(feature = "wasmtime")]
    pub fn run_module(&self, name: &str, input: &[u8]) -> WasmAgentResult<Vec<u8>> {
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
        runtime::execute(module, input)
    }

    /// Stub execution path when the `wasmtime` feature is disabled.
    #[cfg(not(feature = "wasmtime"))]
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
            "WASM execution requires the `wasmtime` feature; module is validated and registered"
                .into(),
        ))
    }
}

/// Fuel- and memory-metered execution backend (feature `wasmtime`).
#[cfg(feature = "wasmtime")]
mod runtime {
    use super::{Capabilities, WasmAgentError, WasmAgentModule, WasmAgentResult, MAX_MODULE_BYTES};
    use wasmtime::{Config, Engine, Instance, Module, Store, StoreLimits, StoreLimitsBuilder};

    struct HostState {
        limits: StoreLimits,
    }

    pub(super) fn execute(module: &WasmAgentModule, input: &[u8]) -> WasmAgentResult<Vec<u8>> {
        if input.len() > MAX_MODULE_BYTES {
            return Err(WasmAgentError::Execution(format!(
                "input is {} bytes; cap is {MAX_MODULE_BYTES}",
                input.len()
            )));
        }
        let caps = &module.capabilities;

        let mut config = Config::new();
        config.consume_fuel(true);
        let engine =
            Engine::new(&config).map_err(|e| WasmAgentError::Execution(format!("engine: {e}")))?;

        let compiled = Module::from_binary(&engine, module.bytes())
            .map_err(|e| WasmAgentError::InvalidModule(format!("compile: {e}")))?;

        let limits = StoreLimitsBuilder::new()
            .memory_size(mem_cap(caps))
            .instances(1)
            .build();
        let mut store = Store::new(&engine, HostState { limits });
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(caps.max_fuel)
            .map_err(|e| WasmAgentError::Execution(format!("set fuel: {e}")))?;

        // Deny-by-default: no imports are provided. A module that imports
        // anything (WASI, host functions) fails to instantiate here.
        let instance = Instance::new(&mut store, &compiled, &[])
            .map_err(|e| WasmAgentError::Execution(format!("instantiate: {e}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| WasmAgentError::InvalidModule("module exports no `memory`".into()))?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| WasmAgentError::InvalidModule(format!("export `alloc`: {e}")))?;
        let run = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, "run")
            .map_err(|e| WasmAgentError::InvalidModule(format!("export `run`: {e}")))?;

        let in_len = i32::try_from(input.len())
            .map_err(|_| WasmAgentError::Execution("input length overflows i32".into()))?;
        let in_ptr = alloc
            .call(&mut store, in_len)
            .map_err(|e| map_trap("alloc", e))?;
        memory
            .write(&mut store, in_ptr as usize, input)
            .map_err(|e| WasmAgentError::Execution(format!("write input: {e}")))?;

        let packed = run
            .call(&mut store, (in_ptr, in_len))
            .map_err(|e| map_trap("run", e))?;
        let out_ptr = ((packed >> 32) & 0xffff_ffff) as usize;
        let out_len = (packed & 0xffff_ffff) as usize;
        if out_len > MAX_MODULE_BYTES {
            return Err(WasmAgentError::Execution(format!(
                "output is {out_len} bytes; cap is {MAX_MODULE_BYTES}"
            )));
        }
        let data = memory.data(&store);
        let end = out_ptr
            .checked_add(out_len)
            .ok_or_else(|| WasmAgentError::Execution("output slice overflows".into()))?;
        if end > data.len() {
            return Err(WasmAgentError::Execution(
                "output slice out of bounds of guest memory".into(),
            ));
        }
        Ok(data[out_ptr..end].to_vec())
    }

    /// Clamp the capability memory cap into `usize` for the resource limiter.
    fn mem_cap(caps: &Capabilities) -> usize {
        usize::try_from(caps.max_memory_bytes).unwrap_or(usize::MAX)
    }

    /// Translate a wasmtime error (including fuel exhaustion / traps) into a
    /// [`WasmAgentError`].
    fn map_trap(stage: &str, error: wasmtime::Error) -> WasmAgentError {
        if let Some(wasmtime::Trap::OutOfFuel) = error.downcast_ref::<wasmtime::Trap>() {
            return WasmAgentError::Execution(format!("{stage}: out of fuel"));
        }
        WasmAgentError::Execution(format!("{stage}: {error}"))
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

    #[cfg(not(feature = "wasmtime"))]
    #[test]
    fn execution_is_a_stub_without_wasmtime_feature() {
        let mut host = WasmAgentHost::new();
        host.register_module("runner", minimal_module(), Capabilities::read_only(1_000))
            .unwrap();
        assert!(matches!(
            host.run_module("runner", b"{}"),
            Err(WasmAgentError::NotImplemented(_))
        ));
    }

    #[cfg(feature = "wasmtime")]
    mod with_runtime {
        use super::*;

        /// WAT that implements the host ABI: bump-allocator + identity `run`.
        fn echo_module() -> Vec<u8> {
            wat::parse_str(
                r#"
                (module
                  (memory (export "memory") 1)
                  (global $heap (mut i32) (i32.const 1024))
                  (func $alloc (export "alloc") (param $len i32) (result i32)
                    (local $ptr i32)
                    (local.set $ptr (global.get $heap))
                    (global.set $heap (i32.add (local.get $ptr) (local.get $len)))
                    (local.get $ptr))
                  (func (export "run") (param $ptr i32) (param $len i32) (result i64)
                    (local $out i32)
                    (local $i i32)
                    (local.set $out (call $alloc (local.get $len)))
                    (loop $copy
                      (if (i32.lt_u (local.get $i) (local.get $len))
                        (then
                          (i32.store8
                            (i32.add (local.get $out) (local.get $i))
                            (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
                          (local.set $i (i32.add (local.get $i) (i32.const 1)))
                          (br $copy))))
                    (i64.or
                      (i64.shl (i64.extend_i32_u (local.get $out)) (i64.const 32))
                      (i64.extend_i32_u (local.get $len)))))
                "#,
            )
            .expect("wat parses")
        }

        /// Module whose `run` busy-loops forever — used to assert fuel exhaustion.
        fn infinite_module() -> Vec<u8> {
            wat::parse_str(
                r#"
                (module
                  (memory (export "memory") 1)
                  (func (export "alloc") (param $len i32) (result i32)
                    (i32.const 0))
                  (func (export "run") (param $ptr i32) (param $len i32) (result i64)
                    (loop $spin (br $spin))
                    (i64.const 0)))
                "#,
            )
            .expect("wat parses")
        }

        #[test]
        fn run_module_echoes_input() {
            let mut host = WasmAgentHost::new();
            host.register_module("echo", echo_module(), Capabilities::read_only(10_000_000))
                .unwrap();
            let out = host.run_module("echo", b"hello wasm").unwrap();
            assert_eq!(out, b"hello wasm");
        }

        #[test]
        fn run_module_exhausts_fuel() {
            let mut host = WasmAgentHost::new();
            host.register_module("spin", infinite_module(), Capabilities::read_only(100))
                .unwrap();
            let err = host.run_module("spin", b"").unwrap_err();
            match err {
                WasmAgentError::Execution(msg) => {
                    assert!(
                        msg.contains("fuel") || msg.contains("Fuel"),
                        "expected fuel error, got: {msg}"
                    );
                }
                other => panic!("expected Execution, got {other:?}"),
            }
        }

        #[test]
        fn modules_with_imports_fail_to_instantiate() {
            let bytes = wat::parse_str(
                r#"
                (module
                  (import "env" "log" (func $log (param i32)))
                  (memory (export "memory") 1)
                  (func (export "alloc") (param $len i32) (result i32) (i32.const 0))
                  (func (export "run") (param $ptr i32) (param $len i32) (result i64)
                    (call $log (i32.const 0))
                    (i64.const 0)))
                "#,
            )
            .unwrap();
            let mut host = WasmAgentHost::new();
            host.register_module("imports", bytes, Capabilities::read_only(1_000_000))
                .unwrap();
            assert!(matches!(
                host.run_module("imports", b""),
                Err(WasmAgentError::Execution(_))
            ));
        }
    }
}
