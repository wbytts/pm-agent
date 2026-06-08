use super::{Component, SelectList, SelectListLayoutOptions, SelectListTheme};
use crate::{
    decode_printable_key, matches_key, truncate_to_width, visible_width, AutocompleteItem,
    AutocompleteSuggestions, CombinedAutocompleteProvider, KeybindingsManager, KillRing, UndoStack,
};
use crate::{is_punctuation_char, is_whitespace_char};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const PASTE_MARKER_PREFIX: &str = "[paste #";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub text: String,
    pub start_index: usize,
    pub end_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_col: usize,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutLine {
    text: String,
    has_cursor: bool,
    cursor_pos: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisualLine {
    logical_line: usize,
    start_col: usize,
    length: usize,
}

pub type EditorStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Clone)]
pub struct EditorTheme {
    pub border_color: EditorStyleFn,
    pub select_list: SelectListTheme,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self {
            border_color: Arc::new(str::to_string),
            select_list: SelectListTheme::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorOptions {
    pub padding_x: usize,
    pub autocomplete_max_visible: usize,
    pub terminal_rows: usize,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            padding_x: 0,
            autocomplete_max_visible: 5,
            terminal_rows: 24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastAction {
    Kill,
    Yank,
    TypeWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpMode {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutocompleteState {
    Regular,
    Force,
}

pub struct Editor {
    state: EditorState,
    focused: bool,
    theme: EditorTheme,
    padding_x: usize,
    terminal_rows: usize,
    last_width: usize,
    scroll_offset: usize,
    autocomplete_provider: Option<CombinedAutocompleteProvider>,
    autocomplete_list: Option<SelectList>,
    autocomplete_state: Option<AutocompleteState>,
    autocomplete_prefix: String,
    autocomplete_max_visible: usize,
    pastes: BTreeMap<usize, String>,
    paste_counter: usize,
    paste_buffer: String,
    is_in_paste: bool,
    history: Vec<String>,
    history_index: isize,
    kill_ring: KillRing,
    last_action: Option<LastAction>,
    jump_mode: Option<JumpMode>,
    preferred_visual_col: Option<usize>,
    snapped_from_cursor_col: Option<usize>,
    undo_stack: UndoStack<EditorState>,
    on_submit: Option<Box<dyn FnMut(&str) + Send>>,
    on_change: Option<Box<dyn FnMut(&str) + Send>>,
    disable_submit: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(EditorTheme::default(), EditorOptions::default())
    }
}

impl Editor {
    pub fn new(theme: EditorTheme, options: EditorOptions) -> Self {
        Self {
            state: EditorState::default(),
            focused: false,
            theme,
            padding_x: options.padding_x,
            terminal_rows: options.terminal_rows.max(1),
            last_width: 80,
            scroll_offset: 0,
            autocomplete_provider: None,
            autocomplete_list: None,
            autocomplete_state: None,
            autocomplete_prefix: String::new(),
            autocomplete_max_visible: options.autocomplete_max_visible.clamp(3, 20),
            pastes: BTreeMap::new(),
            paste_counter: 0,
            paste_buffer: String::new(),
            is_in_paste: false,
            history: Vec::new(),
            history_index: -1,
            kill_ring: KillRing::new(),
            last_action: None,
            jump_mode: None,
            preferred_visual_col: None,
            snapped_from_cursor_col: None,
            undo_stack: UndoStack::new(),
            on_submit: None,
            on_change: None,
            disable_submit: false,
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn set_terminal_rows(&mut self, rows: usize) {
        self.terminal_rows = rows.max(1);
    }

    pub fn padding_x(&self) -> usize {
        self.padding_x
    }

    pub fn set_padding_x(&mut self, padding_x: usize) {
        self.padding_x = padding_x;
    }

    pub fn autocomplete_max_visible(&self) -> usize {
        self.autocomplete_max_visible
    }

    pub fn set_autocomplete_max_visible(&mut self, max_visible: usize) {
        self.autocomplete_max_visible = max_visible.clamp(3, 20);
    }

    pub fn set_autocomplete_provider(&mut self, provider: CombinedAutocompleteProvider) {
        self.cancel_autocomplete();
        self.autocomplete_provider = Some(provider);
    }

    pub fn set_disable_submit(&mut self, disable_submit: bool) {
        self.disable_submit = disable_submit;
    }

    pub fn set_on_submit<F>(&mut self, on_submit: F)
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_submit = Some(Box::new(on_submit));
    }

    pub fn set_on_change<F>(&mut self, on_change: F)
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_change = Some(Box::new(on_change));
    }

    pub fn add_to_history(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() || self.history.first().is_some_and(|entry| entry == trimmed) {
            return;
        }
        self.history.insert(0, trimmed.to_string());
        self.history.truncate(100);
    }

    pub fn get_text(&self) -> String {
        self.state.lines.join("\n")
    }

    pub fn get_expanded_text(&self) -> String {
        self.expand_paste_markers(&self.get_text())
    }

    pub fn lines(&self) -> Vec<String> {
        self.state.lines.clone()
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.state.cursor_line, self.state.cursor_col)
    }

    pub fn set_text(&mut self, text: &str) {
        self.cancel_autocomplete();
        self.last_action = None;
        self.history_index = -1;
        let normalized = normalize_text(text);
        if self.get_text() != normalized {
            self.push_undo_snapshot();
        }
        self.set_text_internal(&normalized);
    }

    pub fn insert_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.cancel_autocomplete();
        self.push_undo_snapshot();
        self.last_action = None;
        self.history_index = -1;
        self.insert_text_at_cursor_internal(text);
    }

    pub fn is_showing_autocomplete(&self) -> bool {
        self.autocomplete_state.is_some()
    }

    pub fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        if self.handle_jump_mode(data, keybindings) {
            return;
        }
        if self.handle_bracketed_paste(data, keybindings) {
            return;
        }
        if keybindings.matches(data, "tui.input.copy") {
            return;
        }
        if keybindings.matches(data, "tui.editor.undo") {
            self.undo();
            return;
        }
        if self.handle_autocomplete_input(data, keybindings) {
            return;
        }
        if keybindings.matches(data, "tui.input.tab") && self.autocomplete_state.is_none() {
            self.handle_tab_completion();
            return;
        }

        if keybindings.matches(data, "tui.editor.deleteToLineEnd") {
            self.delete_to_end_of_line();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteToLineStart") {
            self.delete_to_start_of_line();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteWordBackward") {
            self.delete_word_backwards();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteWordForward") {
            self.delete_word_forward();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteCharBackward")
            || matches_key(data, "shift+backspace")
        {
            self.handle_backspace();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteCharForward")
            || matches_key(data, "shift+delete")
        {
            self.handle_forward_delete();
            return;
        }
        if keybindings.matches(data, "tui.editor.yank") {
            self.yank();
            return;
        }
        if keybindings.matches(data, "tui.editor.yankPop") {
            self.yank_pop();
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorLineStart") {
            self.move_to_line_start();
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorLineEnd") {
            self.move_to_line_end();
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorWordLeft") {
            self.move_word_backwards();
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorWordRight") {
            self.move_word_forwards();
            return;
        }
        if self.is_new_line_input(data, keybindings) {
            if self.should_submit_on_backslash_enter(data, keybindings) {
                self.handle_backspace();
                self.submit_value();
            } else {
                self.add_new_line();
            }
            return;
        }
        if keybindings.matches(data, "tui.input.submit") {
            if self.disable_submit {
                return;
            }
            let current_line = self.current_line().to_string();
            if self.state.cursor_col > 0
                && current_line.as_bytes().get(self.state.cursor_col - 1) == Some(&b'\\')
            {
                self.handle_backspace();
                self.add_new_line();
            } else {
                self.submit_value();
            }
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorUp") {
            if self.is_editor_empty() {
                self.navigate_history(-1);
            } else if self.history_index > -1 && self.is_on_first_visual_line() {
                self.navigate_history(-1);
            } else if self.is_on_first_visual_line() {
                self.move_to_line_start();
            } else {
                self.move_cursor(-1, 0);
            }
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorDown") {
            if self.history_index > -1 && self.is_on_last_visual_line() {
                self.navigate_history(1);
            } else if self.is_on_last_visual_line() {
                self.move_to_line_end();
            } else {
                self.move_cursor(1, 0);
            }
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorRight") {
            self.move_cursor(0, 1);
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorLeft") {
            self.move_cursor(0, -1);
            return;
        }
        if keybindings.matches(data, "tui.editor.pageUp") {
            self.page_scroll(-1);
            return;
        }
        if keybindings.matches(data, "tui.editor.pageDown") {
            self.page_scroll(1);
            return;
        }
        if keybindings.matches(data, "tui.editor.jumpForward") {
            self.jump_mode = Some(JumpMode::Forward);
            return;
        }
        if keybindings.matches(data, "tui.editor.jumpBackward") {
            self.jump_mode = Some(JumpMode::Backward);
            return;
        }
        if matches_key(data, "shift+space") {
            self.insert_character(" ", false);
            return;
        }
        if let Some(printable) = decode_printable_key(data) {
            self.insert_character(&printable, false);
            return;
        }
        if !contains_control_chars(data) {
            self.insert_character(data, false);
        }
    }

    fn handle_jump_mode(&mut self, data: &str, keybindings: &KeybindingsManager) -> bool {
        let Some(direction) = self.jump_mode else {
            return false;
        };
        if keybindings.matches(data, "tui.editor.jumpForward")
            || keybindings.matches(data, "tui.editor.jumpBackward")
        {
            self.jump_mode = None;
            return true;
        }
        if let Some(printable) = decode_printable_key(data)
            .or_else(|| (!contains_control_chars(data)).then(|| data.to_string()))
        {
            self.jump_mode = None;
            self.jump_to_char(&printable, direction);
            return true;
        }
        self.jump_mode = None;
        false
    }

    fn handle_bracketed_paste(&mut self, data: &str, keybindings: &KeybindingsManager) -> bool {
        let mut data = data.to_string();
        if data.contains("\x1b[200~") {
            self.is_in_paste = true;
            self.paste_buffer.clear();
            data = data.replacen("\x1b[200~", "", 1);
        }
        if !self.is_in_paste {
            return false;
        }
        self.paste_buffer.push_str(&data);
        if let Some(end_index) = self.paste_buffer.find("\x1b[201~") {
            let paste_content = self.paste_buffer[..end_index].to_string();
            let remaining = self.paste_buffer[end_index + "\x1b[201~".len()..].to_string();
            if !paste_content.is_empty() {
                self.handle_paste(&paste_content);
            }
            self.is_in_paste = false;
            self.paste_buffer.clear();
            if !remaining.is_empty() {
                self.handle_input(&remaining, keybindings);
            }
        }
        true
    }

    fn handle_autocomplete_input(&mut self, data: &str, keybindings: &KeybindingsManager) -> bool {
        if self.autocomplete_state.is_none() || self.autocomplete_list.is_none() {
            return false;
        }
        if keybindings.matches(data, "tui.select.cancel") {
            self.cancel_autocomplete();
            return true;
        }
        if keybindings.matches(data, "tui.select.up")
            || keybindings.matches(data, "tui.select.down")
        {
            if let Some(list) = self.autocomplete_list.as_mut() {
                list.handle_input(data, keybindings);
            }
            return true;
        }
        if keybindings.matches(data, "tui.input.tab")
            || keybindings.matches(data, "tui.select.confirm")
        {
            self.apply_selected_autocomplete(keybindings.matches(data, "tui.select.confirm"));
            return true;
        }
        false
    }

    fn apply_selected_autocomplete(&mut self, submit_slash: bool) {
        let Some(provider) = self.autocomplete_provider.clone() else {
            return;
        };
        let Some(item) = self
            .autocomplete_list
            .as_ref()
            .and_then(SelectList::selected_item)
            .map(|item| AutocompleteItem {
                value: item.value.clone(),
                label: item.label.clone(),
                description: item.description.clone(),
            })
        else {
            return;
        };
        self.push_undo_snapshot();
        self.last_action = None;
        let result = provider.apply_completion(
            &self.state.lines,
            self.state.cursor_line,
            self.state.cursor_col,
            &item,
            &self.autocomplete_prefix,
        );
        self.state.lines = result.lines;
        self.state.cursor_line = result.cursor_line;
        self.set_cursor_col(result.cursor_col);
        let should_submit = submit_slash && self.autocomplete_prefix.starts_with('/');
        self.cancel_autocomplete();
        if should_submit {
            self.submit_value();
        } else {
            self.notify_change();
        }
    }

    fn handle_tab_completion(&mut self) {
        let Some(provider) = self.autocomplete_provider.clone() else {
            return;
        };
        let current_line = self.current_line();
        let before_cursor = safe_prefix(current_line, self.state.cursor_col);
        let force = !(self.is_in_slash_command_context(before_cursor)
            && !before_cursor.trim_start().contains(' '));
        let Some(suggestions) = provider.suggestions(
            &self.state.lines,
            self.state.cursor_line,
            self.state.cursor_col,
            force,
        ) else {
            return;
        };
        if force && suggestions.items.len() == 1 {
            let item = suggestions.items[0].clone();
            self.push_undo_snapshot();
            let result = provider.apply_completion(
                &self.state.lines,
                self.state.cursor_line,
                self.state.cursor_col,
                &item,
                &suggestions.prefix,
            );
            self.state.lines = result.lines;
            self.state.cursor_line = result.cursor_line;
            self.set_cursor_col(result.cursor_col);
            self.notify_change();
        } else {
            self.apply_autocomplete_suggestions(
                suggestions,
                if force {
                    AutocompleteState::Force
                } else {
                    AutocompleteState::Regular
                },
            );
        }
    }

    fn try_trigger_autocomplete(&mut self, force: bool) {
        let Some(provider) = &self.autocomplete_provider else {
            return;
        };
        if let Some(suggestions) = provider.suggestions(
            &self.state.lines,
            self.state.cursor_line,
            self.state.cursor_col,
            force,
        ) {
            self.apply_autocomplete_suggestions(
                suggestions,
                if force {
                    AutocompleteState::Force
                } else {
                    AutocompleteState::Regular
                },
            );
        } else {
            self.cancel_autocomplete();
        }
    }

    fn apply_autocomplete_suggestions(
        &mut self,
        suggestions: AutocompleteSuggestions,
        state: AutocompleteState,
    ) {
        self.autocomplete_prefix = suggestions.prefix.clone();
        let items = suggestions
            .items
            .iter()
            .map(|item| super::SelectItem {
                value: item.value.clone(),
                label: item.label.clone(),
                description: item.description.clone(),
            })
            .collect::<Vec<_>>();
        let layout = if suggestions.prefix.starts_with('/') {
            SelectListLayoutOptions {
                min_primary_column_width: Some(12),
                max_primary_column_width: Some(32),
                truncate_primary: None,
            }
        } else {
            SelectListLayoutOptions::default()
        };
        let mut list = SelectList::new(
            items,
            self.autocomplete_max_visible,
            self.theme.select_list.clone(),
            layout,
        );
        if let Some(index) = best_autocomplete_match_index(&suggestions.items, &suggestions.prefix)
        {
            list.set_selected_index(index);
        }
        self.autocomplete_list = Some(list);
        self.autocomplete_state = Some(state);
    }

    fn cancel_autocomplete(&mut self) {
        self.autocomplete_state = None;
        self.autocomplete_list = None;
        self.autocomplete_prefix.clear();
    }

    fn is_editor_empty(&self) -> bool {
        self.state.lines.len() == 1 && self.state.lines[0].is_empty()
    }

    fn is_on_first_visual_line(&self) -> bool {
        let visual_lines = self.build_visual_line_map(self.last_width);
        self.find_current_visual_line(&visual_lines) == 0
    }

    fn is_on_last_visual_line(&self) -> bool {
        let visual_lines = self.build_visual_line_map(self.last_width);
        self.find_current_visual_line(&visual_lines) + 1 == visual_lines.len()
    }

    fn navigate_history(&mut self, direction: isize) {
        self.last_action = None;
        if self.history.is_empty() {
            return;
        }
        let new_index = self.history_index - direction;
        if new_index < -1 || new_index >= self.history.len() as isize {
            return;
        }
        if self.history_index == -1 && new_index >= 0 {
            self.push_undo_snapshot();
        }
        self.history_index = new_index;
        if self.history_index == -1 {
            self.set_text_internal("");
        } else if let Some(text) = self.history.get(self.history_index as usize).cloned() {
            self.set_text_internal(&text);
        }
    }

    fn set_text_internal(&mut self, text: &str) {
        let mut lines = text.split('\n').map(str::to_string).collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.state.lines = lines;
        self.state.cursor_line = self.state.lines.len().saturating_sub(1);
        self.set_cursor_col(self.current_line().len());
        self.scroll_offset = 0;
        self.notify_change();
    }

    fn insert_text_at_cursor_internal(&mut self, text: &str) {
        let normalized = normalize_text(text);
        let inserted_lines = normalized.split('\n').collect::<Vec<_>>();
        let current_line = self.current_line().to_string();
        let before_cursor = safe_prefix(&current_line, self.state.cursor_col).to_string();
        let after_cursor = safe_suffix(&current_line, self.state.cursor_col).to_string();

        if inserted_lines.len() == 1 {
            self.state.lines[self.state.cursor_line] =
                format!("{before_cursor}{normalized}{after_cursor}");
            self.set_cursor_col(self.state.cursor_col + normalized.len());
        } else {
            let mut replacement = Vec::new();
            replacement.extend(self.state.lines[..self.state.cursor_line].iter().cloned());
            replacement.push(format!("{}{}", before_cursor, inserted_lines[0]));
            replacement.extend(
                inserted_lines[1..inserted_lines.len() - 1]
                    .iter()
                    .map(|line| (*line).to_string()),
            );
            let last_inserted = inserted_lines.last().copied().unwrap_or_default();
            replacement.push(format!("{last_inserted}{after_cursor}"));
            replacement.extend(
                self.state.lines[self.state.cursor_line + 1..]
                    .iter()
                    .cloned(),
            );
            self.state.lines = replacement;
            self.state.cursor_line += inserted_lines.len() - 1;
            self.set_cursor_col(last_inserted.len());
        }
        self.notify_change();
    }

    fn insert_character(&mut self, text: &str, skip_undo_coalescing: bool) {
        self.history_index = -1;
        if !skip_undo_coalescing {
            if text.chars().any(is_whitespace_char)
                || self.last_action != Some(LastAction::TypeWord)
            {
                self.push_undo_snapshot();
            }
            self.last_action = Some(LastAction::TypeWord);
        }
        let line = self.current_line().to_string();
        let before = safe_prefix(&line, self.state.cursor_col);
        let after = safe_suffix(&line, self.state.cursor_col);
        self.state.lines[self.state.cursor_line] = format!("{before}{text}{after}");
        self.set_cursor_col(self.state.cursor_col + text.len());
        self.notify_change();

        if self.autocomplete_state.is_none() {
            if text == "/" && self.is_at_start_of_message() {
                self.try_trigger_autocomplete(false);
            } else if text == "@" || text == "#" {
                let current_line = self.current_line();
                let before_cursor = safe_prefix(current_line, self.state.cursor_col);
                let char_before_symbol = before_cursor[..before_cursor.len().saturating_sub(1)]
                    .chars()
                    .next_back();
                if before_cursor.len() == 1 || matches!(char_before_symbol, Some(' ' | '\t')) {
                    self.try_trigger_autocomplete(false);
                }
            } else if text
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
            {
                let current_line = self.current_line();
                let before_cursor = safe_prefix(current_line, self.state.cursor_col);
                if self.is_in_slash_command_context(before_cursor)
                    || has_symbol_completion_context(before_cursor)
                {
                    self.try_trigger_autocomplete(false);
                }
            }
        } else {
            self.try_trigger_autocomplete(
                self.autocomplete_state == Some(AutocompleteState::Force),
            );
        }
    }

    fn handle_paste(&mut self, pasted_text: &str) {
        self.cancel_autocomplete();
        self.history_index = -1;
        self.last_action = None;
        self.push_undo_snapshot();
        let decoded = decode_csi_u_ctrl_bytes(pasted_text);
        let mut filtered = normalize_text(&decoded)
            .chars()
            .filter(|ch| *ch == '\n' || *ch as u32 >= 32)
            .collect::<String>();
        if filtered.starts_with(['/', '~', '.']) {
            let current_line = self.current_line();
            let char_before_cursor = safe_prefix(current_line, self.state.cursor_col)
                .chars()
                .next_back();
            if char_before_cursor.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                filtered.insert(0, ' ');
            }
        }
        let pasted_lines = filtered.split('\n').count();
        let total_chars = filtered.len();
        if pasted_lines > 10 || total_chars > 1000 {
            self.paste_counter += 1;
            let paste_id = self.paste_counter;
            self.pastes.insert(paste_id, filtered);
            let marker = if pasted_lines > 10 {
                format!("[paste #{paste_id} +{pasted_lines} lines]")
            } else {
                format!("[paste #{paste_id} {total_chars} chars]")
            };
            self.insert_text_at_cursor_internal(&marker);
        } else {
            self.insert_text_at_cursor_internal(&filtered);
        }
    }

    fn add_new_line(&mut self) {
        self.cancel_autocomplete();
        self.history_index = -1;
        self.last_action = None;
        self.push_undo_snapshot();
        let current_line = self.current_line().to_string();
        let before = safe_prefix(&current_line, self.state.cursor_col).to_string();
        let after = safe_suffix(&current_line, self.state.cursor_col).to_string();
        self.state.lines[self.state.cursor_line] = before;
        self.state.lines.insert(self.state.cursor_line + 1, after);
        self.state.cursor_line += 1;
        self.set_cursor_col(0);
        self.notify_change();
    }

    fn should_submit_on_backslash_enter(
        &self,
        data: &str,
        keybindings: &KeybindingsManager,
    ) -> bool {
        if self.disable_submit || !matches_key(data, "enter") {
            return false;
        }
        let submit_keys = keybindings.keys("tui.input.submit");
        let has_shift_enter = submit_keys
            .iter()
            .any(|key| matches!(key.as_str(), "shift+enter" | "shift+return"));
        has_shift_enter
            && self.state.cursor_col > 0
            && self
                .current_line()
                .as_bytes()
                .get(self.state.cursor_col - 1)
                == Some(&b'\\')
    }

    fn submit_value(&mut self) {
        self.cancel_autocomplete();
        let result = self
            .expand_paste_markers(&self.get_text())
            .trim()
            .to_string();
        self.state = EditorState::default();
        self.pastes.clear();
        self.paste_counter = 0;
        self.history_index = -1;
        self.scroll_offset = 0;
        self.undo_stack.clear();
        self.last_action = None;
        self.notify_change();
        if let Some(on_submit) = self.on_submit.as_mut() {
            on_submit(&result);
        }
    }

    fn handle_backspace(&mut self) {
        self.history_index = -1;
        self.last_action = None;
        if self.state.cursor_col > 0 {
            self.push_undo_snapshot();
            let line = self.current_line().to_string();
            let delete_from =
                prev_segment_boundary(&line, self.state.cursor_col, &self.valid_paste_ids());
            self.state.lines[self.state.cursor_line]
                .replace_range(delete_from..self.state.cursor_col, "");
            self.set_cursor_col(delete_from);
        } else if self.state.cursor_line > 0 {
            self.push_undo_snapshot();
            let current = self.state.lines.remove(self.state.cursor_line);
            self.state.cursor_line -= 1;
            let previous_len = self.state.lines[self.state.cursor_line].len();
            self.state.lines[self.state.cursor_line].push_str(&current);
            self.set_cursor_col(previous_len);
        }
        self.notify_change();
        self.update_autocomplete_after_edit();
    }

    fn handle_forward_delete(&mut self) {
        self.history_index = -1;
        self.last_action = None;
        let current_line = self.current_line().to_string();
        if self.state.cursor_col < current_line.len() {
            self.push_undo_snapshot();
            let delete_to = next_segment_boundary(
                &current_line,
                self.state.cursor_col,
                &self.valid_paste_ids(),
            );
            self.state.lines[self.state.cursor_line]
                .replace_range(self.state.cursor_col..delete_to, "");
        } else if self.state.cursor_line + 1 < self.state.lines.len() {
            self.push_undo_snapshot();
            let next = self.state.lines.remove(self.state.cursor_line + 1);
            self.state.lines[self.state.cursor_line].push_str(&next);
        }
        self.notify_change();
        self.update_autocomplete_after_edit();
    }

    fn delete_to_start_of_line(&mut self) {
        self.history_index = -1;
        let current_line = self.current_line().to_string();
        if self.state.cursor_col > 0 {
            self.push_undo_snapshot();
            let deleted = current_line[..self.state.cursor_col].to_string();
            self.kill_ring
                .push(deleted, true, self.last_action == Some(LastAction::Kill));
            self.last_action = Some(LastAction::Kill);
            self.state.lines[self.state.cursor_line] =
                current_line[self.state.cursor_col..].to_string();
            self.set_cursor_col(0);
        } else if self.state.cursor_line > 0 {
            self.push_undo_snapshot();
            self.kill_ring
                .push("\n", true, self.last_action == Some(LastAction::Kill));
            self.last_action = Some(LastAction::Kill);
            let current = self.state.lines.remove(self.state.cursor_line);
            self.state.cursor_line -= 1;
            let previous_len = self.state.lines[self.state.cursor_line].len();
            self.state.lines[self.state.cursor_line].push_str(&current);
            self.set_cursor_col(previous_len);
        }
        self.notify_change();
    }

    fn delete_to_end_of_line(&mut self) {
        self.history_index = -1;
        let current_line = self.current_line().to_string();
        if self.state.cursor_col < current_line.len() {
            self.push_undo_snapshot();
            let deleted = current_line[self.state.cursor_col..].to_string();
            self.kill_ring
                .push(deleted, false, self.last_action == Some(LastAction::Kill));
            self.last_action = Some(LastAction::Kill);
            self.state.lines[self.state.cursor_line].truncate(self.state.cursor_col);
        } else if self.state.cursor_line + 1 < self.state.lines.len() {
            self.push_undo_snapshot();
            self.kill_ring
                .push("\n", false, self.last_action == Some(LastAction::Kill));
            self.last_action = Some(LastAction::Kill);
            let next = self.state.lines.remove(self.state.cursor_line + 1);
            self.state.lines[self.state.cursor_line].push_str(&next);
        }
        self.notify_change();
    }

    fn delete_word_backwards(&mut self) {
        self.history_index = -1;
        if self.state.cursor_col == 0 {
            if self.state.cursor_line > 0 {
                self.push_undo_snapshot();
                self.kill_ring
                    .push("\n", true, self.last_action == Some(LastAction::Kill));
                self.last_action = Some(LastAction::Kill);
                let current = self.state.lines.remove(self.state.cursor_line);
                self.state.cursor_line -= 1;
                let previous_len = self.state.lines[self.state.cursor_line].len();
                self.state.lines[self.state.cursor_line].push_str(&current);
                self.set_cursor_col(previous_len);
                self.notify_change();
            }
            return;
        }
        self.push_undo_snapshot();
        let was_kill = self.last_action == Some(LastAction::Kill);
        let old_col = self.state.cursor_col;
        self.move_word_backwards();
        let delete_from = self.state.cursor_col;
        self.set_cursor_col(old_col);
        let current_line = self.current_line().to_string();
        let deleted = current_line[delete_from..old_col].to_string();
        self.kill_ring.push(deleted, true, was_kill);
        self.last_action = Some(LastAction::Kill);
        self.state.lines[self.state.cursor_line] = format!(
            "{}{}",
            &current_line[..delete_from],
            &current_line[old_col..]
        );
        self.set_cursor_col(delete_from);
        self.notify_change();
    }

    fn delete_word_forward(&mut self) {
        self.history_index = -1;
        let current_line = self.current_line().to_string();
        if self.state.cursor_col >= current_line.len() {
            if self.state.cursor_line + 1 < self.state.lines.len() {
                self.push_undo_snapshot();
                self.kill_ring
                    .push("\n", false, self.last_action == Some(LastAction::Kill));
                self.last_action = Some(LastAction::Kill);
                let next = self.state.lines.remove(self.state.cursor_line + 1);
                self.state.lines[self.state.cursor_line].push_str(&next);
                self.notify_change();
            }
            return;
        }
        self.push_undo_snapshot();
        let was_kill = self.last_action == Some(LastAction::Kill);
        let old_col = self.state.cursor_col;
        self.move_word_forwards();
        let delete_to = self.state.cursor_col;
        self.set_cursor_col(old_col);
        let current_line = self.current_line().to_string();
        let deleted = current_line[old_col..delete_to].to_string();
        self.kill_ring.push(deleted, false, was_kill);
        self.last_action = Some(LastAction::Kill);
        self.state.lines[self.state.cursor_line] =
            format!("{}{}", &current_line[..old_col], &current_line[delete_to..]);
        self.notify_change();
    }

    fn yank(&mut self) {
        let Some(text) = self.kill_ring.peek().map(str::to_string) else {
            return;
        };
        self.push_undo_snapshot();
        self.insert_yanked_text(&text);
        self.last_action = Some(LastAction::Yank);
    }

    fn yank_pop(&mut self) {
        if self.last_action != Some(LastAction::Yank) || self.kill_ring.len() <= 1 {
            return;
        }
        self.push_undo_snapshot();
        self.delete_yanked_text();
        self.kill_ring.rotate();
        if let Some(text) = self.kill_ring.peek().map(str::to_string) {
            self.insert_yanked_text(&text);
        }
        self.last_action = Some(LastAction::Yank);
    }

    fn insert_yanked_text(&mut self, text: &str) {
        self.history_index = -1;
        self.insert_text_at_cursor_internal(text);
    }

    fn delete_yanked_text(&mut self) {
        let Some(yanked_text) = self.kill_ring.peek().map(str::to_string) else {
            return;
        };
        let yank_lines = yanked_text.split('\n').collect::<Vec<_>>();
        if yank_lines.len() == 1 {
            let line = self.current_line().to_string();
            let delete_len = yanked_text.len();
            let delete_from = self.state.cursor_col.saturating_sub(delete_len);
            self.state.lines[self.state.cursor_line] =
                format!("{}{}", &line[..delete_from], &line[self.state.cursor_col..]);
            self.set_cursor_col(delete_from);
        } else {
            let start_line = self.state.cursor_line.saturating_sub(yank_lines.len() - 1);
            let first_len = yank_lines.first().map(|line| line.len()).unwrap_or(0);
            let start_col = self.state.lines[start_line].len().saturating_sub(first_len);
            let after_cursor = safe_suffix(self.current_line(), self.state.cursor_col).to_string();
            let before_yank = safe_prefix(&self.state.lines[start_line], start_col).to_string();
            self.state.lines.splice(
                start_line..=self.state.cursor_line,
                [format!("{before_yank}{after_cursor}")],
            );
            self.state.cursor_line = start_line;
            self.set_cursor_col(start_col);
        }
        self.notify_change();
    }

    fn undo(&mut self) {
        self.history_index = -1;
        if let Some(snapshot) = self.undo_stack.pop() {
            self.state = snapshot;
            self.last_action = None;
            self.preferred_visual_col = None;
            self.snapped_from_cursor_col = None;
            self.notify_change();
        }
    }

    fn jump_to_char(&mut self, text: &str, direction: JumpMode) {
        self.last_action = None;
        let Some(target) = text.chars().next() else {
            return;
        };
        match direction {
            JumpMode::Forward => {
                for line_idx in self.state.cursor_line..self.state.lines.len() {
                    let line = &self.state.lines[line_idx];
                    let search_from = if line_idx == self.state.cursor_line {
                        self.state.cursor_col + 1
                    } else {
                        0
                    };
                    if let Some(offset) = line.get(search_from..).and_then(|tail| tail.find(target))
                    {
                        self.state.cursor_line = line_idx;
                        self.set_cursor_col(search_from + offset);
                        return;
                    }
                }
            }
            JumpMode::Backward => {
                for line_idx in (0..=self.state.cursor_line).rev() {
                    let line = &self.state.lines[line_idx];
                    let search_until = if line_idx == self.state.cursor_line {
                        self.state.cursor_col
                    } else {
                        line.len()
                    };
                    if let Some(pos) = line[..search_until].rfind(target) {
                        self.state.cursor_line = line_idx;
                        self.set_cursor_col(pos);
                        return;
                    }
                }
            }
        }
    }

    fn move_to_line_start(&mut self) {
        self.last_action = None;
        self.set_cursor_col(0);
    }

    fn move_to_line_end(&mut self) {
        self.last_action = None;
        self.set_cursor_col(self.current_line().len());
    }

    fn move_cursor(&mut self, delta_line: isize, delta_col: isize) {
        self.last_action = None;
        let visual_lines = self.build_visual_line_map(self.last_width);
        let current_visual_line = self.find_current_visual_line(&visual_lines);
        if delta_line != 0 {
            let target = current_visual_line as isize + delta_line;
            if target >= 0 && (target as usize) < visual_lines.len() {
                self.move_to_visual_line(&visual_lines, current_visual_line, target as usize);
            }
        }
        if delta_col > 0 {
            let current_line = self.current_line().to_string();
            if self.state.cursor_col < current_line.len() {
                self.set_cursor_col(next_segment_boundary(
                    &current_line,
                    self.state.cursor_col,
                    &self.valid_paste_ids(),
                ));
            } else if self.state.cursor_line + 1 < self.state.lines.len() {
                self.state.cursor_line += 1;
                self.set_cursor_col(0);
            }
        } else if delta_col < 0 {
            let current_line = self.current_line().to_string();
            if self.state.cursor_col > 0 {
                self.set_cursor_col(prev_segment_boundary(
                    &current_line,
                    self.state.cursor_col,
                    &self.valid_paste_ids(),
                ));
            } else if self.state.cursor_line > 0 {
                self.state.cursor_line -= 1;
                self.set_cursor_col(self.current_line().len());
            }
        }
    }

    fn page_scroll(&mut self, direction: isize) {
        self.last_action = None;
        let page_size = self.max_visible_lines();
        let visual_lines = self.build_visual_line_map(self.last_width);
        let current = self.find_current_visual_line(&visual_lines);
        let target = (current as isize + direction * page_size as isize)
            .clamp(0, visual_lines.len().saturating_sub(1) as isize) as usize;
        self.move_to_visual_line(&visual_lines, current, target);
    }

    fn move_word_backwards(&mut self) {
        self.last_action = None;
        if self.state.cursor_col == 0 {
            if self.state.cursor_line > 0 {
                self.state.cursor_line -= 1;
                self.set_cursor_col(self.current_line().len());
            }
            return;
        }
        let line = self.current_line().to_string();
        let segments = segments_with_paste_markers(&line, &self.valid_paste_ids());
        let mut new_col = self.state.cursor_col;
        let mut index = segments.iter().rposition(|segment| segment.end <= new_col);
        while let Some(i) = index {
            let segment = &segments[i];
            if !segment.is_paste_marker && segment.text.chars().all(is_whitespace_char) {
                new_col = segment.start;
                index = i.checked_sub(1);
            } else {
                break;
            }
        }
        if let Some(i) = index {
            let punctuation_run =
                !segments[i].is_paste_marker && segments[i].text.chars().all(is_punctuation_char);
            if segments[i].is_paste_marker {
                new_col = segments[i].start;
            } else {
                let mut j = Some(i);
                while let Some(idx) = j {
                    let segment = &segments[idx];
                    if segment.is_paste_marker || segment.text.chars().all(is_whitespace_char) {
                        break;
                    }
                    if segment.text.chars().all(is_punctuation_char) != punctuation_run {
                        break;
                    }
                    new_col = segment.start;
                    j = idx.checked_sub(1);
                }
            }
        }
        self.set_cursor_col(new_col);
    }

    fn move_word_forwards(&mut self) {
        self.last_action = None;
        let line = self.current_line().to_string();
        if self.state.cursor_col >= line.len() {
            if self.state.cursor_line + 1 < self.state.lines.len() {
                self.state.cursor_line += 1;
                self.set_cursor_col(0);
            }
            return;
        }
        let segments = segments_with_paste_markers(&line, &self.valid_paste_ids());
        let mut index = segments
            .iter()
            .position(|segment| segment.end > self.state.cursor_col)
            .unwrap_or(segments.len());
        let mut new_col = self.state.cursor_col;
        while index < segments.len() {
            let segment = &segments[index];
            if !segment.is_paste_marker && segment.text.chars().all(is_whitespace_char) {
                new_col = segment.end;
                index += 1;
            } else {
                break;
            }
        }
        if index < segments.len() {
            let segment = &segments[index];
            if segment.is_paste_marker {
                new_col = segment.end;
            } else {
                let punctuation_run = segment.text.chars().all(is_punctuation_char);
                while index < segments.len() {
                    let segment = &segments[index];
                    if segment.is_paste_marker || segment.text.chars().all(is_whitespace_char) {
                        break;
                    }
                    if segment.text.chars().all(is_punctuation_char) != punctuation_run {
                        break;
                    }
                    new_col = segment.end;
                    index += 1;
                }
            }
        }
        self.set_cursor_col(new_col);
    }

    fn build_visual_line_map(&self, width: usize) -> Vec<VisualLine> {
        let mut visual_lines = Vec::new();
        for (line_idx, line) in self.state.lines.iter().enumerate() {
            if line.is_empty() {
                visual_lines.push(VisualLine {
                    logical_line: line_idx,
                    start_col: 0,
                    length: 0,
                });
            } else if visible_width(line) <= width {
                visual_lines.push(VisualLine {
                    logical_line: line_idx,
                    start_col: 0,
                    length: line.len(),
                });
            } else {
                for chunk in word_wrap_line(line, width, &self.valid_paste_ids()) {
                    visual_lines.push(VisualLine {
                        logical_line: line_idx,
                        start_col: chunk.start_index,
                        length: chunk.end_index - chunk.start_index,
                    });
                }
            }
        }
        visual_lines
    }

    fn find_visual_line_at(&self, visual_lines: &[VisualLine], line: usize, col: usize) -> usize {
        for (index, visual_line) in visual_lines.iter().enumerate() {
            if visual_line.logical_line != line {
                continue;
            }
            let offset = col.saturating_sub(visual_line.start_col);
            let is_last_segment = index + 1 == visual_lines.len()
                || visual_lines[index + 1].logical_line != visual_line.logical_line;
            if col >= visual_line.start_col
                && (offset < visual_line.length
                    || (is_last_segment && offset == visual_line.length))
            {
                return index;
            }
        }
        visual_lines.len().saturating_sub(1)
    }

    fn find_current_visual_line(&self, visual_lines: &[VisualLine]) -> usize {
        self.find_visual_line_at(visual_lines, self.state.cursor_line, self.state.cursor_col)
    }

    fn move_to_visual_line(
        &mut self,
        visual_lines: &[VisualLine],
        current_visual_line: usize,
        target_visual_line: usize,
    ) {
        let Some(current) = visual_lines.get(current_visual_line) else {
            return;
        };
        let Some(target) = visual_lines.get(target_visual_line) else {
            return;
        };
        let current_visual_col = self
            .snapped_from_cursor_col
            .map(|col| col.saturating_sub(current.start_col))
            .unwrap_or_else(|| self.state.cursor_col.saturating_sub(current.start_col));
        let source_last = current_visual_line + 1 == visual_lines.len()
            || visual_lines[current_visual_line + 1].logical_line != current.logical_line;
        let target_last = target_visual_line + 1 == visual_lines.len()
            || visual_lines[target_visual_line + 1].logical_line != target.logical_line;
        let source_max = if source_last {
            current.length
        } else {
            current.length.saturating_sub(1)
        };
        let target_max = if target_last {
            target.length
        } else {
            target.length.saturating_sub(1)
        };
        let visual_col =
            self.compute_vertical_move_column(current_visual_col, source_max, target_max);
        self.state.cursor_line = target.logical_line;
        self.state.cursor_col = (target.start_col + visual_col).min(self.current_line().len());
        self.snap_cursor_to_paste_marker(
            target,
            target_visual_line > current_visual_line,
            visual_lines,
            current_visual_line,
        );
    }

    fn compute_vertical_move_column(
        &mut self,
        current_visual_col: usize,
        source_max: usize,
        target_max: usize,
    ) -> usize {
        let has_preferred = self.preferred_visual_col.is_some();
        let cursor_in_middle = current_visual_col < source_max;
        let target_too_short = target_max < current_visual_col;
        if !has_preferred || cursor_in_middle {
            if target_too_short {
                self.preferred_visual_col = Some(current_visual_col);
                return target_max;
            }
            self.preferred_visual_col = None;
            return current_visual_col;
        }
        let preferred = self.preferred_visual_col.unwrap_or(current_visual_col);
        if target_too_short || target_max < preferred {
            return target_max;
        }
        self.preferred_visual_col = None;
        preferred
    }

    fn snap_cursor_to_paste_marker(
        &mut self,
        target: &VisualLine,
        moving_down: bool,
        visual_lines: &[VisualLine],
        current_visual_line: usize,
    ) {
        let line = self.current_line().to_string();
        for segment in segments_with_paste_markers(&line, &self.valid_paste_ids()) {
            if !segment.is_paste_marker
                || segment.start > self.state.cursor_col
                || self.state.cursor_col >= segment.end
            {
                continue;
            }
            if segment.start < target.start_col && moving_down {
                let next = visual_lines
                    .iter()
                    .enumerate()
                    .skip(current_visual_line + 1)
                    .find(|(_, visual_line)| {
                        visual_line.logical_line != target.logical_line
                            || visual_line.start_col >= segment.end
                    })
                    .map(|(idx, _)| idx);
                if let Some(next) = next {
                    self.move_to_visual_line(visual_lines, current_visual_line, next);
                    return;
                }
            }
            self.snapped_from_cursor_col = Some(self.state.cursor_col);
            self.state.cursor_col = segment.start;
            return;
        }
        self.snapped_from_cursor_col = None;
    }

    fn layout_text(&self, content_width: usize) -> Vec<LayoutLine> {
        if self.is_editor_empty() {
            return vec![LayoutLine {
                text: String::new(),
                has_cursor: true,
                cursor_pos: Some(0),
            }];
        }
        let mut layout_lines = Vec::new();
        for (idx, line) in self.state.lines.iter().enumerate() {
            let current = idx == self.state.cursor_line;
            if visible_width(line) <= content_width {
                layout_lines.push(LayoutLine {
                    text: line.clone(),
                    has_cursor: current,
                    cursor_pos: current.then_some(self.state.cursor_col),
                });
            } else {
                for (chunk_idx, chunk) in
                    word_wrap_line(line, content_width, &self.valid_paste_ids())
                        .into_iter()
                        .enumerate()
                {
                    let last_chunk = chunk_idx + 1
                        == word_wrap_line(line, content_width, &self.valid_paste_ids()).len();
                    let has_cursor = current
                        && if last_chunk {
                            self.state.cursor_col >= chunk.start_index
                        } else {
                            self.state.cursor_col >= chunk.start_index
                                && self.state.cursor_col < chunk.end_index
                        };
                    let cursor_pos = has_cursor.then_some(
                        (self.state.cursor_col - chunk.start_index).min(chunk.text.len()),
                    );
                    layout_lines.push(LayoutLine {
                        text: chunk.text,
                        has_cursor,
                        cursor_pos,
                    });
                }
            }
        }
        layout_lines
    }

    fn expand_paste_markers(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (paste_id, content) in &self.pastes {
            for marker in paste_markers_for_id(&result, *paste_id) {
                result = result.replace(&marker, content);
            }
        }
        result
    }

    fn current_line(&self) -> &str {
        self.state
            .lines
            .get(self.state.cursor_line)
            .map(String::as_str)
            .unwrap_or("")
    }

    fn set_cursor_col(&mut self, col: usize) {
        self.state.cursor_col =
            clamp_to_char_boundary(self.current_line(), col.min(self.current_line().len()));
        self.preferred_visual_col = None;
        self.snapped_from_cursor_col = None;
    }

    fn valid_paste_ids(&self) -> BTreeSet<usize> {
        self.pastes.keys().copied().collect()
    }

    fn max_visible_lines(&self) -> usize {
        (self.terminal_rows * 3 / 10).max(5)
    }

    fn is_slash_menu_allowed(&self) -> bool {
        self.state.cursor_line == 0
    }

    fn is_at_start_of_message(&self) -> bool {
        if !self.is_slash_menu_allowed() {
            return false;
        }
        let before = safe_prefix(self.current_line(), self.state.cursor_col);
        before.trim().is_empty() || before.trim() == "/"
    }

    fn is_in_slash_command_context(&self, text_before_cursor: &str) -> bool {
        self.is_slash_menu_allowed() && text_before_cursor.trim_start().starts_with('/')
    }

    fn update_autocomplete_after_edit(&mut self) {
        if let Some(state) = self.autocomplete_state {
            self.try_trigger_autocomplete(state == AutocompleteState::Force);
        } else {
            let before = safe_prefix(self.current_line(), self.state.cursor_col);
            if self.is_in_slash_command_context(before) || has_symbol_completion_context(before) {
                self.try_trigger_autocomplete(false);
            }
        }
    }

    fn notify_change(&mut self) {
        let text = self.get_text();
        if let Some(on_change) = self.on_change.as_mut() {
            on_change(&text);
        }
    }

    fn push_undo_snapshot(&mut self) {
        self.undo_stack.push(&self.state);
    }

    fn is_new_line_input(&self, data: &str, keybindings: &KeybindingsManager) -> bool {
        keybindings.matches(data, "tui.input.newLine")
            || (data.as_bytes().first() == Some(&10) && data.len() > 1)
            || data == "\x1b\r"
            || data == "\x1b[13;2~"
            || (data.len() > 1 && data.contains('\x1b') && data.contains('\r'))
            || data == "\n"
    }
}

impl Component for Editor {
    fn render(&mut self, width: usize) -> Vec<String> {
        let max_padding = width.saturating_sub(1) / 2;
        let padding_x = self.padding_x.min(max_padding);
        let content_width = width.saturating_sub(padding_x * 2).max(1);
        let layout_width = if padding_x > 0 {
            content_width
        } else {
            content_width.saturating_sub(1).max(1)
        };
        self.last_width = layout_width;

        let horizontal = (self.theme.border_color)("─");
        let layout_lines = self.layout_text(layout_width);
        let max_visible = self.max_visible_lines();
        let cursor_line_index = layout_lines
            .iter()
            .position(|line| line.has_cursor)
            .unwrap_or(0);
        if cursor_line_index < self.scroll_offset {
            self.scroll_offset = cursor_line_index;
        } else if cursor_line_index >= self.scroll_offset + max_visible {
            self.scroll_offset = cursor_line_index - max_visible + 1;
        }
        self.scroll_offset = self
            .scroll_offset
            .min(layout_lines.len().saturating_sub(max_visible));
        let visible_lines = layout_lines
            .iter()
            .skip(self.scroll_offset)
            .take(max_visible)
            .cloned()
            .collect::<Vec<_>>();

        let mut result = Vec::new();
        if self.scroll_offset > 0 {
            let indicator = format!("─── ↑ {} more ", self.scroll_offset);
            result.push((self.theme.border_color)(&truncate_to_width(
                &indicator, width, "", false,
            )));
        } else {
            result.push(horizontal.repeat(width));
        }

        let left_padding = " ".repeat(padding_x);
        let right_padding = left_padding.clone();
        let emit_cursor_marker = self.focused && self.autocomplete_state.is_none();
        for layout_line in visible_lines {
            let mut display = layout_line.text;
            let mut line_width = visible_width(&display);
            let mut cursor_in_padding = false;
            if layout_line.has_cursor {
                let cursor_pos = clamp_to_char_boundary(
                    &display,
                    layout_line.cursor_pos.unwrap_or(0).min(display.len()),
                );
                let marker = if emit_cursor_marker {
                    super::CURSOR_MARKER
                } else {
                    ""
                };
                if cursor_pos < display.len() {
                    let end = next_char_boundary(&display, cursor_pos);
                    let before = &display[..cursor_pos];
                    let at = &display[cursor_pos..end];
                    let after = &display[end..];
                    display = format!("{before}{marker}\x1b[7m{at}\x1b[0m{after}");
                } else {
                    display = format!("{}{}\x1b[7m \x1b[0m", &display[..cursor_pos], marker);
                    line_width += 1;
                    cursor_in_padding = line_width > content_width && padding_x > 0;
                }
            }
            let padding = " ".repeat(content_width.saturating_sub(line_width));
            let line_right_padding = if cursor_in_padding {
                right_padding.get(1..).unwrap_or("").to_string()
            } else {
                right_padding.clone()
            };
            result.push(format!(
                "{left_padding}{display}{padding}{line_right_padding}"
            ));
        }

        let lines_below = layout_lines
            .len()
            .saturating_sub(self.scroll_offset + result.len().saturating_sub(1));
        if lines_below > 0 {
            let indicator = format!("─── ↓ {lines_below} more ");
            let remaining = width.saturating_sub(visible_width(&indicator));
            result.push((self.theme.border_color)(&format!(
                "{indicator}{}",
                "─".repeat(remaining)
            )));
        } else {
            result.push(horizontal.repeat(width));
        }

        if self.autocomplete_state.is_some() {
            if let Some(list) = self.autocomplete_list.as_mut() {
                for line in list.render(content_width) {
                    let line_padding =
                        " ".repeat(content_width.saturating_sub(visible_width(&line)));
                    result.push(format!("{left_padding}{line}{line_padding}{right_padding}"));
                }
            }
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    text: String,
    start: usize,
    end: usize,
    is_paste_marker: bool,
}

pub fn word_wrap_line(
    line: &str,
    max_width: usize,
    valid_paste_ids: &BTreeSet<usize>,
) -> Vec<TextChunk> {
    if line.is_empty() || max_width == 0 {
        return vec![TextChunk {
            text: String::new(),
            start_index: 0,
            end_index: 0,
        }];
    }
    if visible_width(line) <= max_width {
        return vec![TextChunk {
            text: line.to_string(),
            start_index: 0,
            end_index: line.len(),
        }];
    }
    let segments = segments_with_paste_markers(line, valid_paste_ids);
    let mut chunks = Vec::new();
    let mut current_width = 0;
    let mut chunk_start = 0;
    let mut wrap_opp_index: Option<usize> = None;
    let mut wrap_opp_width = 0;

    for (idx, segment) in segments.iter().enumerate() {
        let width = visible_width(&segment.text);
        let is_ws = !segment.is_paste_marker && segment.text.chars().all(is_whitespace_char);
        if current_width + width > max_width {
            if let Some(wrap_index) = wrap_opp_index {
                if current_width.saturating_sub(wrap_opp_width) + width <= max_width {
                    chunks.push(TextChunk {
                        text: line[chunk_start..wrap_index].to_string(),
                        start_index: chunk_start,
                        end_index: wrap_index,
                    });
                    chunk_start = wrap_index;
                    current_width = current_width.saturating_sub(wrap_opp_width);
                } else if chunk_start < segment.start {
                    chunks.push(TextChunk {
                        text: line[chunk_start..segment.start].to_string(),
                        start_index: chunk_start,
                        end_index: segment.start,
                    });
                    chunk_start = segment.start;
                    current_width = 0;
                }
            } else if chunk_start < segment.start {
                chunks.push(TextChunk {
                    text: line[chunk_start..segment.start].to_string(),
                    start_index: chunk_start,
                    end_index: segment.start,
                });
                chunk_start = segment.start;
                current_width = 0;
            }
            wrap_opp_index = None;
        }
        if width > max_width {
            for chunk in word_wrap_line(&segment.text, max_width, &BTreeSet::new()) {
                chunks.push(TextChunk {
                    text: chunk.text,
                    start_index: segment.start + chunk.start_index,
                    end_index: segment.start + chunk.end_index,
                });
            }
            chunk_start = segment.end;
            current_width = 0;
            wrap_opp_index = None;
            continue;
        }
        current_width += width;
        let next = segments.get(idx + 1);
        if is_ws
            && next.is_some_and(|next| {
                next.is_paste_marker || !next.text.chars().all(is_whitespace_char)
            })
        {
            wrap_opp_index = Some(segment.end);
            wrap_opp_width = current_width;
        }
    }
    chunks.push(TextChunk {
        text: line[chunk_start..].to_string(),
        start_index: chunk_start,
        end_index: line.len(),
    });
    chunks
}

fn segments_with_paste_markers(line: &str, valid_paste_ids: &BTreeSet<usize>) -> Vec<Segment> {
    let markers = paste_marker_spans(line, valid_paste_ids);
    let mut segments = Vec::new();
    let mut index = 0;
    while index < line.len() {
        if let Some((start, end)) = markers.iter().find(|(start, _)| *start == index) {
            segments.push(Segment {
                text: line[*start..*end].to_string(),
                start: *start,
                end: *end,
                is_paste_marker: true,
            });
            index = *end;
            continue;
        }
        let end = next_char_boundary(line, index);
        segments.push(Segment {
            text: line[index..end].to_string(),
            start: index,
            end,
            is_paste_marker: false,
        });
        index = end;
    }
    segments
}

fn paste_marker_spans(line: &str, valid_paste_ids: &BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut search_start = 0;
    while let Some(relative) = line[search_start..].find(PASTE_MARKER_PREFIX) {
        let start = search_start + relative;
        let Some(end_relative) = line[start..].find(']') else {
            break;
        };
        let end = start + end_relative + 1;
        let marker = &line[start..end];
        if parse_paste_marker_id(marker).is_some_and(|id| valid_paste_ids.contains(&id)) {
            spans.push((start, end));
        }
        search_start = end;
    }
    spans
}

fn paste_markers_for_id(text: &str, paste_id: usize) -> Vec<String> {
    let mut markers = Vec::new();
    let mut search_start = 0;
    while let Some(relative) = text[search_start..].find(PASTE_MARKER_PREFIX) {
        let start = search_start + relative;
        let Some(end_relative) = text[start..].find(']') else {
            break;
        };
        let end = start + end_relative + 1;
        let marker = &text[start..end];
        if parse_paste_marker_id(marker) == Some(paste_id) {
            markers.push(marker.to_string());
        }
        search_start = end;
    }
    markers
}

fn parse_paste_marker_id(marker: &str) -> Option<usize> {
    marker
        .strip_prefix(PASTE_MARKER_PREFIX)?
        .split([' ', ']'])
        .next()?
        .parse()
        .ok()
}

fn best_autocomplete_match_index(items: &[AutocompleteItem], prefix: &str) -> Option<usize> {
    if prefix.is_empty() {
        return None;
    }
    let mut first_prefix = None;
    for (idx, item) in items.iter().enumerate() {
        if item.value == prefix {
            return Some(idx);
        }
        if first_prefix.is_none() && item.value.starts_with(prefix) {
            first_prefix = Some(idx);
        }
    }
    first_prefix
}

fn decode_csi_u_ctrl_bytes(data: &str) -> String {
    let mut output = String::new();
    let mut rest = data;
    while let Some(start) = rest.find("\x1b[") {
        output.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('u') else {
            output.push_str(tail);
            return output;
        };
        let seq = &tail[..=end];
        if let Some(body) = seq
            .strip_prefix("\x1b[")
            .and_then(|seq| seq.strip_suffix('u'))
        {
            if let Some((code, "5")) = body.split_once(';') {
                if let Ok(cp) = code.parse::<u8>() {
                    if cp.is_ascii_lowercase() {
                        output.push((cp - 96) as char);
                        rest = &tail[end + 1..];
                        continue;
                    }
                    if cp.is_ascii_uppercase() {
                        output.push((cp - 64) as char);
                        rest = &tail[end + 1..];
                        continue;
                    }
                }
            }
        }
        output.push_str(seq);
        rest = &tail[end + 1..];
    }
    output.push_str(rest);
    output
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
}

fn contains_control_chars(data: &str) -> bool {
    data.chars()
        .any(|ch| matches!(ch as u32, 0..=31 | 0x7f | 0x80..=0x9f))
}

fn safe_prefix(value: &str, index: usize) -> &str {
    &value[..clamp_to_char_boundary(value, index.min(value.len()))]
}

fn safe_suffix(value: &str, index: usize) -> &str {
    &value[clamp_to_char_boundary(value, index.min(value.len()))..]
}

fn clamp_to_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    if value.is_char_boundary(index) {
        index
    } else {
        prev_char_boundary(value, index)
    }
}

fn prev_char_boundary(value: &str, index: usize) -> usize {
    value[..index]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    let index = clamp_to_char_boundary(value, index);
    value[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(value.len())
}

fn prev_segment_boundary(value: &str, index: usize, valid_paste_ids: &BTreeSet<usize>) -> usize {
    for segment in segments_with_paste_markers(value, valid_paste_ids)
        .iter()
        .rev()
    {
        if segment.end <= index {
            return segment.start;
        }
    }
    0
}

fn next_segment_boundary(value: &str, index: usize, valid_paste_ids: &BTreeSet<usize>) -> usize {
    for segment in segments_with_paste_markers(value, valid_paste_ids) {
        if segment.end > index {
            return segment.end;
        }
    }
    value.len()
}

fn has_symbol_completion_context(value: &str) -> bool {
    let token = value.split_whitespace().last().unwrap_or("");
    token.starts_with('@') || token.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SlashCommand;
    use std::sync::{Arc, Mutex};

    fn kb() -> KeybindingsManager {
        KeybindingsManager::default()
    }

    #[test]
    fn editor_edits_multiline_submits_and_renders_cursor() {
        let submitted = Arc::new(Mutex::new(String::new()));
        let changes = Arc::new(Mutex::new(Vec::<String>::new()));
        let submitted_for_callback = submitted.clone();
        let changes_for_callback = changes.clone();
        let mut editor = Editor::default();
        editor.set_focused(true);
        editor.set_on_submit(move |value| {
            *submitted_for_callback.lock().expect("lock submitted") = value.to_string();
        });
        editor.set_on_change(move |value| {
            changes_for_callback
                .lock()
                .expect("lock changes")
                .push(value.to_string());
        });

        editor.handle_input("hello", &kb());
        editor.handle_input("\n", &kb());
        editor.handle_input("world", &kb());
        assert_eq!(editor.get_text(), "hello\nworld");
        assert_eq!(editor.cursor(), (1, 5));
        let lines = editor.render(20);
        assert!(lines
            .iter()
            .any(|line| line.contains(super::super::CURSOR_MARKER)));
        editor.handle_input("\r", &kb());

        assert_eq!(*submitted.lock().expect("lock submitted"), "hello\nworld");
        assert_eq!(editor.get_text(), "");
        assert!(changes
            .lock()
            .expect("lock changes")
            .iter()
            .any(|value| value == "hello\nworld"));
    }

    #[test]
    fn editor_supports_history_navigation_and_undo() {
        let mut editor = Editor::default();
        editor.add_to_history("first");
        editor.add_to_history("second");

        editor.handle_input("\x1b[A", &kb());
        assert_eq!(editor.get_text(), "second");
        editor.handle_input("\x1b[A", &kb());
        assert_eq!(editor.get_text(), "first");
        editor.handle_input("\x1b[B", &kb());
        assert_eq!(editor.get_text(), "second");
        editor.handle_input("!", &kb());
        assert_eq!(editor.get_text(), "second!");
        editor.handle_input("\x1f", &kb());
        assert_eq!(editor.get_text(), "second");
    }

    #[test]
    fn editor_handles_large_paste_markers_and_expands_on_submit() {
        let submitted = Arc::new(Mutex::new(String::new()));
        let submitted_for_callback = submitted.clone();
        let mut editor = Editor::default();
        editor.set_on_submit(move |value| {
            *submitted_for_callback.lock().expect("lock submitted") = value.to_string();
        });
        let pasted = (0..12)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        editor.handle_input(&format!("\x1b[200~{pasted}\x1b[201~"), &kb());

        assert!(editor.get_text().starts_with("[paste #1 +12 lines]"));
        assert_eq!(editor.get_expanded_text(), pasted);
        editor.handle_input("\r", &kb());
        assert_eq!(*submitted.lock().expect("lock submitted"), pasted);
    }

    #[test]
    fn editor_kill_yank_word_delete_and_jump_work() {
        let mut editor = Editor::default();
        editor.handle_input("hello, world", &kb());
        editor.handle_input("\x1bb", &kb());
        assert_eq!(editor.cursor(), (0, 7));
        editor.handle_input("\x1b\x7f", &kb());
        assert_eq!(editor.get_text(), "helloworld");
        editor.handle_input("\x15", &kb());
        assert_eq!(editor.get_text(), "world");
        editor.handle_input("\x19", &kb());
        assert_eq!(editor.get_text(), "hello, world");
        editor.handle_input("\x1d", &kb());
        editor.handle_input("o", &kb());
        assert_eq!(editor.cursor(), (0, 8));
    }

    #[test]
    fn editor_wraps_text_and_completes_with_provider() {
        let chunks = word_wrap_line("alpha beta gamma", 8, &BTreeSet::new());
        assert_eq!(
            chunks,
            vec![
                TextChunk {
                    text: "alpha ".to_string(),
                    start_index: 0,
                    end_index: 6
                },
                TextChunk {
                    text: "beta ".to_string(),
                    start_index: 6,
                    end_index: 11
                },
                TextChunk {
                    text: "gamma".to_string(),
                    start_index: 11,
                    end_index: 16
                }
            ]
        );

        let mut editor = Editor::default();
        editor.set_autocomplete_provider(CombinedAutocompleteProvider::new(
            vec![SlashCommand {
                name: "help".to_string(),
                description: Some("show help".to_string()),
                argument_hint: None,
            }],
            ".",
        ));
        editor.handle_input("/", &kb());
        editor.handle_input("h", &kb());
        assert!(editor.is_showing_autocomplete());
        editor.handle_input("\t", &kb());
        assert_eq!(editor.get_text(), "/help ");
    }
}
