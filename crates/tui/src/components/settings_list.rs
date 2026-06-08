use super::{Component, Input};
use crate::{
    fuzzy_filter, truncate_to_width, visible_width, wrap_text_with_ansi, KeybindingsManager,
};
use std::sync::{Arc, Mutex};

pub type SettingsStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;
pub type SettingsLabelStyleFn = Arc<dyn Fn(&str, bool) -> String + Send + Sync>;
pub type SettingsSubmenuFactory =
    Arc<dyn Fn(&str, SettingsSubmenuDone) -> Box<dyn SettingsSubmenu> + Send + Sync>;
pub type SettingsSubmenuDone = Box<dyn FnMut(Option<String>) + Send>;

pub trait SettingsSubmenu: Component + Send {
    fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager);
}

#[derive(Clone)]
pub struct SettingItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub current_value: String,
    pub values: Vec<String>,
    pub submenu: Option<SettingsSubmenuFactory>,
}

impl SettingItem {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        current_value: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            current_value: current_value.into(),
            values: Vec::new(),
            submenu: None,
        }
    }
}

#[derive(Clone)]
pub struct SettingsListTheme {
    pub label: SettingsLabelStyleFn,
    pub value: SettingsLabelStyleFn,
    pub description: SettingsStyleFn,
    pub cursor: String,
    pub hint: SettingsStyleFn,
}

impl Default for SettingsListTheme {
    fn default() -> Self {
        let identity: SettingsStyleFn = Arc::new(str::to_string);
        let identity_with_selected: SettingsLabelStyleFn =
            Arc::new(|text, _selected| text.to_string());
        Self {
            label: identity_with_selected.clone(),
            value: identity_with_selected,
            description: identity.clone(),
            cursor: "→ ".to_string(),
            hint: identity,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsListOptions {
    pub enable_search: bool,
}

pub struct SettingsList {
    items: Vec<SettingItem>,
    filtered_items: Vec<SettingItem>,
    theme: SettingsListTheme,
    selected_index: usize,
    max_visible: usize,
    on_change: Box<dyn FnMut(&str, &str) + Send>,
    on_cancel: Box<dyn FnMut() + Send>,
    search_input: Option<Input>,
    search_enabled: bool,
    submenu_component: Option<Box<dyn SettingsSubmenu>>,
    submenu_item_index: Option<usize>,
    submenu_pending: Option<Arc<Mutex<Option<Option<String>>>>>,
}

impl SettingsList {
    pub fn new<FChange, FCancel>(
        items: Vec<SettingItem>,
        max_visible: usize,
        theme: SettingsListTheme,
        on_change: FChange,
        on_cancel: FCancel,
        options: SettingsListOptions,
    ) -> Self
    where
        FChange: FnMut(&str, &str) + Send + 'static,
        FCancel: FnMut() + Send + 'static,
    {
        let search_enabled = options.enable_search;
        Self {
            filtered_items: items.clone(),
            items,
            theme,
            selected_index: 0,
            max_visible,
            on_change: Box::new(on_change),
            on_cancel: Box::new(on_cancel),
            search_input: search_enabled.then(Input::new),
            search_enabled,
            submenu_component: None,
            submenu_item_index: None,
            submenu_pending: None,
        }
    }

    pub fn update_value(&mut self, id: &str, new_value: impl Into<String>) {
        let new_value = new_value.into();
        self.set_item_value(id, &new_value);
    }

    pub fn selected_item(&self) -> Option<&SettingItem> {
        self.display_items().get(self.selected_index)
    }

    pub fn search_value(&self) -> Option<&str> {
        self.search_input.as_ref().map(Input::value)
    }

    pub fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        if let Some(submenu) = self.submenu_component.as_mut() {
            submenu.handle_input(data, keybindings);
            self.consume_submenu_result();
            return;
        }

        let display_len = self.display_items().len();
        if keybindings.matches(data, "tui.select.up") {
            if display_len > 0 {
                self.selected_index = if self.selected_index == 0 {
                    display_len - 1
                } else {
                    self.selected_index - 1
                };
            }
        } else if keybindings.matches(data, "tui.select.down") {
            if display_len > 0 {
                self.selected_index = if self.selected_index == display_len - 1 {
                    0
                } else {
                    self.selected_index + 1
                };
            }
        } else if keybindings.matches(data, "tui.select.confirm") || data == " " {
            self.activate_item();
        } else if keybindings.matches(data, "tui.select.cancel") {
            (self.on_cancel)();
        } else if self.search_enabled {
            let sanitized = data.replace(' ', "");
            if sanitized.is_empty() {
                return;
            }
            if let Some(search_input) = self.search_input.as_mut() {
                search_input.handle_input(&sanitized, keybindings);
                let query = search_input.value().to_string();
                self.apply_filter(&query);
            }
        }
    }

    fn render_main_list(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if self.search_enabled {
            if let Some(search_input) = self.search_input.as_mut() {
                lines.extend(search_input.render(width));
                lines.push(String::new());
            }
        }

        if self.items.is_empty() {
            lines.push((self.theme.hint)("  No settings available"));
            if self.search_enabled {
                self.add_hint_line(&mut lines, width);
            }
            return lines;
        }

        let display_items = self.display_items();
        if display_items.is_empty() {
            lines.push(truncate_to_width(
                &(self.theme.hint)("  No matching settings"),
                width,
                "",
                false,
            ));
            self.add_hint_line(&mut lines, width);
            return lines;
        }

        let start_index = self
            .selected_index
            .saturating_sub(self.max_visible / 2)
            .min(display_items.len().saturating_sub(self.max_visible));
        let end_index = (start_index + self.max_visible).min(display_items.len());
        let max_label_width = self
            .items
            .iter()
            .map(|item| visible_width(&item.label))
            .max()
            .unwrap_or(0)
            .min(30);

        for index in start_index..end_index {
            if let Some(item) = self.display_items().get(index) {
                lines.push(self.render_item(
                    item,
                    index == self.selected_index,
                    width,
                    max_label_width,
                ));
            }
        }

        let display_len = self.display_items().len();
        if start_index > 0 || end_index < display_len {
            let scroll_text = format!("  ({}/{})", self.selected_index + 1, display_len);
            lines.push((self.theme.hint)(&truncate_to_width(
                &scroll_text,
                width.saturating_sub(2),
                "",
                false,
            )));
        }

        let selected_description = self
            .selected_item()
            .and_then(|item| item.description.clone());
        if let Some(description) = selected_description {
            lines.push(String::new());
            for line in wrap_text_with_ansi(&description, width.saturating_sub(4)) {
                lines.push((self.theme.description)(&format!("  {line}")));
            }
        }

        self.add_hint_line(&mut lines, width);
        lines
    }

    fn render_item(
        &self,
        item: &SettingItem,
        is_selected: bool,
        width: usize,
        max_label_width: usize,
    ) -> String {
        let prefix = if is_selected {
            &self.theme.cursor
        } else {
            "  "
        };
        let prefix_width = visible_width(prefix);
        let label_padding = " ".repeat(max_label_width.saturating_sub(visible_width(&item.label)));
        let label_text = (self.theme.label)(&format!("{}{label_padding}", item.label), is_selected);
        let separator = "  ";
        let used_width = prefix_width + max_label_width + visible_width(separator);
        let value_max_width = width.saturating_sub(used_width + 2);
        let value_text = (self.theme.value)(
            &truncate_to_width(&item.current_value, value_max_width, "", false),
            is_selected,
        );
        truncate_to_width(
            &format!("{prefix}{label_text}{separator}{value_text}"),
            width,
            "",
            false,
        )
    }

    fn activate_item(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        if let Some(submenu_factory) = item.submenu {
            self.submenu_item_index = Some(self.selected_index);
            let pending = Arc::new(Mutex::new(None));
            let pending_for_callback = pending.clone();
            let done: SettingsSubmenuDone = Box::new(move |selected_value| {
                *pending_for_callback.lock().expect("lock submenu result") = Some(selected_value);
            });
            self.submenu_component = Some(submenu_factory(&item.current_value, done));
            self.submenu_pending = Some(pending);
        } else if !item.values.is_empty() {
            let current_index = item
                .values
                .iter()
                .position(|value| value == &item.current_value)
                .unwrap_or(usize::MAX);
            let next_index = if current_index == usize::MAX {
                0
            } else {
                (current_index + 1) % item.values.len()
            };
            if let Some(new_value) = item.values.get(next_index) {
                let new_value = new_value.clone();
                self.set_item_value(&item.id, &new_value);
                (self.on_change)(&item.id, &new_value);
            }
        }
    }

    fn consume_submenu_result(&mut self) {
        let selected_value = self
            .submenu_pending
            .as_ref()
            .and_then(|pending| pending.lock().expect("lock submenu result").take());
        if let Some(selected_value) = selected_value {
            if let Some(value) = selected_value {
                if let Some(item) = self.selected_item().cloned() {
                    self.set_item_value(&item.id, &value);
                    (self.on_change)(&item.id, &value);
                }
            }
            self.close_submenu();
        }
    }

    fn close_submenu(&mut self) {
        self.submenu_component = None;
        self.submenu_pending = None;
        if let Some(index) = self.submenu_item_index.take() {
            self.selected_index = index;
        }
    }

    fn apply_filter(&mut self, query: &str) {
        self.filtered_items = fuzzy_filter(&self.items, query, |item| item.label.clone());
        self.selected_index = 0;
    }

    fn set_item_value(&mut self, id: &str, new_value: &str) {
        for item in &mut self.items {
            if item.id == id {
                item.current_value = new_value.to_string();
            }
        }
        for item in &mut self.filtered_items {
            if item.id == id {
                item.current_value = new_value.to_string();
            }
        }
    }

    fn display_items(&self) -> &[SettingItem] {
        if self.search_enabled {
            &self.filtered_items
        } else {
            &self.items
        }
    }

    fn add_hint_line(&self, lines: &mut Vec<String>, width: usize) {
        let hint = if self.search_enabled {
            "  Type to search · Enter/Space to change · Esc to cancel"
        } else {
            "  Enter/Space to change · Esc to cancel"
        };
        lines.push(String::new());
        lines.push(truncate_to_width(
            &(self.theme.hint)(hint),
            width,
            "",
            false,
        ));
    }
}

impl Component for SettingsList {
    fn render(&mut self, width: usize) -> Vec<String> {
        if let Some(submenu) = self.submenu_component.as_mut() {
            return submenu.render(width);
        }
        self.render_main_list(width)
    }

    fn invalidate(&mut self) {
        if let Some(submenu) = self.submenu_component.as_mut() {
            submenu.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct DoneSubmenu {
        done: Option<SettingsSubmenuDone>,
    }

    impl Component for DoneSubmenu {
        fn render(&mut self, _width: usize) -> Vec<String> {
            vec!["submenu".to_string()]
        }
    }

    impl SettingsSubmenu for DoneSubmenu {
        fn handle_input(&mut self, _data: &str, _keybindings: &KeybindingsManager) {
            if let Some(mut done) = self.done.take() {
                done(Some("advanced".to_string()));
            }
        }
    }

    fn item(id: &str, label: &str, current_value: &str, values: Vec<&str>) -> SettingItem {
        let mut item = SettingItem::new(id, label, current_value);
        item.values = values.into_iter().map(str::to_string).collect();
        item
    }

    #[test]
    fn settings_list_cycles_value_and_renders_description() {
        let changes = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let changes_for_callback = changes.clone();
        let mut theme = SettingsListTheme::default();
        theme.cursor = "> ".to_string();
        let mut first = item("mode", "Mode", "auto", vec!["auto", "manual"]);
        first.description = Some("Choose how the agent runs.".to_string());
        let mut list = SettingsList::new(
            vec![first],
            5,
            theme,
            move |id, value| {
                changes_for_callback
                    .lock()
                    .expect("lock changes")
                    .push((id.to_string(), value.to_string()));
            },
            || {},
            SettingsListOptions::default(),
        );

        list.handle_input("\r", &KeybindingsManager::default());
        let lines = list.render(60);

        assert_eq!(
            list.selected_item().map(|item| item.current_value.as_str()),
            Some("manual")
        );
        assert_eq!(
            *changes.lock().expect("lock changes"),
            vec![("mode".to_string(), "manual".to_string())]
        );
        assert!(lines.iter().any(|line| line.contains("Choose how")));
    }

    #[test]
    fn settings_list_searches_and_cancels() {
        let cancelled = Arc::new(Mutex::new(false));
        let cancelled_for_callback = cancelled.clone();
        let mut list = SettingsList::new(
            vec![
                item("alpha", "Alpha", "on", vec!["on", "off"]),
                item("beta", "Beta", "off", vec!["on", "off"]),
            ],
            5,
            SettingsListTheme::default(),
            |_id, _value| {},
            move || {
                *cancelled_for_callback.lock().expect("lock cancelled") = true;
            },
            SettingsListOptions {
                enable_search: true,
            },
        );

        list.handle_input("b", &KeybindingsManager::default());
        assert_eq!(list.search_value(), Some("b"));
        assert_eq!(
            list.selected_item().map(|item| item.id.as_str()),
            Some("beta")
        );
        list.handle_input("\x1b", &KeybindingsManager::default());
        assert!(*cancelled.lock().expect("lock cancelled"));
    }

    #[test]
    fn settings_list_submenu_updates_selected_value() {
        let changes = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let changes_for_callback = changes.clone();
        let mut submenu_item = item("level", "Level", "basic", vec![]);
        submenu_item.submenu = Some(Arc::new(|_current, done| {
            Box::new(DoneSubmenu { done: Some(done) })
        }));
        let mut list = SettingsList::new(
            vec![submenu_item],
            5,
            SettingsListTheme::default(),
            move |id, value| {
                changes_for_callback
                    .lock()
                    .expect("lock changes")
                    .push((id.to_string(), value.to_string()));
            },
            || {},
            SettingsListOptions::default(),
        );

        list.handle_input("\r", &KeybindingsManager::default());
        assert_eq!(list.render(20), vec!["submenu"]);
        list.handle_input("x", &KeybindingsManager::default());

        assert_eq!(
            list.selected_item().map(|item| item.current_value.as_str()),
            Some("advanced")
        );
        assert_eq!(
            *changes.lock().expect("lock changes"),
            vec![("level".to_string(), "advanced".to_string())]
        );
    }
}
