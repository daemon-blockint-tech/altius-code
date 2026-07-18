use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::error::SignerError;
use crate::keys::{Pubkey, Signature};
use crate::protocol::{Request, Response};
use crate::transport::{read_message, write_message};

/// The only way `altius-txguard` (or anything else) talks to the signer:
/// over a Unix domain socket, never in-process. Every call opens a fresh
/// connection — simple and adequate for the request rate a transaction
/// guardrail produces; a persistent connection pool is future work if
/// profiling ever shows it matters.
pub struct SignerClient {
    socket_path: PathBuf,
}

impl SignerClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> SignerClient {
        SignerClient {
            socket_path: socket_path.into(),
        }
    }

    pub fn pubkey(&self) -> Result<Pubkey, SignerError> {
        match self.roundtrip(Request::Pubkey)? {
            Response::Pubkey { bytes } => bytes_to_pubkey(&bytes),
            Response::Error { message } => Err(SignerError::Backend(message)),
            other => Err(unexpected_response(&other)),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        let request = Request::Sign {
            message: message.to_vec(),
        };
        match self.roundtrip(request)? {
            Response::Signature { bytes } => bytes_to_signature(&bytes),
            Response::Error { message } => Err(SignerError::Backend(message)),
            other => Err(unexpected_response(&other)),
        }
    }

    fn roundtrip(&self, request: Request) -> Result<Response, SignerError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        write_message(&mut stream, &request)?;
        read_message(&mut stream)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

fn bytes_to_pubkey(bytes: &[u8]) -> Result<Pubkey, SignerError> {
    let array: [u8; 32] = bytes.try_into().map_err(|_| {
        SignerError::Backend(format!(
            "pubkey response had {} bytes, want 32",
            bytes.len()
        ))
    })?;
    Ok(Pubkey(array))
}

fn bytes_to_signature(bytes: &[u8]) -> Result<Signature, SignerError> {
    let array: [u8; 64] = bytes.try_into().map_err(|_| {
        SignerError::Backend(format!(
            "signature response had {} bytes, want 64",
            bytes.len()
        ))
    })?;
    Ok(Signature(array))
}

fn unexpected_response(response: &Response) -> SignerError {
    SignerError::Backend(format!("unexpected response variant: {response:?}"))
}
