mod keypair_file;

pub use keypair_file::KeypairFileSigner;

use crate::error::SignerError;
use crate::keys::{Pubkey, Signature};

/// The only thing anything outside this crate can do with a private key:
/// ask for the public key it corresponds to, or ask it to sign a message.
/// There is deliberately no method that returns key material — that is
/// the entire point of running the signer as an isolated process (see the
/// crate-level docs and Phase 0 spec §7).
pub trait Signer: Send + Sync {
    fn pubkey(&self) -> Pubkey;
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError>;
}
