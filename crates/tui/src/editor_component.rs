use crate::components::{Component, Editor};
use crate::{CombinedAutocompleteProvider, KeybindingsManager};

/// 自定义编辑器组件契约，对齐 pi 的 EditorComponent 公共接口。
///
/// 这个 trait 让调用方可以依赖编辑器能力边界，而不是绑定到默认 Editor 实现。
pub trait EditorComponent: Component {
    fn get_text(&self) -> String;

    fn set_text(&mut self, text: &str);

    fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager);

    fn add_to_history(&mut self, text: &str) {
        let _ = text;
    }

    fn insert_text_at_cursor(&mut self, text: &str) {
        let _ = text;
    }

    fn get_expanded_text(&self) -> String {
        self.get_text()
    }

    fn set_autocomplete_provider(&mut self, provider: CombinedAutocompleteProvider) {
        let _ = provider;
    }

    fn set_padding_x(&mut self, padding: usize) {
        let _ = padding;
    }

    fn set_autocomplete_max_visible(&mut self, max_visible: usize) {
        let _ = max_visible;
    }
}

impl EditorComponent for Editor {
    fn get_text(&self) -> String {
        self.get_text()
    }

    fn set_text(&mut self, text: &str) {
        self.set_text(text);
    }

    fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        self.handle_input(data, keybindings);
    }

    fn add_to_history(&mut self, text: &str) {
        self.add_to_history(text);
    }

    fn insert_text_at_cursor(&mut self, text: &str) {
        self.insert_text_at_cursor(text);
    }

    fn get_expanded_text(&self) -> String {
        self.get_expanded_text()
    }

    fn set_autocomplete_provider(&mut self, provider: CombinedAutocompleteProvider) {
        self.set_autocomplete_provider(provider);
    }

    fn set_padding_x(&mut self, padding: usize) {
        self.set_padding_x(padding);
    }

    fn set_autocomplete_max_visible(&mut self, max_visible: usize) {
        self.set_autocomplete_max_visible(max_visible);
    }
}

#[cfg(test)]
mod tests {
    use crate::components::Editor;
    use crate::{visible_width, EditorComponent, KeybindingsManager};

    #[test]
    fn editor_implements_custom_editor_component_contract() {
        let mut editor = Editor::default();
        let component: &mut dyn EditorComponent = &mut editor;

        component.set_text("hello");
        component.insert_text_at_cursor(" world");
        assert_eq!(component.get_text(), "hello world");

        component.set_text("before [paste #1] after");
        assert_eq!(component.get_expanded_text(), "before [paste #1] after");

        component.add_to_history("older");
        component.set_text("");
        component.handle_input("\x1b[A", &KeybindingsManager::default());
        assert_eq!(component.get_text(), "older");

        assert_eq!(visible_width(&component.render(12)[0]), 12);
    }
}
