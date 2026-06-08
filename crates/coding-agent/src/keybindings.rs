use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tui::{
    default_keybindings as tui_default_keybindings, KeybindingDefinition, KeybindingsConfig,
    KeybindingsManager,
};

#[derive(Debug, Clone)]
pub struct AppKeybindingsManager {
    manager: KeybindingsManager,
    config_path: Option<PathBuf>,
}

impl AppKeybindingsManager {
    pub fn new(user_bindings: KeybindingsConfig, config_path: Option<PathBuf>) -> Self {
        Self {
            manager: KeybindingsManager::new(app_keybindings(), user_bindings),
            config_path,
        }
    }

    pub fn create(agent_dir: impl AsRef<Path>) -> Self {
        let config_path = agent_dir.as_ref().join("keybindings.json");
        let user_bindings = load_keybindings_from_file(&config_path);
        Self::new(user_bindings, Some(config_path))
    }

    pub fn reload(&mut self) {
        if let Some(path) = &self.config_path {
            self.manager
                .set_user_bindings(load_keybindings_from_file(path));
        }
    }

    pub fn matches(&self, data: &str, keybinding: &str) -> bool {
        self.manager.matches(data, keybinding)
    }

    pub fn effective_config(&self) -> KeybindingsConfig {
        self.manager.resolved_bindings()
    }
}

pub fn app_keybindings() -> BTreeMap<String, KeybindingDefinition> {
    let mut map = tui_default_keybindings();
    for (id, keys, description) in [
        ("app.interrupt", vec!["escape"], "Cancel or abort"),
        ("app.clear", vec!["ctrl+c"], "Clear editor"),
        ("app.exit", vec!["ctrl+d"], "Exit when editor is empty"),
        (
            "app.suspend",
            if cfg!(target_os = "windows") {
                vec![]
            } else {
                vec!["ctrl+z"]
            },
            "Suspend to background",
        ),
        (
            "app.thinking.cycle",
            vec!["shift+tab"],
            "Cycle thinking level",
        ),
        (
            "app.model.cycleForward",
            vec!["ctrl+p"],
            "Cycle to next model",
        ),
        (
            "app.model.cycleBackward",
            vec!["shift+ctrl+p"],
            "Cycle to previous model",
        ),
        ("app.model.select", vec!["ctrl+l"], "Open model selector"),
        ("app.tools.expand", vec!["ctrl+o"], "Toggle tool output"),
        (
            "app.thinking.toggle",
            vec!["ctrl+t"],
            "Toggle thinking blocks",
        ),
        (
            "app.session.toggleNamedFilter",
            vec!["ctrl+n"],
            "Toggle named session filter",
        ),
        (
            "app.editor.external",
            vec!["ctrl+g"],
            "Open external editor",
        ),
        (
            "app.message.followUp",
            vec!["alt+enter"],
            "Queue follow-up message",
        ),
        (
            "app.message.dequeue",
            vec!["alt+up"],
            "Restore queued messages",
        ),
        (
            "app.clipboard.pasteImage",
            if cfg!(target_os = "windows") {
                vec!["alt+v"]
            } else {
                vec!["ctrl+v"]
            },
            "Paste image from clipboard",
        ),
        ("app.session.new", vec![], "Start a new session"),
        ("app.session.tree", vec![], "Open session tree"),
        ("app.session.fork", vec![], "Fork current session"),
        ("app.session.resume", vec![], "Resume a session"),
        (
            "app.tree.foldOrUp",
            vec!["ctrl+left", "alt+left"],
            "Fold tree branch or move up",
        ),
        (
            "app.tree.unfoldOrDown",
            vec!["ctrl+right", "alt+right"],
            "Unfold tree branch or move down",
        ),
        ("app.tree.editLabel", vec!["shift+l"], "Edit tree label"),
        (
            "app.tree.toggleLabelTimestamp",
            vec!["shift+t"],
            "Toggle tree label timestamps",
        ),
        (
            "app.session.togglePath",
            vec!["ctrl+p"],
            "Toggle session path display",
        ),
        (
            "app.session.toggleSort",
            vec!["ctrl+s"],
            "Toggle session sort mode",
        ),
        ("app.session.rename", vec!["ctrl+r"], "Rename session"),
        ("app.session.delete", vec!["ctrl+d"], "Delete session"),
        (
            "app.session.deleteNoninvasive",
            vec!["ctrl+backspace"],
            "Delete session when query is empty",
        ),
        ("app.models.save", vec!["ctrl+s"], "Save model selection"),
        ("app.models.enableAll", vec!["ctrl+a"], "Enable all models"),
        ("app.models.clearAll", vec!["ctrl+x"], "Clear all models"),
        (
            "app.models.toggleProvider",
            vec!["ctrl+p"],
            "Toggle all models for provider",
        ),
        (
            "app.models.reorderUp",
            vec!["alt+up"],
            "Move model up in order",
        ),
        (
            "app.models.reorderDown",
            vec!["alt+down"],
            "Move model down in order",
        ),
        (
            "app.tree.filter.default",
            vec!["ctrl+d"],
            "Tree filter: default view",
        ),
        (
            "app.tree.filter.noTools",
            vec!["ctrl+t"],
            "Tree filter: hide tool results",
        ),
        (
            "app.tree.filter.userOnly",
            vec!["ctrl+u"],
            "Tree filter: user messages only",
        ),
        (
            "app.tree.filter.labeledOnly",
            vec!["ctrl+l"],
            "Tree filter: labeled entries only",
        ),
        (
            "app.tree.filter.all",
            vec!["ctrl+a"],
            "Tree filter: show all entries",
        ),
        (
            "app.tree.filter.cycleForward",
            vec!["ctrl+o"],
            "Tree filter: cycle forward",
        ),
        (
            "app.tree.filter.cycleBackward",
            vec!["shift+ctrl+o"],
            "Tree filter: cycle backward",
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

pub fn migrate_keybindings_config(
    raw_config: &BTreeMap<String, Value>,
) -> (BTreeMap<String, Value>, bool) {
    let mut config = BTreeMap::new();
    let mut migrated = false;

    for (key, value) in raw_config {
        let next_key = legacy_keybinding_name(key).unwrap_or(key);
        if next_key != key {
            migrated = true;
        }
        if key != next_key && raw_config.contains_key(next_key) {
            migrated = true;
            continue;
        }
        config.insert(next_key.to_string(), value.clone());
    }

    (order_keybindings_config(config), migrated)
}

pub fn load_keybindings_from_file(path: impl AsRef<Path>) -> KeybindingsConfig {
    let Some(raw) = load_raw_config(path) else {
        return BTreeMap::new();
    };
    let (migrated, _) = migrate_keybindings_config(&raw);
    to_keybindings_config(&migrated)
}

fn to_keybindings_config(value: &BTreeMap<String, Value>) -> KeybindingsConfig {
    let mut config = BTreeMap::new();
    for (key, binding) in value {
        if let Some(single) = binding.as_str() {
            config.insert(key.clone(), vec![single.to_string()]);
            continue;
        }
        if let Some(values) = binding.as_array() {
            let keys = values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect::<Vec<_>>();
            if values.len() == keys.len() {
                config.insert(key.clone(), keys);
            }
        }
    }
    config
}

fn load_raw_config(path: impl AsRef<Path>) -> Option<BTreeMap<String, Value>> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&content)
        .ok()?
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
}

fn order_keybindings_config(config: BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let definitions = app_keybindings();
    let mut ordered = BTreeMap::new();
    for key in definitions.keys() {
        if let Some(value) = config.get(key) {
            ordered.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in config {
        ordered.entry(key).or_insert(value);
    }
    ordered
}

fn legacy_keybinding_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "cursorUp" => "tui.editor.cursorUp",
        "cursorDown" => "tui.editor.cursorDown",
        "cursorLeft" => "tui.editor.cursorLeft",
        "cursorRight" => "tui.editor.cursorRight",
        "cursorWordLeft" => "tui.editor.cursorWordLeft",
        "cursorWordRight" => "tui.editor.cursorWordRight",
        "cursorLineStart" => "tui.editor.cursorLineStart",
        "cursorLineEnd" => "tui.editor.cursorLineEnd",
        "jumpForward" => "tui.editor.jumpForward",
        "jumpBackward" => "tui.editor.jumpBackward",
        "pageUp" => "tui.editor.pageUp",
        "pageDown" => "tui.editor.pageDown",
        "deleteCharBackward" => "tui.editor.deleteCharBackward",
        "deleteCharForward" => "tui.editor.deleteCharForward",
        "deleteWordBackward" => "tui.editor.deleteWordBackward",
        "deleteWordForward" => "tui.editor.deleteWordForward",
        "deleteToLineStart" => "tui.editor.deleteToLineStart",
        "deleteToLineEnd" => "tui.editor.deleteToLineEnd",
        "yank" => "tui.editor.yank",
        "yankPop" => "tui.editor.yankPop",
        "undo" => "tui.editor.undo",
        "newLine" => "tui.input.newLine",
        "submit" => "tui.input.submit",
        "tab" => "tui.input.tab",
        "copy" => "tui.input.copy",
        "selectUp" => "tui.select.up",
        "selectDown" => "tui.select.down",
        "selectPageUp" => "tui.select.pageUp",
        "selectPageDown" => "tui.select.pageDown",
        "selectConfirm" => "tui.select.confirm",
        "selectCancel" => "tui.select.cancel",
        "interrupt" => "app.interrupt",
        "clear" => "app.clear",
        "exit" => "app.exit",
        "suspend" => "app.suspend",
        "cycleThinkingLevel" => "app.thinking.cycle",
        "cycleModelForward" => "app.model.cycleForward",
        "cycleModelBackward" => "app.model.cycleBackward",
        "selectModel" => "app.model.select",
        "expandTools" => "app.tools.expand",
        "toggleThinking" => "app.thinking.toggle",
        "toggleSessionNamedFilter" => "app.session.toggleNamedFilter",
        "externalEditor" => "app.editor.external",
        "followUp" => "app.message.followUp",
        "dequeue" => "app.message.dequeue",
        "pasteImage" => "app.clipboard.pasteImage",
        "newSession" => "app.session.new",
        "tree" => "app.session.tree",
        "fork" => "app.session.fork",
        "resume" => "app.session.resume",
        "treeFoldOrUp" => "app.tree.foldOrUp",
        "treeUnfoldOrDown" => "app.tree.unfoldOrDown",
        "treeEditLabel" => "app.tree.editLabel",
        "treeToggleLabelTimestamp" => "app.tree.toggleLabelTimestamp",
        "toggleSessionPath" => "app.session.togglePath",
        "toggleSessionSort" => "app.session.toggleSort",
        "renameSession" => "app.session.rename",
        "deleteSession" => "app.session.delete",
        "deleteSessionNoninvasive" => "app.session.deleteNoninvasive",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_keybindings_include_tui_and_app_defaults() {
        let manager = AppKeybindingsManager::new(BTreeMap::new(), None);
        assert!(manager.matches("\x1b", "app.interrupt"));
        assert!(manager.matches("\r", "tui.input.submit"));
    }

    #[test]
    fn migrates_legacy_names_and_prefers_new_name() {
        let raw = BTreeMap::from([
            ("submit".to_string(), json!("ctrl+x")),
            ("tui.input.submit".to_string(), json!("enter")),
            ("toggleThinking".to_string(), json!(["ctrl+t"])),
        ]);
        let (config, migrated) = migrate_keybindings_config(&raw);
        assert!(migrated);
        assert_eq!(config.get("tui.input.submit"), Some(&json!("enter")));
        assert_eq!(config.get("app.thinking.toggle"), Some(&json!(["ctrl+t"])));
    }

    #[test]
    fn converts_json_config_to_keybindings() {
        let raw = BTreeMap::from([
            ("app.interrupt".to_string(), json!("ctrl+c")),
            ("app.clear".to_string(), json!(["ctrl+l", "ctrl+k"])),
            ("bad".to_string(), json!([1, 2])),
        ]);
        let config = to_keybindings_config(&raw);
        assert_eq!(
            config.get("app.interrupt"),
            Some(&vec!["ctrl+c".to_string()])
        );
        assert_eq!(
            config.get("app.clear"),
            Some(&vec!["ctrl+l".to_string(), "ctrl+k".to_string()])
        );
        assert!(!config.contains_key("bad"));
    }
}
