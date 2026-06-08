use tui::KeybindingsManager;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionSelectorAction {
    None,
    Select(String),
    Cancel,
    ToggleToolsExpanded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSelectorState {
    options: Vec<String>,
    selected_index: usize,
}

impl ExtensionSelectorState {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            options,
            selected_index: 0,
        }
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_option(&self) -> Option<&str> {
        self.options.get(self.selected_index).map(String::as_str)
    }

    pub fn render_rows(&self) -> Vec<String> {
        self.options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                if index == self.selected_index {
                    format!("→ {option}")
                } else {
                    format!("  {option}")
                }
            })
            .collect()
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> ExtensionSelectorAction {
        if keybindings.matches(key_data, "app.tools.expand") {
            ExtensionSelectorAction::ToggleToolsExpanded
        } else if keybindings.matches(key_data, "tui.select.up") || key_data == "k" {
            self.move_up();
            ExtensionSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.down") || key_data == "j" {
            self.move_down();
            ExtensionSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.confirm") || key_data == "\n" {
            self.selected_option()
                .map(|option| ExtensionSelectorAction::Select(option.to_string()))
                .unwrap_or(ExtensionSelectorAction::None)
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            ExtensionSelectorAction::Cancel
        } else {
            ExtensionSelectorAction::None
        }
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn move_down(&mut self) {
        if self.options.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = (self.selected_index + 1).min(self.options.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtensionSelectorAction, ExtensionSelectorState};
    use crate::keybindings::app_keybindings;
    use std::collections::BTreeMap;
    use tui::KeybindingsManager;

    #[test]
    fn extension_selector_starts_on_first_option_and_clamps_navigation_like_pi() {
        let mut state = state(vec!["one", "two"]);

        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.selected_option(), Some("one"));

        assert_eq!(
            state.handle_input("k", &keybindings()),
            ExtensionSelectorAction::None
        );
        assert_eq!(state.selected_index(), 0);

        assert_eq!(
            state.handle_input("j", &keybindings()),
            ExtensionSelectorAction::None
        );
        assert_eq!(state.selected_index(), 1);
        assert_eq!(
            state.handle_input("j", &keybindings()),
            ExtensionSelectorAction::None
        );
        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn extension_selector_handles_keyboard_actions_like_pi_component() {
        let mut state = state(vec!["alpha", "beta"]);
        let keybindings = keybindings();

        assert_eq!(
            state.handle_input("\x1b[B", &keybindings),
            ExtensionSelectorAction::None
        );
        assert_eq!(
            state.handle_input("\r", &keybindings),
            ExtensionSelectorAction::Select("beta".to_string())
        );
        assert_eq!(
            state.handle_input("\x1b", &keybindings),
            ExtensionSelectorAction::Cancel
        );
        assert_eq!(
            state.handle_input("\x0f", &keybindings),
            ExtensionSelectorAction::ToggleToolsExpanded
        );
    }

    #[test]
    fn extension_selector_confirm_on_empty_options_does_not_select() {
        let mut state = state(Vec::new());

        assert_eq!(state.selected_option(), None);
        assert_eq!(
            state.handle_input("\n", &keybindings()),
            ExtensionSelectorAction::None
        );
    }

    #[test]
    fn extension_selector_render_rows_mark_selected_option() {
        let mut state = state(vec!["alpha", "beta"]);
        state.handle_input("j", &keybindings());

        assert_eq!(
            state.render_rows(),
            vec!["  alpha".to_string(), "→ beta".to_string()]
        );
    }

    fn state(options: Vec<&str>) -> ExtensionSelectorState {
        ExtensionSelectorState::new(options.into_iter().map(str::to_string).collect())
    }

    fn keybindings() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }
}
