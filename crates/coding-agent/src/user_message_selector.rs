use std::ops::Range;
use tui::{truncate_to_width, KeybindingsManager};

const MAX_VISIBLE_MESSAGES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageItem {
    pub id: String,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserMessageSelectorAction {
    None,
    Select(String),
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageSelectorState {
    messages: Vec<UserMessageItem>,
    selected_index: usize,
}

impl UserMessageSelectorState {
    pub fn new(messages: Vec<UserMessageItem>, initial_selected_id: Option<String>) -> Self {
        let initial_index = initial_selected_id
            .as_deref()
            .and_then(|id| messages.iter().position(|message| message.id == id));
        let selected_index = initial_index.unwrap_or_else(|| messages.len().saturating_sub(1));

        Self {
            messages,
            selected_index,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_item(&self) -> Option<&UserMessageItem> {
        self.messages.get(self.selected_index)
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_item().map(|message| message.id.as_str())
    }

    pub fn visible_range(&self) -> Range<usize> {
        if self.messages.is_empty() {
            return 0..0;
        }

        let half_window = MAX_VISIBLE_MESSAGES / 2;
        let max_start = self.messages.len().saturating_sub(MAX_VISIBLE_MESSAGES);
        let centered_start = self.selected_index.saturating_sub(half_window);
        let start = centered_start.min(max_start);
        let end = (start + MAX_VISIBLE_MESSAGES).min(self.messages.len());
        start..end
    }

    pub fn render_lines(&self, width: usize) -> Vec<String> {
        if self.messages.is_empty() {
            return vec!["  No user messages found".to_string()];
        }

        let visible_range = self.visible_range();
        let mut lines = Vec::new();

        for index in visible_range.clone() {
            let message = &self.messages[index];
            let normalized_message = message.text.replace('\n', " ").trim().to_string();
            let cursor = if index == self.selected_index {
                "› "
            } else {
                "  "
            };
            let max_message_width = width.saturating_sub(2);
            let truncated_message =
                truncate_to_width(&normalized_message, max_message_width, "", false);

            lines.push(format!("{cursor}{truncated_message}"));
            lines.push(format!(
                "  Message {} of {}",
                index + 1,
                self.messages.len()
            ));
            lines.push(String::new());
        }

        if visible_range.start > 0 || visible_range.end < self.messages.len() {
            lines.push(format!(
                "  ({}/{})",
                self.selected_index + 1,
                self.messages.len()
            ));
        }

        lines
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> UserMessageSelectorAction {
        if keybindings.matches(key_data, "tui.select.up") {
            self.move_up();
            UserMessageSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.down") {
            self.move_down();
            UserMessageSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.confirm") {
            self.selected_id()
                .map(|id| UserMessageSelectorAction::Select(id.to_string()))
                .unwrap_or(UserMessageSelectorAction::None)
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            UserMessageSelectorAction::Cancel
        } else {
            UserMessageSelectorAction::None
        }
    }

    fn move_up(&mut self) {
        if self.messages.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index == 0 {
            self.selected_index = self.messages.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.messages.is_empty() || self.selected_index + 1 >= self.messages.len() {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UserMessageItem, UserMessageSelectorAction, UserMessageSelectorState};
    use crate::keybindings::app_keybindings;
    use std::collections::BTreeMap;
    use tui::KeybindingsManager;

    #[test]
    fn user_message_selector_initial_selection_uses_initial_id_or_most_recent() {
        let messages = messages(3);

        let state = UserMessageSelectorState::new(messages.clone(), Some("msg-1".to_string()));
        assert_eq!(state.selected_index(), 1);
        assert_eq!(state.selected_id(), Some("msg-1"));

        let state = UserMessageSelectorState::new(messages, Some("missing".to_string()));
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.selected_id(), Some("msg-2"));
    }

    #[test]
    fn user_message_selector_wraps_up_and_down_like_pi() {
        let mut state = UserMessageSelectorState::new(messages(2), None);
        let keybindings = keybindings();

        assert_eq!(state.selected_id(), Some("msg-1"));
        assert_eq!(
            state.handle_input("\x1b[B", &keybindings),
            UserMessageSelectorAction::None
        );
        assert_eq!(state.selected_id(), Some("msg-0"));
        assert_eq!(
            state.handle_input("\x1b[A", &keybindings),
            UserMessageSelectorAction::None
        );
        assert_eq!(state.selected_id(), Some("msg-1"));
    }

    #[test]
    fn user_message_selector_confirms_selected_message_and_cancels() {
        let mut state = UserMessageSelectorState::new(messages(2), None);
        let keybindings = keybindings();

        assert_eq!(
            state.handle_input("\r", &keybindings),
            UserMessageSelectorAction::Select("msg-1".to_string())
        );
        assert_eq!(
            state.handle_input("\x1b", &keybindings),
            UserMessageSelectorAction::Cancel
        );
    }

    #[test]
    fn user_message_selector_empty_state_renders_message_and_does_not_select() {
        let mut state = UserMessageSelectorState::new(Vec::new(), None);

        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.selected_id(), None);
        assert_eq!(state.render_lines(40), vec!["  No user messages found"]);
        assert_eq!(
            state.handle_input("\r", &keybindings()),
            UserMessageSelectorAction::None
        );
    }

    #[test]
    fn user_message_selector_render_lines_match_pi_window_and_metadata() {
        let mut long_messages = messages(12);
        long_messages[6].text = "line one\nline two".to_string();
        let state = UserMessageSelectorState::new(long_messages, Some("msg-6".to_string()));

        assert_eq!(state.visible_range(), 1..11);
        assert_eq!(
            state.render_lines(14),
            vec![
                "  message 1".to_string(),
                "  Message 2 of 12".to_string(),
                "".to_string(),
                "  message 2".to_string(),
                "  Message 3 of 12".to_string(),
                "".to_string(),
                "  message 3".to_string(),
                "  Message 4 of 12".to_string(),
                "".to_string(),
                "  message 4".to_string(),
                "  Message 5 of 12".to_string(),
                "".to_string(),
                "  message 5".to_string(),
                "  Message 6 of 12".to_string(),
                "".to_string(),
                "› line one lin\u{1b}[0m".to_string(),
                "  Message 7 of 12".to_string(),
                "".to_string(),
                "  message 7".to_string(),
                "  Message 8 of 12".to_string(),
                "".to_string(),
                "  message 8".to_string(),
                "  Message 9 of 12".to_string(),
                "".to_string(),
                "  message 9".to_string(),
                "  Message 10 of 12".to_string(),
                "".to_string(),
                "  message 10".to_string(),
                "  Message 11 of 12".to_string(),
                "".to_string(),
                "  (7/12)".to_string(),
            ]
        );
    }

    fn messages(count: usize) -> Vec<UserMessageItem> {
        (0..count)
            .map(|index| UserMessageItem {
                id: format!("msg-{index}"),
                text: format!("message {index}"),
                timestamp: None,
            })
            .collect()
    }

    fn keybindings() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }
}
