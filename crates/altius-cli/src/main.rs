mod cli;
mod deploy_command;
mod detect_command;
mod error;
mod fleet_command;
mod rpc_endpoint;
mod terminal_approval;
mod toolchain_for;

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
        Command::Detect(args) => detect_command::run_detect(&args.project),
        Command::Deploy(args) => deploy_command::run_deploy(args),
        Command::Fleet(args) => match &args.command {
            FleetCommand::Run(run) => fleet_command::run_fleet_cmd(run),
        },
    };

    if let Err(err) = result {
        eprintln!("altius: {err}");
        std::process::exit(1);
    }
}
