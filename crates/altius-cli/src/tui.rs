mod app;
mod dispatcher;
mod ui;

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;
use crate::error::CliError;

/// Run the interactive TUI REPL.
pub fn run() -> Result<(), CliError> {
    let project_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    // Set up terminal.
    enable_raw_mode()
        .map_err(|e| CliError::message(format!("enable raw mode: {e}")))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| CliError::message(format!("enter alternate screen: {e}")))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| CliError::message(format!("create terminal: {e}")))?;

    // Panic hook to restore terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    let mut app_state = App::new(project_path);

    // Main event loop.
    let result = run_loop(&mut terminal, &mut app_state);

    // Restore terminal regardless of outcome.
    let _ = restore_terminal();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app_state: &mut App,
) -> Result<(), CliError> {
    loop {
        // Draw the UI.
        terminal
            .draw(|frame| ui::draw(app_state, frame))
            .map_err(|e| CliError::message(format!("draw: {e}")))?;

        // Poll for events with a short timeout so we can update if busy.
        if !event::poll(Duration::from_millis(100))
            .map_err(|e| CliError::message(format!("poll: {e}")))?
        {
            continue;
        }

        let event = event::read()
            .map_err(|e| CliError::message(format!("read event: {e}")))?;

        if let Event::Key(key) = event {
            handle_key(key, app_state);
        }

        if app_state.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut App) {
    // Global shortcuts first.
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => {
                app.should_quit = true;
                return;
            }
            KeyCode::Char('l') => {
                app.clear_output();
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Enter => {
            let input = app.input.clone();
            dispatcher::dispatch(&input, app);
        }
        KeyCode::Up => {
            app.history_prev();
        }
        KeyCode::Down => {
            app.history_next();
        }
        KeyCode::Left => {
            app.cursor_left();
        }
        KeyCode::Right => {
            app.cursor_right();
        }
        KeyCode::Backspace => {
            app.backspace();
        }
        KeyCode::PageUp => {
            app.scroll_up(10);
        }
        KeyCode::PageDown => {
            app.scroll_down(10);
        }
        KeyCode::Char(c) => {
            app.insert_char(c);
        }
        _ => {}
    }
}

fn restore_terminal() -> Result<(), CliError> {
    disable_raw_mode()
        .map_err(|e| CliError::message(format!("disable raw mode: {e}")))?;
    execute!(io::stdout(), LeaveAlternateScreen)
        .map_err(|e| CliError::message(format!("leave alternate screen: {e}")))?;
    let _ = io::stdout().flush();
    Ok(())
}
