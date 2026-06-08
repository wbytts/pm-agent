use super::Component;
use crate::{truncate_to_width, visible_width, KeybindingsManager};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState, StatefulWidget, Widget};
use std::sync::Arc;

const DEFAULT_PRIMARY_COLUMN_WIDTH: usize = 32;
const PRIMARY_COLUMN_GAP: usize = 2;
const MIN_DESCRIPTION_WIDTH: usize = 10;

pub type SelectStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectItem {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct SelectListTheme {
    pub selected_prefix: SelectStyleFn,
    pub selected_text: SelectStyleFn,
    pub description: SelectStyleFn,
    pub scroll_info: SelectStyleFn,
    pub no_match: SelectStyleFn,
}

impl Default for SelectListTheme {
    fn default() -> Self {
        let identity: SelectStyleFn = Arc::new(str::to_string);
        Self {
            selected_prefix: identity.clone(),
            selected_text: identity.clone(),
            description: identity.clone(),
            scroll_info: identity.clone(),
            no_match: identity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectListTruncatePrimaryContext {
    pub text: String,
    pub max_width: usize,
    pub column_width: usize,
    pub item: SelectItem,
    pub is_selected: bool,
}

pub type TruncatePrimaryFn = Arc<dyn Fn(SelectListTruncatePrimaryContext) -> String + Send + Sync>;

#[derive(Clone, Default)]
pub struct SelectListLayoutOptions {
    pub min_primary_column_width: Option<usize>,
    pub max_primary_column_width: Option<usize>,
    pub truncate_primary: Option<TruncatePrimaryFn>,
}

pub struct SelectList {
    items: Vec<SelectItem>,
    filtered_items: Vec<SelectItem>,
    selected_index: usize,
    max_visible: usize,
    theme: SelectListTheme,
    layout: SelectListLayoutOptions,
    on_select: Option<Box<dyn FnMut(&SelectItem) + Send>>,
    on_cancel: Option<Box<dyn FnMut() + Send>>,
    on_selection_change: Option<Box<dyn FnMut(&SelectItem) + Send>>,
}

impl SelectList {
    pub fn new(
        items: Vec<SelectItem>,
        max_visible: usize,
        theme: SelectListTheme,
        layout: SelectListLayoutOptions,
    ) -> Self {
        Self {
            filtered_items: items.clone(),
            items,
            selected_index: 0,
            max_visible,
            theme,
            layout,
            on_select: None,
            on_cancel: None,
            on_selection_change: None,
        }
    }

    pub fn set_filter(&mut self, filter: &str) {
        let filter = filter.to_lowercase();
        self.filtered_items = self
            .items
            .iter()
            .filter(|item| item.value.to_lowercase().starts_with(&filter))
            .cloned()
            .collect();
        self.selected_index = 0;
    }

    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = if self.filtered_items.is_empty() {
            0
        } else {
            index.min(self.filtered_items.len() - 1)
        };
    }

    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.filtered_items.get(self.selected_index)
    }

    pub fn set_on_select<F>(&mut self, on_select: F)
    where
        F: FnMut(&SelectItem) + Send + 'static,
    {
        self.on_select = Some(Box::new(on_select));
    }

    pub fn set_on_cancel<F>(&mut self, on_cancel: F)
    where
        F: FnMut() + Send + 'static,
    {
        self.on_cancel = Some(Box::new(on_cancel));
    }

    pub fn set_on_selection_change<F>(&mut self, on_selection_change: F)
    where
        F: FnMut(&SelectItem) + Send + 'static,
    {
        self.on_selection_change = Some(Box::new(on_selection_change));
    }

    pub fn render_ratatui(&mut self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let items = self.ratatui_items(area.width as usize);
        Widget::render(List::new(items), area, buffer);
    }

    pub fn ratatui_list_state(&self) -> ListState {
        let selected = (!self.filtered_items.is_empty()).then_some(self.selected_index);
        ListState::default().with_selected(selected)
    }

    pub fn render_ratatui_stateful(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        state: &mut ListState,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let items = self.ratatui_items(area.width as usize);
        StatefulWidget::render(List::new(items), area, buffer, state);
    }

    fn ratatui_items(&mut self, width: usize) -> Vec<ListItem<'static>> {
        Component::render(self, width)
            .into_iter()
            .map(ListItem::new)
            .collect()
    }

    pub fn handle_input(&mut self, key_data: &str, keybindings: &KeybindingsManager) {
        if self.filtered_items.is_empty() {
            if keybindings.matches(key_data, "tui.select.cancel") {
                if let Some(on_cancel) = self.on_cancel.as_mut() {
                    on_cancel();
                }
            }
            return;
        }

        if keybindings.matches(key_data, "tui.select.up") {
            self.selected_index = if self.selected_index == 0 {
                self.filtered_items.len() - 1
            } else {
                self.selected_index - 1
            };
            self.notify_selection_change();
        } else if keybindings.matches(key_data, "tui.select.down") {
            self.selected_index = if self.selected_index == self.filtered_items.len() - 1 {
                0
            } else {
                self.selected_index + 1
            };
            self.notify_selection_change();
        } else if keybindings.matches(key_data, "tui.select.confirm") {
            if let (Some(item), Some(on_select)) = (
                self.filtered_items.get(self.selected_index),
                self.on_select.as_mut(),
            ) {
                on_select(item);
            }
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            if let Some(on_cancel) = self.on_cancel.as_mut() {
                on_cancel();
            }
        }
    }

    fn render_item(
        &self,
        item: &SelectItem,
        is_selected: bool,
        width: usize,
        description_single_line: Option<&str>,
        primary_column_width: usize,
    ) -> String {
        let prefix = if is_selected { "→ " } else { "  " };
        let prefix_width = visible_width(prefix);

        if let Some(description) = description_single_line {
            if width > 40 {
                let effective_primary_column_width = primary_column_width
                    .min(width.saturating_sub(prefix_width + 4))
                    .max(1);
                let max_primary_width = effective_primary_column_width
                    .saturating_sub(PRIMARY_COLUMN_GAP)
                    .max(1);
                let truncated_value = self.truncate_primary(
                    item,
                    is_selected,
                    max_primary_width,
                    effective_primary_column_width,
                );
                let truncated_value_width = visible_width(&truncated_value);
                let spacing = " ".repeat(
                    effective_primary_column_width
                        .saturating_sub(truncated_value_width)
                        .max(1),
                );
                let description_start = prefix_width + truncated_value_width + spacing.len();
                let remaining_width = width.saturating_sub(description_start + 2);
                if remaining_width > MIN_DESCRIPTION_WIDTH {
                    let truncated_desc = truncate_to_width(description, remaining_width, "", false);
                    if is_selected {
                        return (self.theme.selected_text)(&format!(
                            "{prefix}{truncated_value}{spacing}{truncated_desc}"
                        ));
                    }
                    return format!(
                        "{prefix}{truncated_value}{}",
                        (self.theme.description)(&format!("{spacing}{truncated_desc}"))
                    );
                }
            }
        }

        let max_width = width.saturating_sub(prefix_width + 2).max(1);
        let truncated_value = self.truncate_primary(item, is_selected, max_width, max_width);
        if is_selected {
            (self.theme.selected_text)(&format!("{prefix}{truncated_value}"))
        } else {
            format!("{prefix}{truncated_value}")
        }
    }

    fn primary_column_width(&self) -> usize {
        let (min, max) = self.primary_column_bounds();
        let widest_primary = self
            .filtered_items
            .iter()
            .map(|item| visible_width(&self.display_value(item)) + PRIMARY_COLUMN_GAP)
            .max()
            .unwrap_or(0);
        widest_primary.clamp(min, max)
    }

    fn primary_column_bounds(&self) -> (usize, usize) {
        let raw_min = self
            .layout
            .min_primary_column_width
            .or(self.layout.max_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        let raw_max = self
            .layout
            .max_primary_column_width
            .or(self.layout.min_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        (raw_min.min(raw_max).max(1), raw_min.max(raw_max).max(1))
    }

    fn truncate_primary(
        &self,
        item: &SelectItem,
        is_selected: bool,
        max_width: usize,
        column_width: usize,
    ) -> String {
        let display_value = self.display_value(item);
        let truncated_value = if let Some(truncate_primary) = &self.layout.truncate_primary {
            truncate_primary(SelectListTruncatePrimaryContext {
                text: display_value,
                max_width,
                column_width,
                item: item.clone(),
                is_selected,
            })
        } else {
            truncate_to_width(&display_value, max_width, "", false)
        };
        truncate_to_width(&truncated_value, max_width, "", false)
    }

    fn display_value(&self, item: &SelectItem) -> String {
        if item.label.is_empty() {
            item.value.clone()
        } else {
            item.label.clone()
        }
    }

    fn notify_selection_change(&mut self) {
        if let (Some(item), Some(on_selection_change)) = (
            self.filtered_items.get(self.selected_index),
            self.on_selection_change.as_mut(),
        ) {
            on_selection_change(item);
        }
    }
}

impl Component for SelectList {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.filtered_items.is_empty() {
            return vec![(self.theme.no_match)("  No matching commands")];
        }

        let primary_column_width = self.primary_column_width();
        let start_index = self
            .selected_index
            .saturating_sub(self.max_visible / 2)
            .min(self.filtered_items.len().saturating_sub(self.max_visible));
        let end_index = (start_index + self.max_visible).min(self.filtered_items.len());
        let mut lines = Vec::new();

        for index in start_index..end_index {
            if let Some(item) = self.filtered_items.get(index) {
                let description = item
                    .description
                    .as_ref()
                    .map(|description| normalize_to_single_line(description));
                lines.push(self.render_item(
                    item,
                    index == self.selected_index,
                    width,
                    description.as_deref(),
                    primary_column_width,
                ));
            }
        }

        if start_index > 0 || end_index < self.filtered_items.len() {
            let scroll_text = format!(
                "  ({}/{})",
                self.selected_index + 1,
                self.filtered_items.len()
            );
            lines.push((self.theme.scroll_info)(&truncate_to_width(
                &scroll_text,
                width.saturating_sub(2),
                "",
                false,
            )));
        }

        lines
    }
}

fn normalize_to_single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ").trim().to_string()
}

impl Widget for &mut SelectList {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        Widget::render(
            List::new(self.ratatui_items(area.width as usize)),
            area,
            buffer,
        );
    }
}

impl StatefulWidget for &mut SelectList {
    type State = ListState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        StatefulWidget::render(
            List::new(self.ratatui_items(area.width as usize)),
            area,
            buffer,
            state,
        );
    }
}
