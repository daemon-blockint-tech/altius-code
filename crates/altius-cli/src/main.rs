mod cli;
mod deploy_command;
mod detect_command;
mod error;
mod fleet_command;
mod rpc_endpoint;
mod terminal_approval;
mod toolchain_for;

use clap::Parser;

use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Detect(args) => detect_command::run_detect(&args.project),
        Command::Deploy(args) => deploy_command::run_deploy(args),
        Command::Fleet(args) => fleet_command::run_fleet_command(args),
    };

    if let Err(err) = result {
        eprintln!("altius: {err}");
        std::process::exit(1);
    }
}
