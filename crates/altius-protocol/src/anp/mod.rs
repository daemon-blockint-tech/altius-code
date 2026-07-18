//! **ANP** — the [Agent Network Protocol]: identity and discovery.
//!
//! Phase B ships stubs with real seams:
//!
//! - [`DidWba`] — syntactic parsing/validation of `did:wba` identifiers
//! - [`DidVerifier`] / [`StubDidVerifier`] — the cryptographic
//!   verification path. The stub **fails closed**: until a real
//!   implementation lands, no remote identity is ever treated as verified.
//! - [`AgentDescription`] — a bounded, simplified agent description
//! - [`InMemoryRegistry`] — local register/discover, keyed by DID
//!
//! All descriptions come from remote peers and are untrusted; they pass
//! bounded validation before storage and are never dereferenced.
//!
//! [Agent Network Protocol]: https://github.com/agent-network-protocol/AgentNetworkProtocol

mod description;
mod did;
mod registry;

pub use description::{AgentDescription, InterfaceDescription};
pub use did::{DidVerifier, DidWba, StubDidVerifier, VerifiedIdentity};
pub use registry::{AgentRegistry, InMemoryRegistry};
