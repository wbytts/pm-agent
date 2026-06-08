use std::fs;
use std::path::{Path, PathBuf};

use crate::fuzzy::fuzzy_filter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteSuggestions {
    pub items: Vec<AutocompleteItem>,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_col: usize,
}

#[derive(Debug, Clone)]
pub struct CombinedAutocompleteProvider {
    commands: Vec<SlashCommand>,
    base_path: PathBuf,
}

impl CombinedAutocompleteProvider {
    pub fn new(commands: Vec<SlashCommand>, base_path: impl Into<PathBuf>) -> Self {
        Self {
            commands,
            base_path: base_path.into(),
        }
    }

    pub fn suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        force: bool,
    ) -> Option<AutocompleteSuggestions> {
        let current_line = lines.get(cursor_line).map(String::as_str).unwrap_or("");
        let text_before_cursor = safe_prefix(current_line, cursor_col);

        if let Some(at_prefix) = extract_at_prefix(text_before_cursor) {
            let suggestions = self.file_suggestions(&at_prefix);
            return (!suggestions.is_empty()).then_some(AutocompleteSuggestions {
                items: suggestions,
                prefix: at_prefix,
            });
        }

        if !force && text_before_cursor.starts_with('/') {
            if !text_before_cursor.contains(' ') {
                let prefix = &text_before_cursor[1..];
                let items = self
                    .commands
                    .iter()
                    .map(|command| AutocompleteItem {
                        value: command.name.clone(),
                        label: command.name.clone(),
                        description: command
                            .argument_hint
                            .as_ref()
                            .map(|hint| {
                                command
                                    .description
                                    .as_ref()
                                    .map(|description| format!("{hint} - {description}"))
                                    .unwrap_or_else(|| hint.clone())
                            })
                            .or_else(|| command.description.clone()),
                    })
                    .collect::<Vec<_>>();
                let filtered = fuzzy_filter(&items, prefix, |item| item.label.clone());
                return (!filtered.is_empty()).then_some(AutocompleteSuggestions {
                    items: filtered,
                    prefix: text_before_cursor.to_string(),
                });
            }
            return None;
        }

        let path_prefix = extract_path_prefix(text_before_cursor, force)?;
        let suggestions = self.file_suggestions(&path_prefix);
        (!suggestions.is_empty()).then_some(AutocompleteSuggestions {
            items: suggestions,
            prefix: path_prefix,
        })
    }

    pub fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> CompletionResult {
        let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
        let before_prefix_len = cursor_col.saturating_sub(prefix.len());
        let before_prefix = safe_prefix(&current_line, before_prefix_len);
        let after_cursor = safe_suffix(&current_line, cursor_col);
        let is_quoted_prefix = prefix.starts_with('"') || prefix.starts_with("@\"");
        let adjusted_after_cursor =
            if is_quoted_prefix && item.value.ends_with('"') && after_cursor.starts_with('"') {
                &after_cursor[1..]
            } else {
                after_cursor
            };

        let is_slash_command = prefix.starts_with('/')
            && before_prefix.trim().is_empty()
            && !prefix[1..].contains('/');
        let is_directory = item.label.ends_with('/');
        let mut new_lines = lines.to_vec();

        if is_slash_command {
            let new_line = format!("{before_prefix}/{} {adjusted_after_cursor}", item.value);
            new_lines[cursor_line] = new_line;
            return CompletionResult {
                lines: new_lines,
                cursor_line,
                cursor_col: before_prefix.len() + item.value.len() + 2,
            };
        }

        let suffix = if prefix.starts_with('@') && !is_directory {
            " "
        } else {
            ""
        };
        let new_line = format!(
            "{before_prefix}{}{suffix}{adjusted_after_cursor}",
            item.value
        );
        new_lines[cursor_line] = new_line;
        let cursor_offset = if is_directory && item.value.ends_with('"') {
            item.value.len().saturating_sub(1)
        } else {
            item.value.len()
        };
        CompletionResult {
            lines: new_lines,
            cursor_line,
            cursor_col: before_prefix.len() + cursor_offset + suffix.len(),
        }
    }

    fn file_suggestions(&self, prefix: &str) -> Vec<AutocompleteItem> {
        let parsed = parse_path_prefix(prefix);
        let (search_dir, search_prefix, display_base) =
            resolve_search(&self.base_path, &parsed.raw_prefix);
        let Ok(entries) = fs::read_dir(&search_dir) else {
            return Vec::new();
        };
        let mut suggestions = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name
                    .to_lowercase()
                    .starts_with(&search_prefix.to_lowercase())
                {
                    return None;
                }
                let path = entry.path();
                let is_directory = path.is_dir();
                let mut display = format!("{display_base}{name}");
                if is_directory {
                    display.push('/');
                }
                Some(AutocompleteItem {
                    value: build_completion_value(
                        &display,
                        parsed.is_at_prefix,
                        parsed.is_quoted_prefix,
                    ),
                    label: display,
                    description: if is_directory {
                        Some("directory".to_string())
                    } else {
                        None
                    },
                })
            })
            .collect::<Vec<_>>();
        suggestions.sort_by(|a, b| a.label.cmp(&b.label));
        suggestions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPathPrefix {
    raw_prefix: String,
    is_at_prefix: bool,
    is_quoted_prefix: bool,
}

fn parse_path_prefix(prefix: &str) -> ParsedPathPrefix {
    if let Some(raw) = prefix.strip_prefix("@\"") {
        return ParsedPathPrefix {
            raw_prefix: raw.to_string(),
            is_at_prefix: true,
            is_quoted_prefix: true,
        };
    }
    if let Some(raw) = prefix.strip_prefix('"') {
        return ParsedPathPrefix {
            raw_prefix: raw.to_string(),
            is_at_prefix: false,
            is_quoted_prefix: true,
        };
    }
    if let Some(raw) = prefix.strip_prefix('@') {
        return ParsedPathPrefix {
            raw_prefix: raw.to_string(),
            is_at_prefix: true,
            is_quoted_prefix: false,
        };
    }
    ParsedPathPrefix {
        raw_prefix: prefix.to_string(),
        is_at_prefix: false,
        is_quoted_prefix: false,
    }
}

fn extract_at_prefix(text: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_prefix(text) {
        if quoted.starts_with("@\"") {
            return Some(quoted);
        }
    }
    let token_start = find_last_delimiter(text).map_or(0, |index| index + 1);
    text[token_start..]
        .starts_with('@')
        .then(|| text[token_start..].to_string())
}

fn extract_path_prefix(text: &str, force: bool) -> Option<String> {
    if let Some(quoted) = extract_quoted_prefix(text) {
        return Some(quoted);
    }
    let token_start = find_last_delimiter(text).map_or(0, |index| index + 1);
    let path_prefix = &text[token_start..];
    if force
        || path_prefix.contains('/')
        || path_prefix.starts_with('.')
        || path_prefix.starts_with("~/")
        || (path_prefix.is_empty() && text.ends_with(' '))
    {
        return Some(path_prefix.to_string());
    }
    None
}

fn extract_quoted_prefix(text: &str) -> Option<String> {
    let mut in_quotes = false;
    let mut quote_start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
            if in_quotes {
                quote_start = index;
            }
        }
    }
    if !in_quotes {
        return None;
    }
    if quote_start > 0 && text[..quote_start].ends_with('@') {
        let at_index = quote_start - 1;
        if is_token_start(text, at_index) {
            return Some(text[at_index..].to_string());
        }
    }
    is_token_start(text, quote_start).then(|| text[quote_start..].to_string())
}

fn is_token_start(text: &str, index: usize) -> bool {
    index == 0 || text[..index].chars().last().is_some_and(is_path_delimiter)
}

fn find_last_delimiter(text: &str) -> Option<usize> {
    text.char_indices()
        .rev()
        .find(|(_, ch)| is_path_delimiter(*ch))
        .map(|(index, _)| index)
}

fn is_path_delimiter(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '"' | '\'' | '=')
}

fn resolve_search(base_path: &Path, raw_prefix: &str) -> (PathBuf, String, String) {
    let expanded = expand_home(raw_prefix);
    let display_prefix = raw_prefix.replace('\\', "/");
    let is_root = raw_prefix.is_empty()
        || raw_prefix == "./"
        || raw_prefix == "../"
        || raw_prefix == "~"
        || raw_prefix == "~/"
        || raw_prefix == "/";
    if is_root || raw_prefix.ends_with('/') {
        let search_dir = if expanded.is_absolute() {
            expanded
        } else {
            base_path.join(&expanded)
        };
        return (search_dir, String::new(), display_prefix);
    }
    let dir = expanded.parent().map(Path::to_path_buf).unwrap_or_default();
    let file = expanded
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let search_dir = if expanded.is_absolute() {
        dir
    } else {
        base_path.join(&dir)
    };
    let display_base = display_prefix
        .rsplit_once('/')
        .map(|(base, _)| format!("{base}/"))
        .unwrap_or_default();
    (search_dir, file, display_base)
}

fn build_completion_value(path: &str, is_at_prefix: bool, is_quoted_prefix: bool) -> String {
    let needs_quotes = is_quoted_prefix || path.contains(' ');
    let prefix = if is_at_prefix { "@" } else { "" };
    if needs_quotes {
        format!("{prefix}\"{path}\"")
    } else {
        format!("{prefix}{path}")
    }
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn safe_prefix(value: &str, len: usize) -> &str {
    value.get(..len).unwrap_or(value)
}

fn safe_suffix(value: &str, index: usize) -> &str {
    value.get(index..).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn suggests_and_applies_slash_commands() {
        let provider = CombinedAutocompleteProvider::new(
            vec![SlashCommand {
                name: "help".to_string(),
                description: Some("Show help".to_string()),
                argument_hint: None,
            }],
            ".",
        );
        let lines = vec!["/he".to_string()];
        let suggestions = provider
            .suggestions(&lines, 0, 3, false)
            .expect("suggestions");
        assert_eq!(suggestions.items[0].value, "help");
        let applied =
            provider.apply_completion(&lines, 0, 3, &suggestions.items[0], &suggestions.prefix);
        assert_eq!(applied.lines[0], "/help ");
        assert_eq!(applied.cursor_col, 6);
    }

    #[test]
    fn suggests_file_paths_and_at_references() {
        let dir = temp_dir();
        fs::write(dir.join("note.txt"), "").expect("file");
        fs::create_dir(dir.join("src")).expect("dir");
        let provider = CombinedAutocompleteProvider::new(Vec::new(), &dir);

        let lines = vec!["@no".to_string()];
        let suggestions = provider
            .suggestions(&lines, 0, 3, false)
            .expect("suggestions");
        assert_eq!(suggestions.items[0].value, "@note.txt");

        let lines = vec!["./s".to_string()];
        let suggestions = provider
            .suggestions(&lines, 0, 3, false)
            .expect("suggestions");
        assert_eq!(suggestions.items[0].label, "./src/");
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-autocomplete-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
