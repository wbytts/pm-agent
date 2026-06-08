use std::io::{self, Write};

pub const TERMINAL_PROGRESS_KEEPALIVE_MS: u64 = 1000;
pub const APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE: &str = "\x1b[13;2u";

const TERMINAL_PROGRESS_ACTIVE_SEQUENCE: &str = "\x1b]9;4;3\x07";
const TERMINAL_PROGRESS_CLEAR_SEQUENCE: &str = "\x1b]9;4;0;\x07";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalInputContext<'a> {
    pub platform: &'a str,
    pub term_program: Option<&'a str>,
    pub shift_pressed: bool,
}

/// TUI 使用的最小终端抽象，对齐 pi 的 Terminal 接口并保留可测试的输出边界。
pub trait Terminal {
    fn start(&mut self) -> io::Result<()>;
    fn stop(&mut self) -> io::Result<()>;
    fn write(&mut self, data: &str) -> io::Result<()>;
    fn columns(&self) -> u16;
    fn rows(&self) -> u16;
    fn kitty_protocol_active(&self) -> bool;

    fn move_by(&mut self, lines: i32) -> io::Result<()> {
        if let Some(sequence) = move_by_sequence(lines) {
            self.write(&sequence)?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.write(hide_cursor_sequence())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.write(show_cursor_sequence())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.write(clear_line_sequence())
    }

    fn clear_from_cursor(&mut self) -> io::Result<()> {
        self.write(clear_from_cursor_sequence())
    }

    fn clear_screen(&mut self) -> io::Result<()> {
        self.write(clear_screen_sequence())
    }

    fn set_title(&mut self, title: &str) -> io::Result<()> {
        self.write(&set_title_sequence(title))
    }

    fn set_progress(&mut self, active: bool) -> io::Result<()> {
        if active {
            self.write(start_progress_sequence())
        } else {
            self.write(clear_progress_sequence())
        }
    }
}

/// 基于任意 Write 的真实终端输出实现；生产环境可传 stdout，测试可传 Vec<u8>。
pub struct ProcessTerminal<W> {
    writer: W,
    columns: u16,
    rows: u16,
    kitty_protocol_active: bool,
    modify_other_keys_active: bool,
    progress_active: bool,
    started: bool,
}

impl<W> ProcessTerminal<W>
where
    W: Write,
{
    pub fn new(writer: W, columns: u16, rows: u16) -> Self {
        Self {
            writer,
            columns,
            rows,
            kitty_protocol_active: false,
            modify_other_keys_active: false,
            progress_active: false,
            started: false,
        }
    }

    pub fn with_dimensions(
        writer: W,
        stdout_columns: Option<u16>,
        stdout_rows: Option<u16>,
        env_columns: Option<&str>,
        env_lines: Option<&str>,
    ) -> Self {
        let (columns, rows) =
            resolve_terminal_dimensions(stdout_columns, stdout_rows, env_columns, env_lines);
        Self::new(writer, columns, rows)
    }

    pub fn set_dimensions(&mut self, columns: u16, rows: u16) {
        self.columns = columns;
        self.rows = rows;
    }

    pub fn set_kitty_protocol_active(&mut self, active: bool) {
        self.kitty_protocol_active = active;
    }

    pub fn modify_other_keys_active(&self) -> bool {
        self.modify_other_keys_active
    }

    pub fn progress_active(&self) -> bool {
        self.progress_active
    }

    pub fn enable_modify_other_keys_fallback(&mut self) -> io::Result<bool> {
        if self.kitty_protocol_active || self.modify_other_keys_active {
            return Ok(false);
        }
        self.write(enable_modify_other_keys_sequence())?;
        self.modify_other_keys_active = true;
        Ok(true)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W> Terminal for ProcessTerminal<W>
where
    W: Write,
{
    fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        self.write(enable_bracketed_paste_sequence())?;
        self.write(query_kitty_keyboard_protocol_sequence())
    }

    fn stop(&mut self) -> io::Result<()> {
        if !self.started {
            return Ok(());
        }
        self.started = false;
        self.write(bracketed_paste_sequence(false))?;
        if self.kitty_protocol_active {
            self.write(disable_kitty_keyboard_protocol_sequence())?;
            self.kitty_protocol_active = false;
        }
        if self.modify_other_keys_active {
            self.write(disable_modify_other_keys_sequence())?;
            self.modify_other_keys_active = false;
        }
        if self.progress_active {
            self.write(clear_progress_sequence())?;
            self.progress_active = false;
        }
        self.show_cursor()
    }

    fn set_progress(&mut self, active: bool) -> io::Result<()> {
        if active {
            self.progress_active = true;
            self.write(start_progress_sequence())
        } else {
            self.progress_active = false;
            self.write(clear_progress_sequence())
        }
    }

    fn write(&mut self, data: &str) -> io::Result<()> {
        self.writer.write_all(data.as_bytes())?;
        self.writer.flush()
    }

    fn columns(&self) -> u16 {
        self.columns
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn kitty_protocol_active(&self) -> bool {
        self.kitty_protocol_active
    }
}

pub fn move_by_sequence(lines: i32) -> Option<String> {
    match lines.cmp(&0) {
        std::cmp::Ordering::Greater => Some(format!("\x1b[{lines}B")),
        std::cmp::Ordering::Less => Some(format!("\x1b[{}A", -lines)),
        std::cmp::Ordering::Equal => None,
    }
}

pub fn hide_cursor_sequence() -> &'static str {
    "\x1b[?25l"
}

pub fn show_cursor_sequence() -> &'static str {
    "\x1b[?25h"
}

pub fn clear_line_sequence() -> &'static str {
    "\x1b[K"
}

pub fn clear_from_cursor_sequence() -> &'static str {
    "\x1b[J"
}

pub fn clear_screen_sequence() -> &'static str {
    "\x1b[2J\x1b[H"
}

pub fn set_title_sequence(title: &str) -> String {
    format!("\x1b]0;{title}\x07")
}

pub fn start_progress_sequence() -> &'static str {
    TERMINAL_PROGRESS_ACTIVE_SEQUENCE
}

pub fn clear_progress_sequence() -> &'static str {
    TERMINAL_PROGRESS_CLEAR_SEQUENCE
}

pub fn bracketed_paste_sequence(enabled: bool) -> &'static str {
    if enabled {
        enable_bracketed_paste_sequence()
    } else {
        "\x1b[?2004l"
    }
}

pub fn enable_bracketed_paste_sequence() -> &'static str {
    "\x1b[?2004h"
}

pub fn query_kitty_keyboard_protocol_sequence() -> &'static str {
    "\x1b[?u"
}

pub fn enable_kitty_keyboard_protocol_sequence() -> &'static str {
    "\x1b[>7u"
}

pub fn disable_kitty_keyboard_protocol_sequence() -> &'static str {
    "\x1b[<u"
}

pub fn enable_modify_other_keys_sequence() -> &'static str {
    "\x1b[>4;2m"
}

pub fn disable_modify_other_keys_sequence() -> &'static str {
    "\x1b[>4;0m"
}

pub fn resolve_terminal_dimensions(
    stdout_columns: Option<u16>,
    stdout_rows: Option<u16>,
    env_columns: Option<&str>,
    env_lines: Option<&str>,
) -> (u16, u16) {
    let columns = stdout_columns
        .filter(|columns| *columns > 0)
        .or_else(|| parse_terminal_dimension(env_columns))
        .unwrap_or(80);
    let rows = stdout_rows
        .filter(|rows| *rows > 0)
        .or_else(|| parse_terminal_dimension(env_lines))
        .unwrap_or(24);
    (columns, rows)
}

pub fn normalize_apple_terminal_input(
    data: &str,
    is_apple_terminal: bool,
    is_shift_pressed: bool,
) -> String {
    if is_apple_terminal && data == "\r" && is_shift_pressed {
        APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE.to_string()
    } else {
        data.to_string()
    }
}

pub fn is_apple_terminal_session(platform: &str, term_program: Option<&str>) -> bool {
    platform == "darwin" && term_program == Some("Apple_Terminal")
}

pub fn normalize_terminal_input(data: &str, context: TerminalInputContext<'_>) -> String {
    normalize_apple_terminal_input(
        data,
        is_apple_terminal_session(context.platform, context.term_program),
        context.shift_pressed,
    )
}

fn parse_terminal_dimension(value: Option<&str>) -> Option<u16> {
    value?.parse::<u16>().ok().filter(|value| *value > 0)
}
