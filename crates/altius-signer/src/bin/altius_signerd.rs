//! Standalone signer daemon. Meant to be started as its own OS process,
//! separate from the Altius agent, so that a bug or a compromised prompt
//! in the agent can never obtain the keypair — the only channel to it is
//! the narrow sign-only socket protocol.
//!
//! Usage:
//!   ALTIUS_SIGNER_KEYPAIR=~/.config/solana/id.json \
//!   ALTIUS_SIGNER_SOCKET=/tmp/altius-signer.sock \
//!     altius-signerd

use std::path::PathBuf;

use altius_signer::{KeypairFileSigner, Signer, SignerServer};

fn main() {
    let keypair_path = require_env("ALTIUS_SIGNER_KEYPAIR");
    let socket_path = PathBuf::from(require_env("ALTIUS_SIGNER_SOCKET"));

    let signer = match KeypairFileSigner::load(&keypair_path) {
        Ok(signer) => signer,
        Err(err) => {
            eprintln!("altius-signerd: failed to load keypair: {err}");
            std::process::exit(1);
        }
    };
    println!(
        "altius-signerd: serving pubkey {} on {}",
        signer.pubkey(),
        socket_path.display()
    );

    let server = SignerServer::new(socket_path, signer);
    if let Err(err) = server.run() {
        eprintln!("altius-signerd: server error: {err}");
        std::process::exit(1);
    }
}

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("altius-signerd: missing required environment variable {name}");
        std::process::exit(2);
    })
}
