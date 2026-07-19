use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use tracing::{debug, error, info, info_span};

use crate::backend::Signer;
use crate::error::SignerError;
use crate::protocol::{Request, Response};
use crate::transport::{read_message, write_message};

/// Owns the private key (via a `Signer` backend) and answers `pubkey`/
/// `sign` requests over a Unix domain socket. This is meant to run as its
/// own OS process — the agent talks to it only through [`crate::client::SignerClient`],
/// never in-process, so a bug in agent tool code cannot reach key material
/// even in principle.
pub struct SignerServer<S: Signer + 'static> {
    socket_path: PathBuf,
    backend: Arc<S>,
}

impl<S: Signer + 'static> SignerServer<S> {
    pub fn new(socket_path: impl Into<PathBuf>, backend: S) -> SignerServer<S> {
        SignerServer {
            socket_path: socket_path.into(),
            backend: Arc::new(backend),
        }
    }

    /// Binds the socket (removing a stale file left over from a previous
    /// run) and serves connections until the process is killed. Each
    /// connection is handled on its own thread and can carry multiple
    /// requests.
    pub fn run(&self) -> Result<(), SignerError> {
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        let listener = UnixListener::bind(&self.socket_path)?;
        std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o600))?;
        info!("signer IPC server listening");
        for stream in listener.incoming() {
            let stream = stream?;
            let backend = Arc::clone(&self.backend);
            thread::spawn(move || {
                let span = info_span!("signer.connection");
                let _entered = span.enter();
                debug!("accepted signer IPC connection");
                if let Err(err) = handle_connection(stream, backend.as_ref()) {
                    if !matches!(err, SignerError::ConnectionClosed) {
                        error!(error = %err, "signer IPC connection failed");
                    }
                }
            });
        }
        Ok(())
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

fn handle_connection<S: Signer>(mut stream: UnixStream, backend: &S) -> Result<(), SignerError> {
    loop {
        let request: Request = match read_message(&mut stream) {
            Ok(request) => request,
            Err(SignerError::ConnectionClosed) => return Ok(()),
            Err(e) => return Err(e),
        };
        let (operation, message_len) = request_metadata(&request);
        let span = info_span!(
            "signer.request",
            operation,
            message_len = message_len.unwrap_or_default(),
        );
        let _entered = span.enter();
        debug!("dispatching signer request");
        let response = dispatch(&request, backend);
        write_message(&mut stream, &response)?;
        debug!("completed signer request");
    }
}

fn request_metadata(request: &Request) -> (&'static str, Option<usize>) {
    match request {
        Request::Pubkey => ("pubkey", None),
        Request::Sign { message } => ("sign", Some(message.len())),
    }
}

fn dispatch<S: Signer>(request: &Request, backend: &S) -> Response {
    match request {
        Request::Pubkey => Response::Pubkey {
            bytes: backend.pubkey().0.to_vec(),
        },
        Request::Sign { message } => match backend.sign(message) {
            Ok(signature) => Response::Signature {
                bytes: signature.0.to_vec(),
            },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::KeypairFileSigner;
    use crate::client::SignerClient;
    use ed25519_dalek::SigningKey;
    use rand::rngs::SysRng;
    use rand_core::UnwrapErr;

    fn write_sample_keypair(path: &Path) {
        let signing_key = SigningKey::generate(&mut UnwrapErr(SysRng));
        let bytes = signing_key.to_keypair_bytes().to_vec();
        std::fs::write(path, serde_json::to_string(&bytes).unwrap()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn client_round_trips_pubkey_and_sign_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let keypair_path = dir.path().join("id.json");
        write_sample_keypair(&keypair_path);
        let socket_path = dir.path().join("signer.sock");

        let backend = KeypairFileSigner::load(&keypair_path).unwrap();
        let expected_pubkey = backend.pubkey();
        let server = SignerServer::new(&socket_path, backend);

        thread::spawn(move || {
            let _ = server.run();
        });
        // Give the listener a moment to bind before the client connects.
        wait_for_socket(&socket_path);
        assert_eq!(
            std::fs::metadata(&socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let client = SignerClient::new(&socket_path);
        assert_eq!(client.pubkey().unwrap(), expected_pubkey);

        let message = b"hello txguard";
        let signature = client.sign(message).unwrap();
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&expected_pubkey.0).unwrap();
        let dalek_sig = ed25519_dalek::Signature::from_bytes(&signature.0);
        use ed25519_dalek::Verifier;
        assert!(verifying_key.verify(message, &dalek_sig).is_ok());
    }

    fn wait_for_socket(path: &Path) {
        for _ in 0..200 {
            if path.exists() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("signer socket never appeared at {path:?}");
    }
}
