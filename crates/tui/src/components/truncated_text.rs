use super::Component;
use crate::{truncate_to_width, visible_width};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncatedText {
    text: String,
    padding_x: usize,
    padding_y: usize,
}

impl TruncatedText {
    pub fn new(text: impl Into<String>, padding_x: usize, padding_y: usize) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }
}

impl Component for TruncatedText {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut result = Vec::new();
        let empty_line = " ".repeat(width);

        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        let available_width = width.saturating_sub(self.padding_x * 2).max(1);
        let single_line_text = self.text.split('\n').next().unwrap_or_default();
        let display_text = truncate_to_width(single_line_text, available_width, "...", false);
        let line_with_padding = format!(
            "{}{}{}",
            " ".repeat(self.padding_x),
            display_text,
            " ".repeat(self.padding_x)
        );
        let final_line = format!(
            "{}{}",
            line_with_padding,
            " ".repeat(width.saturating_sub(visible_width(&line_with_padding)))
        );
        result.push(final_line);

        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        result
    }
}
