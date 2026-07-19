use super::app::App;

/// Dispatch a TUI command string to the appropriate CLI handler.
///
/// Output from the command is captured and pushed into `app.output`.
pub fn dispatch(input: &str, app: &mut App) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return;
    }

    // Echo the command to output.
    app.push_output(format!("> {}", trimmed));

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let cmd = tokens[0];

    match cmd {
        "quit" | "exit" | "q" => {
            app.should_quit = true;
            app.commit_input();
            return;
        }
        "help" | "h" | "?" => {
            show_help(app);
            app.commit_input();
            return;
        }
        "clear" | "cls" => {
            app.clear_output();
            app.commit_input();
            return;
        }
        _ => {}
    }

    // For all other commands, parse with clap and dispatch to existing handlers.
    app.busy = true;
    app.commit_input();

    let result = run_command(trimmed);

    app.busy = false;

    match result {
        Ok(output) => {
            for line in output.lines() {
                app.push_output(line.to_string());
            }
        }
        Err(err) => {
            app.push_output(format!("error: {}", err));
        }
    }
    app.push_output(String::new());
}

/// Run a command string and return a summary.
///
/// Commands print to stdout/stderr normally. A future version will capture
/// stdout via `dup2` or a pipe for in-TUI display.
fn run_command(input: &str) -> Result<String, String> {
    let args = format!("altius {}", input);
    let tokens: Vec<String> = args.split_whitespace().map(String::from).collect();

    let cli = crate::cli::Cli::try_parse_from(tokens)
        .map_err(|e| e.to_string())?;

    let command = cli.command.ok_or("no command specified")?;

    match &command {
        crate::cli::Command::Detect(args) => {
            crate::detect_command::run_detect(&args.project)
                .map(|_| "detect: completed".to_string())
                .map_err(|e| e.to_string())
        }
        crate::cli::Command::Scan(args) => {
            crate::scan_command::run_scan(args)
                .map(|_| "scan: completed (output printed to terminal)".to_string())
                .map_err(|e| e.to_string())
        }
        crate::cli::Command::Eval(args) => {
            crate::eval_command::run_eval(args)
                .map(|_| "eval: completed (output printed to terminal)".to_string())
                .map_err(|e| e.to_string())
        }
        crate::cli::Command::Deploy(args) => {
            if !args.yes && !args.dry_run {
                return Err("deploy: requires --yes or --dry-run in TUI mode".into());
            }
            crate::deploy_command::run_deploy(&args)
                .map(|_| "deploy: completed (output printed to terminal)".to_string())
                .map_err(|e| e.to_string())
        }
        crate::cli::Command::Fleet(args) => match &args.command {
            crate::cli::FleetCommand::Run(run) => {
                crate::fleet_command::run_fleet_cmd(run)
                    .map(|_| "fleet run: completed (output printed to terminal)".to_string())
                    .map_err(|e| e.to_string())
            }
            crate::cli::FleetCommand::Serve(_) => {
                Err("fleet serve: long-running server — run outside TUI".into())
            }
            crate::cli::FleetCommand::Mcp(mcp) => {
                crate::fleet_command::run_mcp_cmd(mcp)
                    .map(|_| "fleet mcp: completed".to_string())
                    .map_err(|e| e.to_string())
            }
            crate::cli::FleetCommand::Acp(_) => {
                Err("fleet acp: stdio JSON-RPC — run outside TUI".into())
            }
            crate::cli::FleetCommand::A2a(_) => {
                Err("fleet a2a: long-running server — run outside TUI".into())
            }
        },
    }
}

fn show_help(app: &mut App) {
    let commands = [
        ("scan [args]", "Run security scanners on a project"),
        ("detect [path]", "Detect SVM framework (Anchor, Pinocchio, native)"),
        ("deploy [args]", "Deploy a program (requires --yes or --dry-run in TUI)"),
        ("eval [args]", "Run evaluation harness against gold-label fixtures"),
        ("fleet run --prompt <text>", "Run supervisor graph headlessly"),
        ("fleet serve [args]", "Serve BeeAI ACP + A2A (run outside TUI)"),
        ("fleet mcp [args]", "Serve MCP tools"),
        ("fleet acp", "Serve Editor ACP over stdio (run outside TUI)"),
        ("fleet a2a [args]", "Serve A2A agent card (run outside TUI)"),
        ("help", "Show this help"),
        ("clear", "Clear output"),
        ("quit", "Exit TUI"),
    ];

    app.push_output("Available commands:");
    app.push_output(String::new());
    for (cmd, desc) in commands {
        app.push_output(format!("  {:<35} {}", cmd, desc));
    }
    app.push_output(String::new());
    app.push_output("All command arguments match the CLI exactly (e.g. 'scan --path . --format markdown').");
}
