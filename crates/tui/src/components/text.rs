use super::{BackgroundFn, Component};
use crate::{apply_background_to_line, visible_width, wrap_text_with_ansi};

#[derive(Clone)]
pub struct Text {
    text: String,
    padding_x: usize,
    padding_y: usize,
    custom_bg_fn: Option<BackgroundFn>,
    cached_text: Option<String>,
    cached_width: Option<usize>,
    cached_lines: Option<Vec<String>>,
}

impl Text {
    pub fn new(text: impl Into<String>, padding_x: usize, padding_y: usize) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
            custom_bg_fn: None,
            cached_text: None,
            cached_width: None,
            cached_lines: None,
        }
    }

    pub fn with_background(mut self, bg_fn: BackgroundFn) -> Self {
        self.custom_bg_fn = Some(bg_fn);
        self.invalidate();
        self
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.invalidate();
    }

    pub fn set_custom_bg_fn(&mut self, custom_bg_fn: Option<BackgroundFn>) {
        self.custom_bg_fn = custom_bg_fn;
        self.invalidate();
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new("", 1, 1)
    }
}

impl Component for Text {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.cached_text.as_deref() == Some(self.text.as_str())
            && self.cached_width == Some(width)
            && self.cached_lines.is_some()
        {
            return self.cached_lines.clone().unwrap_or_default();
        }

        if self.text.trim().is_empty() {
            self.cached_text = Some(self.text.clone());
            self.cached_width = Some(width);
            self.cached_lines = Some(Vec::new());
            return Vec::new();
        }

        let normalized_text = self.text.replace('\t', "   ");
        let content_width = width.saturating_sub(self.padding_x * 2).max(1);
        let wrapped_lines = wrap_text_with_ansi(&normalized_text, content_width);
        let left_margin = " ".repeat(self.padding_x);
        let right_margin = " ".repeat(self.padding_x);
        let mut content_lines = Vec::new();

        for line in wrapped_lines {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            content_lines.push(pad_or_background(
                &line_with_margins,
                width,
                self.custom_bg_fn.as_ref(),
            ));
        }

        let empty_line = " ".repeat(width);
        let empty_lines = (0..self.padding_y)
            .map(|_| pad_or_background(&empty_line, width, self.custom_bg_fn.as_ref()))
            .collect::<Vec<_>>();

        let mut result = Vec::new();
        result.extend(empty_lines.iter().cloned());
        result.extend(content_lines);
        result.extend(empty_lines);

        self.cached_text = Some(self.text.clone());
        self.cached_width = Some(width);
        self.cached_lines = Some(result.clone());

        if result.is_empty() {
            vec![String::new()]
        } else {
            result
        }
    }

    fn invalidate(&mut self) {
        self.cached_text = None;
        self.cached_width = None;
        self.cached_lines = None;
    }
}

fn pad_or_background(line: &str, width: usize, bg_fn: Option<&BackgroundFn>) -> String {
    if let Some(bg_fn) = bg_fn {
        apply_background_to_line(line, width, |text| bg_fn(text))
    } else {
        let padding_needed = width.saturating_sub(visible_width(line));
        format!("{line}{}", " ".repeat(padding_needed))
    }
}
