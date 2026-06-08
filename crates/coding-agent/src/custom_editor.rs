use std::collections::BTreeSet;

use tui::{components::Editor, KeybindingsManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomEditorAction {
    ExtensionShortcut,
    PasteImage,
    Interrupt,
    Exit,
    AppAction(String),
    Editor,
}

pub struct CustomEditor {
    editor: Editor,
    action_handlers: BTreeSet<String>,
    on_escape: bool,
    on_ctrl_d: bool,
}

impl Default for CustomEditor {
    fn default() -> Self {
        Self::new(Editor::default())
    }
}

impl CustomEditor {
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            action_handlers: BTreeSet::new(),
            on_escape: false,
            on_ctrl_d: false,
        }
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    pub fn on_action(&mut self, action: impl Into<String>) {
        self.action_handlers.insert(action.into());
    }

    pub fn set_on_escape(&mut self, enabled: bool) {
        self.on_escape = enabled;
    }

    pub fn set_on_ctrl_d(&mut self, enabled: bool) {
        self.on_ctrl_d = enabled;
    }

    pub fn handle_input<F>(
        &mut self,
        data: &str,
        keybindings: &KeybindingsManager,
        mut extension_shortcut: F,
    ) -> CustomEditorAction
    where
        F: FnMut(&str) -> bool,
    {
        if extension_shortcut(data) {
            return CustomEditorAction::ExtensionShortcut;
        }

        if keybindings.matches(data, "app.clipboard.pasteImage") {
            return CustomEditorAction::PasteImage;
        }

        if keybindings.matches(data, "app.interrupt") {
            if !self.editor.is_showing_autocomplete()
                && (self.on_escape || self.action_handlers.contains("app.interrupt"))
            {
                return CustomEditorAction::Interrupt;
            }
            self.editor.handle_input(data, keybindings);
            return CustomEditorAction::Editor;
        }

        if keybindings.matches(data, "app.exit") {
            if self.editor.get_text().is_empty() {
                if self.on_ctrl_d || self.action_handlers.contains("app.exit") {
                    return CustomEditorAction::Exit;
                }
                return CustomEditorAction::Editor;
            }
        }

        for action in self.action_handlers.iter() {
            if action == "app.interrupt" || action == "app.exit" {
                continue;
            }
            if keybindings.matches(data, action) {
                return CustomEditorAction::AppAction(action.clone());
            }
        }

        self.editor.handle_input(data, keybindings);
        CustomEditorAction::Editor
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::keybindings::app_keybindings;
    use tui::{
        components::{Editor, EditorOptions, EditorTheme},
        CombinedAutocompleteProvider, KeybindingsManager, SlashCommand,
    };

    fn kb() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }

    #[test]
    fn extension_shortcut_has_highest_priority_and_does_not_edit_text() {
        let mut editor = CustomEditor::default();

        let action = editor.handle_input("x", &kb(), |data| data == "x");

        assert_eq!(action, CustomEditorAction::ExtensionShortcut);
        assert_eq!(editor.editor().get_text(), "");
    }

    #[test]
    fn paste_image_keybinding_runs_before_editor_input() {
        let mut editor = CustomEditor::default();

        let action = editor.handle_input("\x16", &kb(), |_| false);

        assert_eq!(action, CustomEditorAction::PasteImage);
        assert_eq!(editor.editor().get_text(), "");
    }

    #[test]
    fn escape_without_autocomplete_returns_interrupt() {
        let mut editor = CustomEditor::default();
        editor.on_action("app.interrupt");

        let action = editor.handle_input("\x1b", &kb(), |_| false);

        assert_eq!(action, CustomEditorAction::Interrupt);
        assert_eq!(editor.editor().get_text(), "");
    }

    #[test]
    fn escape_with_autocomplete_falls_back_to_editor() {
        let mut editor = CustomEditor::default();
        editor.on_action("app.interrupt");
        editor
            .editor_mut()
            .set_autocomplete_provider(CombinedAutocompleteProvider::new(
                vec![SlashCommand {
                    name: "help".to_string(),
                    description: Some("show help".to_string()),
                    argument_hint: None,
                }],
                ".",
            ));
        editor.editor_mut().handle_input("/", &kb());
        editor.editor_mut().handle_input("h", &kb());
        assert!(editor.editor().is_showing_autocomplete());

        let action = editor.handle_input("\x1b", &kb(), |_| false);

        assert_eq!(action, CustomEditorAction::Editor);
        assert!(!editor.editor().is_showing_autocomplete());
        assert_eq!(editor.editor().get_text(), "/h");
    }

    #[test]
    fn ctrl_d_exits_only_when_editor_is_empty() {
        let mut editor = CustomEditor::default();
        editor.on_action("app.exit");

        let empty_action = editor.handle_input("\x04", &kb(), |_| false);
        editor.editor_mut().set_text("ab");
        editor.editor_mut().handle_input("\x1b[D", &kb());
        let editing_action = editor.handle_input("\x04", &kb(), |_| false);

        assert_eq!(empty_action, CustomEditorAction::Exit);
        assert_eq!(editing_action, CustomEditorAction::Editor);
        assert_eq!(editor.editor().get_text(), "a");
    }

    #[test]
    fn registered_custom_action_runs_before_editor_input() {
        let mut editor = CustomEditor::default();
        editor.on_action("app.clear");

        let action = editor.handle_input("\x03", &kb(), |_| false);

        assert_eq!(
            action,
            CustomEditorAction::AppAction("app.clear".to_string())
        );
        assert_eq!(editor.editor().get_text(), "");
    }

    #[test]
    fn unhandled_input_falls_back_to_editor() {
        let mut editor = CustomEditor::new(Editor::new(
            EditorTheme::default(),
            EditorOptions::default(),
        ));

        let action = editor.handle_input("hello", &kb(), |_| false);

        assert_eq!(action, CustomEditorAction::Editor);
        assert_eq!(editor.editor().get_text(), "hello");
    }
}
