use altius_fleet::{run_fleet, FleetConfig, OpenRouterAdapter};

use crate::cli::FleetArgs;
use crate::error::CliError;

const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4.5";

pub fn run_fleet_command(args: &FleetArgs) -> Result<(), CliError> {
    let api_key = std::env::var("OPENROUTER_API_KEY").map_err(|_| CliError::MissingApiKey)?;
    let model_id = args
        .model
        .clone()
        .or_else(|| std::env::var("ALTIUS_FLEET_MODEL").ok())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let mut config = FleetConfig::new(&args.goal, &args.project);
    config.max_steps = args.max_steps;

    println!(
        "fleet: model {model_id}, project {}, budget {} steps per specialist",
        args.project.display(),
        args.max_steps
    );

    let runtime = tokio::runtime::Runtime::new()?;
    let report = runtime.block_on(run_fleet(config, move |_role, tool_infos| {
        OpenRouterAdapter::with_api_key(&model_id, &api_key)
            .with_temperature(0.2)
            .bind_tools(tool_infos.to_vec())
    }))?;

    for agent_report in &report.reports {
        let status = if agent_report.tool_failure {
            "FAILED"
        } else {
            "ok"
        };
        println!("\n=== {} [{status}] ===", agent_report.role);
        println!("{}", agent_report.summary);
    }
    if report.failed {
        println!("\nfleet: a stage failed; remaining stages were skipped");
        std::process::exit(1);
    }
    Ok(())
}
