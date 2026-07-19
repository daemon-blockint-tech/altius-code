mod acp_command;
mod cli;
mod deploy_command;
mod detect_command;
mod error;
mod eval_command;
mod fleet_command;
mod github_connector;
mod plugin;
mod rpc_endpoint;
mod scan_command;
mod serve_command;
mod terminal_approval;
mod toolchain_for;
mod tui;

use clap::Parser;

use cli::{Cli, Command, FleetCommand};
use error::CliError;

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let cli = Cli::parse();

    let result: Result<(), CliError> = match &cli.command {
        None => tui::run(),
        Some(Command::Detect(args)) => detect_command::run_detect(&args.project),
        Some(Command::Scan(args)) => scan_command::run_scan(args),
        Some(Command::Eval(args)) => eval_command::run_eval(args),
        Some(Command::Deploy(args)) => deploy_command::run_deploy(args),
        Some(Command::Fleet(args)) => match &args.command {
            FleetCommand::Run(run) => fleet_command::run_fleet_cmd(run),
            FleetCommand::Serve(serve) => serve_command::run_serve_cmd(serve),
            FleetCommand::Mcp(mcp) => fleet_command::run_mcp_cmd(mcp),
            FleetCommand::Acp(acp) => acp_command::run_acp_cmd(acp),
            FleetCommand::A2a(a2a) => serve_command::run_a2a_cmd(a2a),
        },
    };

    if let Err(err) = result {
        eprintln!("altius: {err}");
        std::process::exit(1);
    }
}
