use std::path::PathBuf;

/// TUI application state.
pub struct App {
    /// Current input buffer.
    pub input: String,
    /// Cursor position within the input buffer.
    pub cursor: usize,
    /// Command history (most recent last).
    pub history: Vec<String>,
    /// Index into history when navigating with Up/Down; `history.len()` = new line.
    pub history_idx: usize,
    /// Output lines (stdout/stderr from commands + TUI messages).
    pub output: Vec<String>,
    /// Vertical scroll offset in the output pane (lines from bottom).
    pub scroll: u16,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Current project path for the status bar.
    pub project_path: PathBuf,
    /// Whether a command is currently running.
    pub busy: bool,
}

impl App {
    pub fn new(project_path: PathBuf) -> Self {
        let mut output = Vec::new();
        output.push("Altius Code — interactive REPL. Type 'help' for commands, 'quit' to exit.".into());
        output.push(String::new());

        Self {
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_idx: 0,
            output,
            scroll: 0,
            should_quit: false,
            project_path,
            busy: false,
        }
    }

    /// Push a line to the output buffer and reset scroll.
    pub fn push_output(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
        self.scroll = 0;
    }

    /// Push the current input to history and reset for next command.
    pub fn commit_input(&mut self) {
        let cmd = self.input.trim().to_string();
        if !cmd.is_empty() {
            self.history.push(cmd.clone());
        }
        self.history_idx = self.history.len();
        self.input.clear();
        self.cursor = 0;
    }

    /// Navigate history backwards (older).
    pub fn history_prev(&mut self) {
        if self.history.is_empty() || self.history_idx == 0 {
            return;
        }
        self.history_idx -= 1;
        self.input = self.history[self.history_idx].clone();
        self.cursor = self.input.len();
    }

    /// Navigate history forwards (newer).
    pub fn history_next(&mut self) {
        if self.history_idx + 1 >= self.history.len() {
            self.history_idx = self.history.len();
            self.input.clear();
            self.cursor = 0;
        } else {
            self.history_idx += 1;
            self.input = self.history[self.history_idx].clone();
            self.cursor = self.input.len();
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous char boundary.
            let prev = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    /// Move cursor left one char.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.cursor = prev;
        }
    }

    /// Move cursor right one char.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.input.len() {
            let next = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
            self.cursor = next;
        }
    }

    /// Clear all output.
    pub fn clear_output(&mut self) {
        self.output.clear();
        self.scroll = 0;
    }

    /// Scroll output up by `n` lines.
    pub fn scroll_up(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_add(n);
    }

    /// Scroll output down by `n` lines.
    pub fn scroll_down(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    /// Get the visible slice of output for rendering.
    pub fn visible_output(&self, height: usize) -> &[String] {
        let total = self.output.len();
        if total <= height {
            return &self.output;
        }
        let end = total.saturating_sub(self.scroll as usize);
        let start = end.saturating_sub(height);
        &self.output[start..end]
    }
}
