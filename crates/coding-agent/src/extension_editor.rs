use std::sync::{Arc, Mutex};

use tui::{components::Editor, KeybindingsManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionEditorAction {
    None,
    Submit(String),
    Cancel,
    OpenExternalEditor { current_text: String },
    ExternalEditorFinished { updated: bool },
}

pub struct ExtensionEditorState {
    title: String,
    editor: Editor,
    focused: bool,
    submitted: Arc<Mutex<Option<String>>>,
}

impl ExtensionEditorState {
    pub fn new(title: impl Into<String>, prefill: Option<&str>) -> Self {
        let submitted = Arc::new(Mutex::new(None));
        let submitted_for_callback = submitted.clone();
        let mut editor = Editor::default();
        if let Some(prefill) = prefill {
            editor.set_text(prefill);
        }
        editor.set_on_submit(move |value| {
            *submitted_for_callback.lock().expect("lock submitted") = Some(value.to_string());
        });

        Self {
            title: title.into(),
            editor,
            focused: false,
            submitted,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn text(&self) -> String {
        self.editor.get_text()
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.editor.set_focused(focused);
    }

    pub fn hint(&self, has_external_editor: bool) -> String {
        let mut hint = "enter submit  shift+enter newline  escape cancel".to_string();
        if has_external_editor {
            hint.push_str("  ctrl+g external editor");
        }
        hint
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
        has_external_editor: bool,
    ) -> ExtensionEditorAction {
        if keybindings.matches(key_data, "tui.select.cancel") {
            return ExtensionEditorAction::Cancel;
        }

        if keybindings.matches(key_data, "app.editor.external") {
            if has_external_editor {
                return ExtensionEditorAction::OpenExternalEditor {
                    current_text: self.editor.get_text(),
                };
            }
            return ExtensionEditorAction::None;
        }

        self.editor.handle_input(key_data, keybindings);
        if let Some(value) = self.submitted.lock().expect("lock submitted").take() {
            return ExtensionEditorAction::Submit(value);
        }

        ExtensionEditorAction::None
    }

    pub fn apply_external_editor_result(
        &mut self,
        status: Option<i32>,
        new_content: Option<String>,
    ) -> ExtensionEditorAction {
        if status == Some(0) {
            if let Some(content) = new_content {
                self.editor
                    .set_text(content.strip_suffix('\n').unwrap_or(&content));
                return ExtensionEditorAction::ExternalEditorFinished { updated: true };
            }
        }

        ExtensionEditorAction::ExternalEditorFinished { updated: false }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::keybindings::app_keybindings;
    use tui::KeybindingsManager;

    fn kb() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }

    #[test]
    fn extension_editor_initializes_title_prefill_focus_and_hint() {
        let mut state = ExtensionEditorState::new("Edit extension input", Some("prefilled text"));

        assert_eq!(state.title(), "Edit extension input");
        assert_eq!(state.text(), "prefilled text");
        assert!(!state.focused());
        state.set_focused(true);
        assert!(state.focused());
        assert!(state.hint(false).contains("enter submit"));
        assert!(!state.hint(false).contains("external editor"));
        assert!(state.hint(true).contains("ctrl+g external editor"));
    }

    #[test]
    fn extension_editor_cancels_before_editor_receives_escape() {
        let mut state = ExtensionEditorState::new("Title", Some("draft"));

        let action = state.handle_input("\x1b", &kb(), true);

        assert_eq!(action, ExtensionEditorAction::Cancel);
        assert_eq!(state.text(), "draft");
    }

    #[test]
    fn extension_editor_requests_external_editor_when_configured() {
        let mut state = ExtensionEditorState::new("Title", Some("draft"));

        let action = state.handle_input("\x07", &kb(), true);

        assert_eq!(
            action,
            ExtensionEditorAction::OpenExternalEditor {
                current_text: "draft".to_string()
            }
        );
        assert_eq!(state.text(), "draft");
    }

    #[test]
    fn extension_editor_ignores_external_shortcut_without_editor_command() {
        let mut state = ExtensionEditorState::new("Title", Some("draft"));

        let action = state.handle_input("\x07", &kb(), false);

        assert_eq!(action, ExtensionEditorAction::None);
        assert_eq!(state.text(), "draft");
    }

    #[test]
    fn extension_editor_delegates_text_editing_and_submit_to_inner_editor() {
        let mut state = ExtensionEditorState::new("Title", None);

        assert_eq!(
            state.handle_input("hello", &kb(), false),
            ExtensionEditorAction::None
        );
        assert_eq!(
            state.handle_input("\n", &kb(), false),
            ExtensionEditorAction::None
        );
        assert_eq!(
            state.handle_input("world", &kb(), false),
            ExtensionEditorAction::None
        );
        assert_eq!(
            state.handle_input("\r", &kb(), false),
            ExtensionEditorAction::Submit("hello\nworld".to_string())
        );
        assert_eq!(state.text(), "");
    }

    #[test]
    fn extension_editor_applies_successful_external_edit_like_pi() {
        let mut state = ExtensionEditorState::new("Title", Some("before"));

        assert_eq!(
            state.apply_external_editor_result(Some(0), Some("after\n".to_string())),
            ExtensionEditorAction::ExternalEditorFinished { updated: true }
        );

        assert_eq!(state.text(), "after");
    }

    #[test]
    fn extension_editor_keeps_text_when_external_editor_fails_or_errors() {
        let mut state = ExtensionEditorState::new("Title", Some("before"));

        assert_eq!(
            state.apply_external_editor_result(Some(1), Some("after".to_string())),
            ExtensionEditorAction::ExternalEditorFinished { updated: false }
        );
        assert_eq!(
            state.apply_external_editor_result(None, Some("after".to_string())),
            ExtensionEditorAction::ExternalEditorFinished { updated: false }
        );

        assert_eq!(state.text(), "before");
    }
}
