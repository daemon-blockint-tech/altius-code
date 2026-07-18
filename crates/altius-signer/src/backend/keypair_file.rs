use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer as DalekSigner, SigningKey};

use super::Signer;
use crate::error::SignerError;
use crate::keys::{Pubkey, Signature};

/// Loads a Solana-style `id.json` keypair file (a JSON array of 64 bytes:
/// 32-byte seed followed by the 32-byte public key, i.e. dalek's
/// `to_keypair_bytes` layout) once at construction time and keeps the
/// signing key only in this process's memory — never re-read, never
/// exported.
#[derive(Debug)]
pub struct KeypairFileSigner {
    signing_key: SigningKey,
}

impl KeypairFileSigner {
    /// Reads and parses the keypair file at `path`. The path is consumed
    /// here and nowhere else in the codebase should ever open it — that
    /// invariant is what makes running this in a separate process from the
    /// agent meaningful.
    pub fn load(path: impl AsRef<Path>) -> Result<KeypairFileSigner, SignerError> {
        let path: &Path = path.as_ref();
        let contents = std::fs::read_to_string(path)?;
        Self::parse(&contents, path)
    }

    fn parse(contents: &str, path: &Path) -> Result<KeypairFileSigner, SignerError> {
        let values: Vec<u8> = serde_json::from_str(contents)?;
        if values.len() != 64 {
            return Err(SignerError::MalformedKeypairFile {
                path: path.display().to_string(),
                len: values.len(),
            });
        }
        let mut keypair_bytes = [0u8; 64];
        keypair_bytes.copy_from_slice(&values);
        let signing_key = SigningKey::from_keypair_bytes(&keypair_bytes).map_err(|e| {
            SignerError::InvalidKeypairBytes {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        })?;
        Ok(KeypairFileSigner { signing_key })
    }

    /// Path a caller intends to load, without reading it — used only to
    /// let higher layers display *where* a signer's key lives, not what
    /// it is.
    pub fn describe_path(path: impl AsRef<Path>) -> PathBuf {
        path.as_ref().to_path_buf()
    }
}

impl Signer for KeypairFileSigner {
    fn pubkey(&self) -> Pubkey {
        Pubkey(self.signing_key.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        Ok(Signature(self.signing_key.sign(message).to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;
    use rand::rngs::SysRng;
    use rand_core::UnwrapErr;

    fn sample_keypair_json() -> String {
        let signing_key = SigningKey::generate(&mut UnwrapErr(SysRng));
        let bytes = signing_key.to_keypair_bytes();
        serde_json::to_string(&bytes.to_vec()).unwrap()
    }

    #[test]
    fn loads_valid_keypair_file_and_signs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id.json");
        std::fs::write(&path, sample_keypair_json()).unwrap();

        let signer = KeypairFileSigner::load(&path).unwrap();
        let message = b"altius txguard test message";
        let signature = signer.sign(message).unwrap();

        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&signer.pubkey().0).unwrap();
        let dalek_sig = ed25519_dalek::Signature::from_bytes(&signature.0);
        assert!(verifying_key.verify(message, &dalek_sig).is_ok());
    }

    #[test]
    fn rejects_wrong_length_keypair_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id.json");
        std::fs::write(&path, "[1, 2, 3]").unwrap();

        let err = KeypairFileSigner::load(&path).unwrap_err();
        match err {
            SignerError::MalformedKeypairFile { len, .. } => assert_eq!(len, 3),
            other => panic!("expected MalformedKeypairFile, got {other:?}"),
        }
    }
}
