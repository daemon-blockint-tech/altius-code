use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
    /// Multi-agent fleet commands (supervisor + specialists).
    Fleet(FleetArgs),
}

#[derive(Debug, Parser)]
pub struct FleetArgs {
    #[command(subcommand)]
    pub command: FleetCommand,
}

#[derive(Debug, Subcommand)]
pub enum FleetCommand {
    /// Run the supervisor graph headlessly against a prompt.
    Run(FleetRunArgs),
    /// Serve BeeAI ACP runs and A2A tasks over HTTP.
    Serve(FleetServeArgs),
    /// Serve safe SVM tools over the Model Context Protocol.
    Mcp(FleetMcpArgs),
    /// Serve the Editor Agent Client Protocol over stdio JSON-RPC.
    Acp(FleetAcpArgs),
    /// Serve the A2A Agent Card and task endpoint over HTTP.
    A2a(FleetServeArgs),
}

#[derive(Debug, Parser)]
pub struct FleetRunArgs {
    /// User task for the fleet.
    #[arg(long)]
    pub prompt: String,

    /// Project directory the fleet should ground on.
    #[arg(long, default_value = ".")]
    pub project: PathBuf,

    /// Use the deterministic offline LLM (no network). Useful for demos and CI.
    #[arg(long)]
    pub offline: bool,
}

#[derive(Debug, Parser)]
pub struct FleetServeArgs {
    /// HTTP bind address.
    #[arg(long, default_value = "127.0.0.1:8788")]
    pub bind: String,

    /// Public base URL advertised by the A2A Agent Card.
    #[arg(long, default_value = "http://127.0.0.1:8788")]
    pub public_url: String,

    /// Use the deterministic offline LLM for BeeAI ACP run execution.
    #[arg(long)]
    pub offline: bool,

    /// Command used to launch the optional browser MCP stdio server
    /// (e.g. `npx`). Overrides `ALTIUS_BROWSER_MCP_CMD` when set.
    #[arg(long)]
    pub browser_mcp_cmd: Option<String>,

    /// JSON array of arguments for `--browser-mcp-cmd`
    /// (e.g. `["@playwright/mcp@latest"]`). Overrides
    /// `ALTIUS_BROWSER_MCP_ARGS` when set.
    #[arg(long)]
    pub browser_mcp_args: Option<String>,
}

#[derive(Debug, Parser)]
pub struct FleetAcpArgs {
    /// Use deterministic offline responses instead of an LLM provider.
    #[arg(long)]
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum McpTransport {
    Stdio,
    Http,
}

#[derive(Debug, Parser)]
pub struct FleetMcpArgs {
    /// Workspace boundary for all MCP project paths.
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,

    /// MCP transport to serve.
    #[arg(long, value_enum, default_value_t = McpTransport::Stdio)]
    pub transport: McpTransport,

    /// HTTP bind address. Ignored for stdio.
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub bind: String,
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
