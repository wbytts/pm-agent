pub const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
pub const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
pub const OSC133_ZONE_FINAL: &str = "\x1b]133;C\x07";

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

        if let Some(first) = lines.first_mut() {
            first.insert_str(0, OSC133_ZONE_START);
        }
        if let Some(last) = lines.last_mut() {
            last.insert_str(0, &format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}"));
        }

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
}
