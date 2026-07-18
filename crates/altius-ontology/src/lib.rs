//! Domain ontology layer for the Altius fleet (Phase D).
//!
//! Note on naming: this crate is about OWL/RDF-style *domain schemas* (what
//! is a Program, an Instruction, a Vulnerability, and how do they relate),
//! **not** the Ontology blockchain — its WASM CDT toolchain belongs to
//! `altius-wasm-agents` as a chain specialist, per the fleet plan.
//!
//! Two layers:
//!
//! - [`schema`] — a small, built-in SVM/security ontology subset the
//!   knowledge agent can use offline (classes, properties, subclass edges).
//! - [`OntologyClient`] — the adapter trait an external ontology MCP server
//!   (e.g. an open-ontologies deployment) implements. [`StaticOntologyClient`]
//!   backed by the built-in schema is always available; [`McpOntologyClient`]
//!   (feature `mcp`) talks to an external OWL/RDF ontology MCP server and
//!   treats its responses as untrusted input.

pub mod schema;

mod client;
#[cfg(feature = "mcp")]
mod mcp;

pub use client::{OntologyClient, OntologyError, OntologyResult, StaticOntologyClient};
#[cfg(feature = "mcp")]
pub use mcp::{McpOntologyClient, McpOntologyConfig};
pub use schema::{svm_security_schema, ClassDef, DomainSchema, PropertyDef};
