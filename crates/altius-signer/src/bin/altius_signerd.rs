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
use tracing::{error, info};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let keypair_path = require_env("ALTIUS_SIGNER_KEYPAIR");
    let socket_path = PathBuf::from(require_env("ALTIUS_SIGNER_SOCKET"));

    let signer = match KeypairFileSigner::load(&keypair_path) {
        Ok(signer) => signer,
        Err(err) => {
            error!(error = %err, "failed to load signer keypair");
            std::process::exit(1);
        }
    };
    info!(pubkey = %signer.pubkey(), "starting isolated signer daemon");

    let server = SignerServer::new(socket_path, signer);
    if let Err(err) = server.run() {
        error!(error = %err, "signer server stopped with an error");
        std::process::exit(1);
    }
}

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        error!(variable = name, "missing required environment variable");
        std::process::exit(2);
    })
}
