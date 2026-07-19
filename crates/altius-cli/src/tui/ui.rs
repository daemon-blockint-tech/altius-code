use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::App;

/// Render the full TUI.
pub fn draw(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Layout: title bar (3) | output (flex) | input (3) | status bar (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_title_bar(frame, chunks[0]);
    draw_output(frame, chunks[1], app);
    draw_input(frame, chunks[2], app);
    draw_status_bar(frame, chunks[3], app);
}

fn draw_title_bar(frame: &mut Frame, area: Rect) {
    let title = format!(" Altius Code v{}", env!("CARGO_PKG_VERSION"));
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(block);
    frame.render_widget(paragraph, area);
}

fn draw_output(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Output ")
        .style(Style::default().fg(Color::DarkGray));

    let visible = app.visible_output(area.height.saturating_sub(2) as usize);
    let lines: Vec<Line> = visible
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((0, 0));
    frame.render_widget(paragraph, area);
}

fn draw_input(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Input ")
        .style(Style::default().fg(Color::Yellow));

    let prompt_span = Span::styled("> ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    let input_span = Span::raw(app.input.as_str());
    let line = Line::from(vec![prompt_span, input_span]);

    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);

    // Place the cursor at the right position inside the input area.
    let inner = {
        let mut r = area;
        r.x += 1; // border
        r.y += 1; // border
        r.width -= 2; // both borders
        r
    };
    // Prompt is "> " (2 chars) + cursor offset
    let cursor_x = inner.x + 2 + app.cursor as u16;
    let cursor_y = inner.y;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let project = app.project_path.display().to_string();
    let busy = if app.busy { " [running]" } else { "" };
    let status = format!(" {}{} | ↑↓ history | Ctrl+C quit | Ctrl+L clear", project, busy);
    let span = Span::styled(
        status,
        Style::default().fg(Color::Black).bg(Color::DarkGray),
    );
    let line = Line::from(span);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
