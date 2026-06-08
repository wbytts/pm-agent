use tui::components::Input;
use tui::KeybindingsManager;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionInputAction {
    None,
    Submit(String),
    Cancel,
}

pub struct ExtensionInputState {
    input: Input,
}

impl ExtensionInputState {
    pub fn new() -> Self {
        Self {
            input: Input::new(),
        }
    }

    pub fn value(&self) -> &str {
        self.input.value()
    }

    pub fn focused(&self) -> bool {
        self.input.focused()
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.input.set_focused(focused);
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> ExtensionInputAction {
        if keybindings.matches(key_data, "tui.select.confirm") || key_data == "\n" {
            ExtensionInputAction::Submit(self.input.value().to_string())
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            ExtensionInputAction::Cancel
        } else {
            self.input.handle_input(key_data, keybindings);
            ExtensionInputAction::None
        }
    }
}

impl Default for ExtensionInputState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtensionInputAction, ExtensionInputState};
    use crate::keybindings::app_keybindings;
    use std::collections::BTreeMap;
    use tui::KeybindingsManager;

    #[test]
    fn extension_input_delegates_text_editing_to_tui_input() {
        let mut state = ExtensionInputState::new();

        assert_eq!(
            state.handle_input("hello", &keybindings()),
            ExtensionInputAction::None
        );
        assert_eq!(state.value(), "hello");

        state.handle_input("\x7f", &keybindings());

        assert_eq!(state.value(), "hell");
    }

    #[test]
    fn extension_input_submit_returns_current_value_like_pi_component() {
        let mut state = ExtensionInputState::new();
        let keybindings = keybindings();

        state.handle_input("alpha", &keybindings);

        assert_eq!(
            state.handle_input("\r", &keybindings),
            ExtensionInputAction::Submit("alpha".to_string())
        );
        assert_eq!(
            state.handle_input("\n", &keybindings),
            ExtensionInputAction::Submit("alpha".to_string())
        );
    }

    #[test]
    fn extension_input_cancel_returns_cancel_action() {
        let mut state = ExtensionInputState::new();

        assert_eq!(
            state.handle_input("\x1b", &keybindings()),
            ExtensionInputAction::Cancel
        );
    }

    #[test]
    fn extension_input_focus_state_propagates_to_inner_input() {
        let mut state = ExtensionInputState::new();

        assert!(!state.focused());
        state.set_focused(true);

        assert!(state.focused());
    }

    fn keybindings() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }
}
