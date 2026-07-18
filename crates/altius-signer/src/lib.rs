//! Isolates private key material from the rest of Altius Code.
//!
//! The signer runs as its own OS process (see the `altius-signerd`
//! binary) behind a narrow sign-only IPC API ([`client::SignerClient`] /
//! [`server::SignerServer`]). Nothing in the agent process — and nothing
//! in `altius-txguard`, which is this crate's only intended caller — ever
//! sees a private key.
//!
//! See `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §7 in the repo root.

mod backend;
mod client;
mod error;
mod keys;
mod protocol;
pub mod redaction;
mod server;
mod transport;

pub use backend::{KeypairFileSigner, Signer};
pub use client::SignerClient;
pub use error::SignerError;
pub use keys::{Pubkey, Signature};
pub use protocol::{Request, Response};
pub use server::SignerServer;
