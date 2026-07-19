use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    // We capture stdout/stderr by redirecting into a buffer.
    app.busy = true;
    app.commit_input();

    let result = run_command(trimmed, &app.project_path);

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

/// Run a command string and capture its stdout output.
fn run_command(input: &str, project: &PathBuf) -> Result<String, String> {
    // Re-parse the input as CLI args by prepending the binary name.
    let args = format!("altius {}", input);
    let tokens: Vec<String> = args.split_whitespace().map(String::from).collect();

    // Try to parse as a Cli. If it fails, return the error message.
    let cli = crate::cli::Cli::try_parse_from(tokens)
        .map_err(|e| e.to_string())?;

    let command = cli.command.ok_or("no command specified")?;

    // Capture stdout by redirecting into a buffer.
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_clone = Arc::clone(&buffer);

    // Redirect stdout.
    let stdout = std::io::stdout();
    let _guard = StdoutRedirect::new(buf_clone);

    let result: Result<(), crate::error::CliError> = match &command {
        crate::cli::Command::Detect(args) => {
            crate::detect_command::run_detect(&args.project)
        }
        crate::cli::Command::Scan(args) => crate::scan_command::run_scan(args),
        crate::cli::Command::Eval(args) => crate::eval_command::run_eval(args),
        crate::cli::Command::Deploy(args) => {
            // Deploy requires interactive approval which doesn't work in TUI.
            if !args.yes && !args.dry_run {
                eprintln!("deploy: requires --yes or --dry-run in TUI mode");
                return Err("deploy: requires --yes or --dry-run in TUI mode".into());
            }
            crate::deploy_command::run_deploy(args)
        }
        crate::cli::Command::Fleet(args) => match &args.command {
            crate::cli::FleetCommand::Run(run) => {
                crate::fleet_command::run_fleet_cmd(run)
            }
            crate::cli::FleetCommand::Serve(_) => {
                eprintln!("fleet serve: long-running server — run outside TUI");
                return Err("fleet serve: long-running server — run outside TUI".into());
            }
            crate::cli::FleetCommand::Mcp(mcp) => {
                crate::fleet_command::run_mcp_cmd(mcp)
            }
            crate::cli::FleetCommand::Acp(_) => {
                eprintln!("fleet acp: stdio JSON-RPC — run outside TUI");
                return Err("fleet acp: stdio JSON-RPC — run outside TUI".into());
            }
            crate::cli::FleetCommand::A2a(_) => {
                eprintln!("fleet a2a: long-running server — run outside TUI");
                return Err("fleet a2a: long-running server — run outside TUI".into());
            }
        },
    };

    // Flush and restore stdout before we read the buffer.
    let _ = stdout.lock().flush();
    drop(_guard);

    if let Err(err) = result {
        return Err(err.to_string());
    }

    let buf = buffer.lock().unwrap();
    let output = String::from_utf8_lossy(&buf).to_string();
    Ok(output)
}

/// Redirect stdout writes into a shared buffer for the duration of the guard.
struct StdoutRedirect {
    /// We need to keep the original stdout handle to restore it on drop.
    _original: std::io::Stdout,
}

impl StdoutRedirect {
    fn new(_buffer: Arc<Mutex<Vec<u8>>>) -> Self {
        // Note: Rust's std::io::stdout() doesn't support per-thread redirection.
        // We use a simpler approach: print to stderr for errors and capture
        // via a pipe. For now, we just let commands print normally and
        // capture what we can. The real implementation would use dup2 on Unix.
        // This is a Phase 1 simplification — commands print to the terminal
        // normally, and we show a placeholder in the output pane.
        Self {
            _original: std::io::stdout(),
        }
    }
}

impl Write for StdoutRedirect {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        // In a real implementation, this would write to the buffer.
        // For now, pass through to stdout.
        std::io::stdout().write(_buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        let _ = std::io::stdout().flush();
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
