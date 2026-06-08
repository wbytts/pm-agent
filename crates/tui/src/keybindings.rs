use std::collections::{BTreeMap, BTreeSet};
use std::sync::{OnceLock, RwLock};

use crate::keys::matches_key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingDefinition {
    pub default_keys: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingConflict {
    pub key: String,
    pub keybindings: Vec<String>,
}

pub type KeybindingsConfig = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct KeybindingsManager {
    definitions: BTreeMap<String, KeybindingDefinition>,
    user_bindings: KeybindingsConfig,
    keys_by_id: BTreeMap<String, Vec<String>>,
    conflicts: Vec<KeybindingConflict>,
}

impl KeybindingsManager {
    pub fn new(
        definitions: BTreeMap<String, KeybindingDefinition>,
        user_bindings: KeybindingsConfig,
    ) -> Self {
        let mut manager = Self {
            definitions,
            user_bindings,
            keys_by_id: BTreeMap::new(),
            conflicts: Vec::new(),
        };
        manager.rebuild();
        manager
    }

    pub fn with_defaults() -> Self {
        Self::new(default_keybindings(), BTreeMap::new())
    }

    pub fn matches(&self, data: &str, keybinding: &str) -> bool {
        self.keys_by_id
            .get(keybinding)
            .into_iter()
            .flat_map(|keys| keys.iter())
            .any(|key| matches_key(data, key))
    }

    pub fn keys(&self, keybinding: &str) -> Vec<String> {
        self.keys_by_id.get(keybinding).cloned().unwrap_or_default()
    }

    pub fn definition(&self, keybinding: &str) -> Option<&KeybindingDefinition> {
        self.definitions.get(keybinding)
    }

    pub fn conflicts(&self) -> Vec<KeybindingConflict> {
        self.conflicts.clone()
    }

    pub fn set_user_bindings(&mut self, user_bindings: KeybindingsConfig) {
        self.user_bindings = user_bindings;
        self.rebuild();
    }

    pub fn user_bindings(&self) -> KeybindingsConfig {
        self.user_bindings.clone()
    }

    pub fn resolved_bindings(&self) -> KeybindingsConfig {
        self.keys_by_id.clone()
    }

    fn rebuild(&mut self) {
        self.keys_by_id.clear();
        self.conflicts.clear();

        let mut claims: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (keybinding, keys) in &self.user_bindings {
            if !self.definitions.contains_key(keybinding) {
                continue;
            }
            for key in normalize_keys(keys) {
                claims.entry(key).or_default().insert(keybinding.clone());
            }
        }
        self.conflicts = claims
            .into_iter()
            .filter_map(|(key, keybindings)| {
                (keybindings.len() > 1).then(|| KeybindingConflict {
                    key,
                    keybindings: keybindings.into_iter().collect(),
                })
            })
            .collect();

        for (id, definition) in &self.definitions {
            let keys = self
                .user_bindings
                .get(id)
                .map(|keys| normalize_keys(keys))
                .unwrap_or_else(|| normalize_keys(&definition.default_keys));
            self.keys_by_id.insert(id.clone(), keys);
        }
    }
}

impl Default for KeybindingsManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

static GLOBAL_KEYBINDINGS: OnceLock<RwLock<KeybindingsManager>> = OnceLock::new();

pub fn set_keybindings(keybindings: KeybindingsManager) {
    let lock = GLOBAL_KEYBINDINGS.get_or_init(|| RwLock::new(KeybindingsManager::default()));
    *lock.write().expect("global keybindings lock poisoned") = keybindings;
}

pub fn get_keybindings() -> KeybindingsManager {
    GLOBAL_KEYBINDINGS
        .get_or_init(|| RwLock::new(KeybindingsManager::default()))
        .read()
        .expect("global keybindings lock poisoned")
        .clone()
}

pub fn default_keybindings() -> BTreeMap<String, KeybindingDefinition> {
    let mut map = BTreeMap::new();
    for (id, keys, description) in [
        ("tui.editor.cursorUp", vec!["up"], "Move cursor up"),
        ("tui.editor.cursorDown", vec!["down"], "Move cursor down"),
        (
            "tui.editor.cursorLeft",
            vec!["left", "ctrl+b"],
            "Move cursor left",
        ),
        (
            "tui.editor.cursorRight",
            vec!["right", "ctrl+f"],
            "Move cursor right",
        ),
        (
            "tui.editor.cursorWordLeft",
            vec!["alt+left", "ctrl+left", "alt+b"],
            "Move cursor word left",
        ),
        (
            "tui.editor.cursorWordRight",
            vec!["alt+right", "ctrl+right", "alt+f"],
            "Move cursor word right",
        ),
        (
            "tui.editor.cursorLineStart",
            vec!["home", "ctrl+a"],
            "Move to line start",
        ),
        (
            "tui.editor.cursorLineEnd",
            vec!["end", "ctrl+e"],
            "Move to line end",
        ),
        (
            "tui.editor.jumpForward",
            vec!["ctrl+]"],
            "Jump forward to character",
        ),
        (
            "tui.editor.jumpBackward",
            vec!["ctrl+alt+]"],
            "Jump backward to character",
        ),
        ("tui.editor.pageUp", vec!["pageUp"], "Page up"),
        ("tui.editor.pageDown", vec!["pageDown"], "Page down"),
        (
            "tui.editor.deleteCharBackward",
            vec!["backspace"],
            "Delete character backward",
        ),
        (
            "tui.editor.deleteCharForward",
            vec!["delete", "ctrl+d"],
            "Delete character forward",
        ),
        (
            "tui.editor.deleteWordBackward",
            vec!["ctrl+w", "alt+backspace"],
            "Delete word backward",
        ),
        (
            "tui.editor.deleteWordForward",
            vec!["alt+d", "alt+delete"],
            "Delete word forward",
        ),
        (
            "tui.editor.deleteToLineStart",
            vec!["ctrl+u"],
            "Delete to line start",
        ),
        (
            "tui.editor.deleteToLineEnd",
            vec!["ctrl+k"],
            "Delete to line end",
        ),
        ("tui.editor.yank", vec!["ctrl+y"], "Yank"),
        ("tui.editor.yankPop", vec!["alt+y"], "Yank pop"),
        ("tui.editor.undo", vec!["ctrl+-"], "Undo"),
        ("tui.input.newLine", vec!["shift+enter"], "Insert newline"),
        ("tui.input.submit", vec!["enter"], "Submit input"),
        ("tui.input.tab", vec!["tab"], "Tab / autocomplete"),
        ("tui.input.copy", vec!["ctrl+c"], "Copy selection"),
        ("tui.select.up", vec!["up"], "Move selection up"),
        ("tui.select.down", vec!["down"], "Move selection down"),
        ("tui.select.pageUp", vec!["pageUp"], "Selection page up"),
        (
            "tui.select.pageDown",
            vec!["pageDown"],
            "Selection page down",
        ),
        ("tui.select.confirm", vec!["enter"], "Confirm selection"),
        (
            "tui.select.cancel",
            vec!["escape", "ctrl+c"],
            "Cancel selection",
        ),
    ] {
        map.insert(
            id.to_string(),
            KeybindingDefinition {
                default_keys: keys.into_iter().map(str::to_string).collect(),
                description: Some(description.to_string()),
            },
        );
    }
    map
}

fn normalize_keys(keys: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    keys.iter()
        .filter(|key| seen.insert((*key).clone()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_match_key_data() {
        let manager = KeybindingsManager::default();
        assert!(manager.matches("\x1b[A", "tui.editor.cursorUp"));
        assert!(manager.matches("\r", "tui.input.submit"));
    }

    #[test]
    fn user_bindings_override_and_report_conflicts() {
        let mut config = BTreeMap::new();
        config.insert("tui.input.submit".to_string(), vec!["ctrl+x".to_string()]);
        config.insert("tui.input.copy".to_string(), vec!["ctrl+x".to_string()]);
        let manager = KeybindingsManager::new(default_keybindings(), config);
        assert!(manager.matches("\x18", "tui.input.submit"));
        assert_eq!(manager.conflicts().len(), 1);
    }

    #[test]
    fn global_keybindings_can_be_replaced_and_read_like_pi() {
        let original = get_keybindings();
        let mut config = BTreeMap::new();
        config.insert("tui.input.submit".to_string(), vec!["ctrl+x".to_string()]);
        let custom = KeybindingsManager::new(default_keybindings(), config);

        set_keybindings(custom);
        let active = get_keybindings();
        assert!(active.matches("\x18", "tui.input.submit"));
        assert!(!active.matches("\r", "tui.input.submit"));

        set_keybindings(original);
    }
}
