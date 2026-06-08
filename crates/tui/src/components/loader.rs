use super::{Component, Text};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use std::sync::Arc;

const DEFAULT_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DEFAULT_INTERVAL_MS: u64 = 80;

pub type ColorFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Clone)]
pub struct LoaderIndicatorOptions {
    pub frames: Option<Vec<String>>,
    pub interval_ms: Option<u64>,
}

pub struct Loader {
    text: Text,
    frames: Vec<String>,
    interval_ms: u64,
    current_frame: usize,
    running: bool,
    render_indicator_verbatim: bool,
    spinner_color_fn: ColorFn,
    message_color_fn: ColorFn,
    message: String,
}

impl Loader {
    pub fn new(
        spinner_color_fn: ColorFn,
        message_color_fn: ColorFn,
        message: impl Into<String>,
        indicator: Option<LoaderIndicatorOptions>,
    ) -> Self {
        let mut loader = Self {
            text: Text::new("", 1, 0),
            frames: default_frames(),
            interval_ms: DEFAULT_INTERVAL_MS,
            current_frame: 0,
            running: false,
            render_indicator_verbatim: false,
            spinner_color_fn,
            message_color_fn,
            message: message.into(),
        };
        loader.set_indicator(indicator);
        loader
    }

    pub fn start(&mut self) {
        self.running = true;
        self.update_display();
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn tick(&mut self) {
        if self.running && self.frames.len() > 1 {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
            self.update_display();
        }
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.update_display();
    }

    pub fn set_indicator(&mut self, indicator: Option<LoaderIndicatorOptions>) {
        self.render_indicator_verbatim = indicator.is_some();
        self.frames = indicator
            .as_ref()
            .and_then(|indicator| indicator.frames.clone())
            .unwrap_or_else(default_frames);
        self.interval_ms = indicator
            .as_ref()
            .and_then(|indicator| indicator.interval_ms)
            .filter(|interval| *interval > 0)
            .unwrap_or(DEFAULT_INTERVAL_MS);
        self.current_frame = 0;
        self.start();
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    pub fn render_ratatui(&mut self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        for (row, line) in self
            .render(area.width as usize)
            .into_iter()
            .take(area.height as usize)
            .enumerate()
        {
            buffer.set_stringn(
                area.x,
                area.y + row as u16,
                line,
                area.width as usize,
                Style::default(),
            );
        }
    }

    fn update_display(&mut self) {
        let frame = self
            .frames
            .get(self.current_frame)
            .map_or("", String::as_str);
        let rendered_frame = if self.render_indicator_verbatim {
            frame.to_string()
        } else {
            (self.spinner_color_fn)(frame)
        };
        let indicator = if frame.is_empty() {
            String::new()
        } else {
            format!("{rendered_frame} ")
        };
        self.text.set_text(format!(
            "{indicator}{}",
            (self.message_color_fn)(&self.message)
        ));
    }
}

impl Component for Loader {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = vec![String::new()];
        lines.extend(self.text.render(width));
        lines
    }

    fn invalidate(&mut self) {
        self.text.invalidate();
    }
}

fn default_frames() -> Vec<String> {
    DEFAULT_FRAMES
        .iter()
        .map(|frame| (*frame).to_string())
        .collect()
}
