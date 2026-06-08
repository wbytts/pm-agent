use super::Component;
use crate::{
    decode_printable_key, is_punctuation_char, is_whitespace_char, slice_by_column, visible_width,
    KeybindingsManager, KillRing, UndoStack,
};
use std::borrow::Cow;

pub const CURSOR_MARKER: &str = "\x1b_pi:c\x07";

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputState {
    value: String,
    cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastAction {
    Kill,
    Yank,
    TypeWord,
}

pub struct Input {
    value: String,
    cursor: usize,
    focused: bool,
    paste_buffer: String,
    is_in_paste: bool,
    kill_ring: KillRing,
    last_action: Option<LastAction>,
    undo_stack: UndoStack<InputState>,
    on_submit: Option<Box<dyn FnMut(&str) + Send>>,
    on_escape: Option<Box<dyn FnMut() + Send>>,
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Input {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            focused: false,
            paste_buffer: String::new(),
            is_in_paste: false,
            kill_ring: KillRing::new(),
            last_action: None,
            undo_stack: UndoStack::new(),
            on_submit: None,
            on_escape: None,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = clamp_to_char_boundary(&self.value, self.cursor.min(self.value.len()));
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = clamp_to_char_boundary(&self.value, cursor.min(self.value.len()));
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn set_on_submit<F>(&mut self, on_submit: F)
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_submit = Some(Box::new(on_submit));
    }

    pub fn set_on_escape<F>(&mut self, on_escape: F)
    where
        F: FnMut() + Send + 'static,
    {
        self.on_escape = Some(Box::new(on_escape));
    }

    pub fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        let mut data = Cow::Borrowed(data);
        if data.contains("\x1b[200~") {
            self.is_in_paste = true;
            self.paste_buffer.clear();
            data = Cow::Owned(data.replacen("\x1b[200~", "", 1));
        }
        let data = data.as_ref();

        if self.is_in_paste {
            self.paste_buffer.push_str(data);
            if let Some(end_index) = self.paste_buffer.find("\x1b[201~") {
                let paste_content = self.paste_buffer[..end_index].to_string();
                let remaining = self.paste_buffer[end_index + "\x1b[201~".len()..].to_string();
                self.handle_paste(&paste_content);
                self.is_in_paste = false;
                self.paste_buffer.clear();
                if !remaining.is_empty() {
                    self.handle_input(&remaining, keybindings);
                }
            }
            return;
        }

        if keybindings.matches(data, "tui.select.cancel") {
            if let Some(on_escape) = self.on_escape.as_mut() {
                on_escape();
            }
            return;
        }

        if keybindings.matches(data, "tui.editor.undo") {
            self.undo();
            return;
        }

        if keybindings.matches(data, "tui.input.submit") || data == "\n" {
            if let Some(on_submit) = self.on_submit.as_mut() {
                on_submit(&self.value);
            }
            return;
        }

        if keybindings.matches(data, "tui.editor.deleteCharBackward") {
            self.handle_backspace();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteCharForward") {
            self.handle_forward_delete();
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
        if keybindings.matches(data, "tui.editor.deleteToLineStart") {
            self.delete_to_line_start();
            return;
        }
        if keybindings.matches(data, "tui.editor.deleteToLineEnd") {
            self.delete_to_line_end();
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
        if keybindings.matches(data, "tui.editor.cursorLeft") {
            self.last_action = None;
            self.cursor = prev_char_boundary(&self.value, self.cursor);
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorRight") {
            self.last_action = None;
            self.cursor = next_char_boundary(&self.value, self.cursor);
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorLineStart") {
            self.last_action = None;
            self.cursor = 0;
            return;
        }
        if keybindings.matches(data, "tui.editor.cursorLineEnd") {
            self.last_action = None;
            self.cursor = self.value.len();
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

        if let Some(printable) = decode_printable_key(data) {
            self.insert_text(&printable);
            return;
        }

        if !contains_control_chars(data) {
            self.insert_text(data);
        }
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if text.chars().any(is_whitespace_char) || self.last_action != Some(LastAction::TypeWord) {
            self.push_undo();
        }
        self.last_action = Some(LastAction::TypeWord);
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn handle_backspace(&mut self) {
        self.last_action = None;
        if self.cursor == 0 {
            return;
        }
        self.push_undo();
        let delete_from = prev_char_boundary(&self.value, self.cursor);
        self.value.replace_range(delete_from..self.cursor, "");
        self.cursor = delete_from;
    }

    fn handle_forward_delete(&mut self) {
        self.last_action = None;
        if self.cursor >= self.value.len() {
            return;
        }
        self.push_undo();
        let delete_to = next_char_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..delete_to, "");
    }

    fn delete_to_line_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.push_undo();
        let deleted_text = self.value[..self.cursor].to_string();
        self.kill_ring.push(
            deleted_text,
            true,
            self.last_action == Some(LastAction::Kill),
        );
        self.last_action = Some(LastAction::Kill);
        self.value.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    fn delete_to_line_end(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        self.push_undo();
        let deleted_text = self.value[self.cursor..].to_string();
        self.kill_ring.push(
            deleted_text,
            false,
            self.last_action == Some(LastAction::Kill),
        );
        self.last_action = Some(LastAction::Kill);
        self.value.truncate(self.cursor);
    }

    fn delete_word_backwards(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let was_kill = self.last_action == Some(LastAction::Kill);
        self.push_undo();
        let old_cursor = self.cursor;
        self.move_word_backwards();
        let delete_from = self.cursor;
        self.cursor = old_cursor;
        let deleted_text = self.value[delete_from..self.cursor].to_string();
        self.kill_ring.push(deleted_text, true, was_kill);
        self.last_action = Some(LastAction::Kill);
        self.value.replace_range(delete_from..self.cursor, "");
        self.cursor = delete_from;
    }

    fn delete_word_forward(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let was_kill = self.last_action == Some(LastAction::Kill);
        self.push_undo();
        let old_cursor = self.cursor;
        self.move_word_forwards();
        let delete_to = self.cursor;
        self.cursor = old_cursor;
        let deleted_text = self.value[self.cursor..delete_to].to_string();
        self.kill_ring.push(deleted_text, false, was_kill);
        self.last_action = Some(LastAction::Kill);
        self.value.replace_range(self.cursor..delete_to, "");
    }

    fn yank(&mut self) {
        let Some(text) = self.kill_ring.peek().map(str::to_string) else {
            return;
        };
        self.push_undo();
        self.value.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.last_action = Some(LastAction::Yank);
    }

    fn yank_pop(&mut self) {
        if self.last_action != Some(LastAction::Yank) || self.kill_ring.len() <= 1 {
            return;
        }
        self.push_undo();
        let previous_len = self.kill_ring.peek().map(str::len).unwrap_or(0);
        let delete_from = self.cursor.saturating_sub(previous_len);
        self.value.replace_range(delete_from..self.cursor, "");
        self.cursor = delete_from;
        self.kill_ring.rotate();
        if let Some(text) = self.kill_ring.peek().map(str::to_string) {
            self.value.insert_str(self.cursor, &text);
            self.cursor += text.len();
        }
        self.last_action = Some(LastAction::Yank);
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(&InputState {
            value: self.value.clone(),
            cursor: self.cursor,
        });
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.value = snapshot.value;
            self.cursor = snapshot.cursor;
            self.last_action = None;
        }
    }

    fn move_word_backwards(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.last_action = None;
        while self.cursor > 0 {
            let previous = previous_char(&self.value, self.cursor);
            if previous.is_some_and(is_whitespace_char) {
                self.cursor = prev_char_boundary(&self.value, self.cursor);
            } else {
                break;
            }
        }
        if self.cursor == 0 {
            return;
        }
        let punctuation_run =
            previous_char(&self.value, self.cursor).is_some_and(is_punctuation_char);
        while self.cursor > 0 {
            let Some(previous) = previous_char(&self.value, self.cursor) else {
                break;
            };
            if is_whitespace_char(previous) {
                break;
            }
            if is_punctuation_char(previous) != punctuation_run {
                break;
            }
            self.cursor = prev_char_boundary(&self.value, self.cursor);
        }
    }

    fn move_word_forwards(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        self.last_action = None;
        while self.cursor < self.value.len() {
            let Some(current) = current_char(&self.value, self.cursor) else {
                break;
            };
            if is_whitespace_char(current) {
                self.cursor = next_char_boundary(&self.value, self.cursor);
            } else {
                break;
            }
        }
        if self.cursor >= self.value.len() {
            return;
        }
        let punctuation_run =
            current_char(&self.value, self.cursor).is_some_and(is_punctuation_char);
        while self.cursor < self.value.len() {
            let Some(current) = current_char(&self.value, self.cursor) else {
                break;
            };
            if is_whitespace_char(current) {
                break;
            }
            if is_punctuation_char(current) != punctuation_run {
                break;
            }
            self.cursor = next_char_boundary(&self.value, self.cursor);
        }
    }

    fn handle_paste(&mut self, pasted_text: &str) {
        self.last_action = None;
        self.push_undo();
        let clean_text = pasted_text
            .replace("\r\n", "")
            .replace(['\r', '\n'], "")
            .replace('\t', "    ");
        self.value.insert_str(self.cursor, &clean_text);
        self.cursor += clean_text.len();
    }
}

impl Component for Input {
    fn render(&mut self, width: usize) -> Vec<String> {
        let prompt = "> ";
        let available_width = width.saturating_sub(prompt.len());
        if available_width == 0 {
            return vec![prompt.to_string()];
        }

        let total_width = visible_width(&self.value);
        let (visible_text, cursor_display) = if total_width < available_width {
            (self.value.clone(), self.cursor)
        } else {
            let scroll_width = if self.cursor == self.value.len() {
                available_width.saturating_sub(1)
            } else {
                available_width
            };
            let cursor_col = visible_width(&self.value[..self.cursor]);
            if scroll_width == 0 {
                (String::new(), 0)
            } else {
                let half_width = scroll_width / 2;
                let start_col = if cursor_col < half_width {
                    0
                } else if cursor_col > total_width.saturating_sub(half_width) {
                    total_width.saturating_sub(scroll_width)
                } else {
                    cursor_col.saturating_sub(half_width)
                };
                let visible_text = slice_by_column(&self.value, start_col, scroll_width, true);
                let before_cursor = slice_by_column(
                    &self.value,
                    start_col,
                    cursor_col.saturating_sub(start_col),
                    true,
                );
                let cursor_display = before_cursor.len();
                (visible_text, cursor_display)
            }
        };

        let cursor_display =
            clamp_to_char_boundary(&visible_text, cursor_display.min(visible_text.len()));
        let at_cursor_end = next_char_boundary(&visible_text, cursor_display);
        let before_cursor = &visible_text[..cursor_display];
        let at_cursor = if cursor_display < visible_text.len() {
            &visible_text[cursor_display..at_cursor_end]
        } else {
            " "
        };
        let after_cursor = if cursor_display < visible_text.len() {
            &visible_text[at_cursor_end..]
        } else {
            ""
        };
        let marker = if self.focused { CURSOR_MARKER } else { "" };
        let text_with_cursor =
            format!("{before_cursor}{marker}\x1b[7m{at_cursor}\x1b[27m{after_cursor}");
        let padding = " ".repeat(available_width.saturating_sub(visible_width(&text_with_cursor)));
        vec![format!("{prompt}{text_with_cursor}{padding}")]
    }
}

fn contains_control_chars(data: &str) -> bool {
    data.chars()
        .any(|ch| matches!(ch as u32, 0..=31 | 0x7f | 0x80..=0x9f))
}

fn clamp_to_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    if value.is_char_boundary(index) {
        return index;
    }
    prev_char_boundary(value, index)
}

fn prev_char_boundary(value: &str, index: usize) -> usize {
    value[..index]
        .char_indices()
        .next_back()
        .map(|(pos, _)| pos)
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

fn previous_char(value: &str, index: usize) -> Option<char> {
    value[..index].chars().next_back()
}

fn current_char(value: &str, index: usize) -> Option<char> {
    value[index..].chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn keybindings() -> KeybindingsManager {
        KeybindingsManager::default()
    }

    #[test]
    fn input_inserts_submits_and_escapes() {
        let mut input = Input::new();
        let submitted = Arc::new(Mutex::new(String::new()));
        let escaped = Arc::new(Mutex::new(false));
        let submitted_for_callback = submitted.clone();
        let escaped_for_callback = escaped.clone();
        input.set_on_submit(move |value| {
            *submitted_for_callback.lock().expect("lock submitted") = value.to_string();
        });
        input.set_on_escape(move || {
            *escaped_for_callback.lock().expect("lock escaped") = true;
        });

        input.handle_input("abc", &keybindings());
        input.handle_input("\r", &keybindings());
        input.handle_input("\x1b", &keybindings());

        assert_eq!(input.value(), "abc");
        assert_eq!(*submitted.lock().expect("lock submitted"), "abc");
        assert!(*escaped.lock().expect("lock escaped"));
    }

    #[test]
    fn input_moves_and_deletes_words() {
        let mut input = Input::new();
        input.handle_input("hello, world", &keybindings());
        input.handle_input("\x1bb", &keybindings());
        assert_eq!(input.cursor(), 7);
        input.handle_input("\x1b\x7f", &keybindings());
        assert_eq!(input.value(), "helloworld");
        input.handle_input("\x15", &keybindings());
        assert_eq!(input.value(), "world");
        input.handle_input("\x1f", &keybindings());
        assert_eq!(input.value(), "helloworld");
    }

    #[test]
    fn input_handles_paste_and_kitty_printable() {
        let mut input = Input::new();
        input.handle_input("\x1b[200~a\r\nb\tc\x1b[201~", &keybindings());
        input.handle_input("\x1b[33u", &keybindings());
        assert_eq!(input.value(), "ab    c!");
    }

    #[test]
    fn input_renders_focus_marker_and_horizontal_scroll() {
        let mut input = Input::new();
        input.set_value("abcdef");
        input.set_cursor(6);
        input.set_focused(true);
        let lines = input.render(6);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(CURSOR_MARKER));
        assert!(lines[0].contains("\x1b[7m"));
        assert_eq!(visible_width(&lines[0]), 6);
    }
}
