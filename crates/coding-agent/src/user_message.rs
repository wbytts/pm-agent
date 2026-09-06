pub const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
pub const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
pub const OSC133_ZONE_FINAL: &str = "\x1b]133;C\x07";

pub fn add_osc133_zone_markers(lines: &mut [String]) {
    if lines.is_empty() {
        return;
    }
    if let Some(first) = lines.first_mut() {
        first.insert_str(0, OSC133_ZONE_START);
    }
    if let Some(last) = lines.last_mut() {
        last.insert_str(0, &format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}"));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageState {
    text: String,
}

impl UserMessageState {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn render_lines(&self) -> Vec<String> {
        if self.text.is_empty() {
            return Vec::new();
        }

        let mut lines = self.text.lines().map(str::to_string).collect::<Vec<_>>();
        if lines.is_empty() {
            return lines;
        }

        add_osc133_zone_markers(&mut lines);

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_renders_text_with_osc133_zone_like_pi() {
        let state = UserMessageState::new("hello\nworld");

        assert_eq!(
            state.render_lines(),
            vec![
                format!("{OSC133_ZONE_START}hello"),
                format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}world"),
            ]
        );
    }

    #[test]
    fn user_message_preserves_empty_render_without_zone_markers() {
        let state = UserMessageState::new("");

        assert!(state.render_lines().is_empty());
    }

    #[test]
    fn osc133_zone_markers_wrap_first_and_last_rendered_lines_like_pi() {
        let mut lines = vec!["top".to_string(), "bottom".to_string()];

        add_osc133_zone_markers(&mut lines);

        assert_eq!(lines[0], format!("{OSC133_ZONE_START}top"));
        assert_eq!(
            lines[1],
            format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}bottom")
        );
    }
}
