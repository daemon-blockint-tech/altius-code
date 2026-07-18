use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "altius", version, about = "Altius Code SVM tooling")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Detect the SVM framework (Anchor, Pinocchio, native) at a path and
    /// print what was found.
    Detect(DetectArgs),
    /// Build the deployment plan for a program and run every transaction
    /// in it through the mandatory guardrail pipeline (policy, simulation,
    /// diff, approval, audit log) before broadcasting.
    Deploy(DeployArgs),
}

#[derive(Debug, Parser)]
pub struct DetectArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(default_value = ".")]
    pub project: PathBuf,
}

#[derive(Debug, Parser)]
pub struct DeployArgs {
    /// Project directory containing the already-built program.
    #[arg(long, default_value = ".")]
    pub project: PathBuf,

    /// Target cluster. Defaults to whatever the project declares (Anchor.toml's
    /// `[provider] cluster`, or localnet otherwise).
    #[arg(long)]
    pub cluster: Option<String>,

    /// RPC URL to simulate and broadcast against. Defaults to the well-known
    /// public endpoint for `--cluster` (or http://127.0.0.1:8899 for localnet).
    #[arg(long)]
    pub rpc_url: Option<String>,

    /// Unix socket of a running `altius-signerd` holding the fee payer /
    /// upgrade authority keypair. Defaults to $ALTIUS_SIGNER_SOCKET.
    #[arg(long)]
    pub signer_socket: Option<PathBuf>,

    /// Redeploy an already-existing program (uses the `Upgrade` instruction)
    /// instead of an initial deploy.
    #[arg(long)]
    pub upgrade: bool,

    /// Skip the interactive confirmation prompt. This still goes through
    /// `AutoApprove`, which refuses mainnet and irreversible transactions
    /// on its own — it does not bypass that safeguard.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Run policy, simulation, and diff reporting for every step and print
    /// the results, but never approve, sign, or broadcast anything.
    #[arg(long)]
    pub dry_run: bool,
}
