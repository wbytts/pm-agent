use serde_json::{Map, Value};

use crate::http_dispatcher::{parse_http_idle_timeout_ms, DEFAULT_HTTP_IDLE_TIMEOUT_MS};

mod merge;
mod migration;
mod storage;
mod types;

pub use merge::deep_merge_settings;
pub use migration::{migrate_commands_to_prompts, migrate_settings};
pub use storage::{FileSettingsStorage, InMemorySettingsStorage, SettingsStorage, CONFIG_DIR_NAME};
pub use types::{
    BranchSummarySettings, CompactionSettings, ImageSettings, ProviderRetrySettings, RetrySettings,
    Settings, SettingsError, SettingsScope, TerminalSettings, WarningSettings,
};

pub struct SettingsManager<S: SettingsStorage> {
    storage: S,
    global_settings: Value,
    project_settings: Value,
    settings: Value,
    errors: Vec<SettingsError>,
}

impl SettingsManager<InMemorySettingsStorage> {
    pub fn in_memory(settings: Value) -> Self {
        let mut storage = InMemorySettingsStorage::new();
        let migrated = migrate_settings(settings);
        storage
            .write(
                SettingsScope::Global,
                serde_json::to_string_pretty(&migrated).expect("settings should encode"),
            )
            .expect("memory settings write should work");
        Self::from_storage(storage)
    }
}

impl<S: SettingsStorage> SettingsManager<S> {
    pub fn from_storage(storage: S) -> Self {
        let mut manager = Self {
            storage,
            global_settings: Value::Object(Map::new()),
            project_settings: Value::Object(Map::new()),
            settings: Value::Object(Map::new()),
            errors: Vec::new(),
        };
        manager.reload();
        manager
    }

    pub fn reload(&mut self) {
        self.global_settings = self.load_scope(SettingsScope::Global);
        self.project_settings = self.load_scope(SettingsScope::Project);
        self.settings = deep_merge_settings(&self.global_settings, &self.project_settings);
    }

    pub fn global_settings(&self) -> Settings {
        value_to_settings(&self.global_settings)
    }

    pub fn project_settings(&self) -> Settings {
        value_to_settings(&self.project_settings)
    }

    pub fn settings(&self) -> Settings {
        value_to_settings(&self.settings)
    }

    pub fn apply_overrides(&mut self, overrides: Value) {
        self.settings = deep_merge_settings(&self.settings, &migrate_settings(overrides));
    }

    pub fn drain_errors(&mut self) -> Vec<SettingsError> {
        std::mem::take(&mut self.errors)
    }

    pub fn flush(&mut self) {}

    pub fn get_session_dir(&self) -> Option<String> {
        self.get_string("sessionDir")
    }

    pub fn get_default_provider(&self) -> Option<String> {
        self.get_string("defaultProvider")
    }

    pub fn get_default_model(&self) -> Option<String> {
        self.get_string("defaultModel")
    }

    pub fn get_theme(&self) -> Option<String> {
        self.get_string("theme")
    }

    pub fn set_theme(&mut self, theme: String) {
        self.set_global("theme", Value::String(theme));
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_default_model_and_provider(&mut self, provider: String, model_id: String) {
        self.set_global("defaultProvider", Value::String(provider));
        self.set_global("defaultModel", Value::String(model_id));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_default_thinking_level(&self) -> Option<String> {
        self.get_string("defaultThinkingLevel")
    }

    pub fn set_default_thinking_level(&mut self, level: String) {
        self.set_global("defaultThinkingLevel", Value::String(level));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_transport(&self) -> String {
        self.get_string("transport")
            .unwrap_or_else(|| "auto".to_string())
    }

    pub fn get_steering_mode(&self) -> String {
        self.get_string("steeringMode")
            .unwrap_or_else(|| "one-at-a-time".to_string())
    }

    pub fn get_follow_up_mode(&self) -> String {
        self.get_string("followUpMode")
            .unwrap_or_else(|| "one-at-a-time".to_string())
    }

    pub fn get_compaction_settings(&self) -> (bool, u64, u64) {
        (
            self.get_nested_bool("compaction", "enabled")
                .unwrap_or(true),
            self.get_nested_u64("compaction", "reserveTokens")
                .unwrap_or(16_384),
            self.get_nested_u64("compaction", "keepRecentTokens")
                .unwrap_or(20_000),
        )
    }

    pub fn set_compaction_enabled(&mut self, enabled: bool) {
        self.set_global_nested("compaction", "enabled", Value::Bool(enabled));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_branch_summary_settings(&self) -> (u64, bool) {
        (
            self.get_nested_u64("branchSummary", "reserveTokens")
                .unwrap_or(16_384),
            self.get_nested_bool("branchSummary", "skipPrompt")
                .unwrap_or(false),
        )
    }

    pub fn get_retry_settings(&self) -> (bool, u64, u64) {
        (
            self.get_nested_bool("retry", "enabled").unwrap_or(true),
            self.get_nested_u64("retry", "maxRetries").unwrap_or(3),
            self.get_nested_u64("retry", "baseDelayMs").unwrap_or(2_000),
        )
    }

    pub fn get_provider_retry_settings(&self) -> (Option<u64>, Option<u64>, u64) {
        (
            self.get_nested_path_u64(&["retry", "provider", "timeoutMs"]),
            self.get_nested_path_u64(&["retry", "provider", "maxRetries"]),
            self.get_nested_path_u64(&["retry", "provider", "maxRetryDelayMs"])
                .unwrap_or(60_000),
        )
    }

    pub fn get_hide_thinking_block(&self) -> bool {
        self.get_bool("hideThinkingBlock").unwrap_or(false)
    }

    pub fn set_hide_thinking_block(&mut self, hide: bool) {
        self.set_global("hideThinkingBlock", Value::Bool(hide));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_packages(&self) -> Vec<Value> {
        self.get_array("packages")
    }

    pub fn get_global_packages(&self) -> Vec<Value> {
        value_array(&self.global_settings, "packages")
    }

    pub fn get_project_packages(&self) -> Vec<Value> {
        value_array(&self.project_settings, "packages")
    }

    pub fn set_packages(&mut self, packages: Vec<Value>) {
        self.set_global("packages", Value::Array(packages));
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_project_packages(&mut self, packages: Vec<Value>) {
        self.set_project("packages", Value::Array(packages));
        self.save_scope(SettingsScope::Project);
    }

    pub fn get_extension_paths(&self) -> Vec<String> {
        self.get_string_array("extensions")
    }

    pub fn set_extension_paths(&mut self, paths: Vec<String>) {
        self.set_global_string_array("extensions", paths);
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_project_extension_paths(&mut self, paths: Vec<String>) {
        self.set_project_string_array("extensions", paths);
        self.save_scope(SettingsScope::Project);
    }

    pub fn get_skill_paths(&self) -> Vec<String> {
        self.get_string_array("skills")
    }

    pub fn set_skill_paths(&mut self, paths: Vec<String>) {
        self.set_global_string_array("skills", paths);
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_project_skill_paths(&mut self, paths: Vec<String>) {
        self.set_project_string_array("skills", paths);
        self.save_scope(SettingsScope::Project);
    }

    pub fn get_prompt_template_paths(&self) -> Vec<String> {
        self.get_string_array("prompts")
    }

    pub fn set_prompt_template_paths(&mut self, paths: Vec<String>) {
        self.set_global_string_array("prompts", paths);
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_project_prompt_template_paths(&mut self, paths: Vec<String>) {
        self.set_project_string_array("prompts", paths);
        self.save_scope(SettingsScope::Project);
    }

    pub fn get_theme_paths(&self) -> Vec<String> {
        self.get_string_array("themes")
    }

    pub fn set_theme_paths(&mut self, paths: Vec<String>) {
        self.set_global_string_array("themes", paths);
        self.save_scope(SettingsScope::Global);
    }

    pub fn set_project_theme_paths(&mut self, paths: Vec<String>) {
        self.set_project_string_array("themes", paths);
        self.save_scope(SettingsScope::Project);
    }

    pub fn get_enable_skill_commands(&self) -> bool {
        self.get_bool("enableSkillCommands").unwrap_or(true)
    }

    pub fn get_show_images(&self) -> bool {
        self.get_nested_bool("terminal", "showImages")
            .unwrap_or(true)
    }

    pub fn set_show_images(&mut self, show: bool) {
        self.set_global_nested("terminal", "showImages", Value::Bool(show));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_image_width_cells(&self) -> u64 {
        self.get_nested_u64("terminal", "imageWidthCells")
            .filter(|value| *value > 0)
            .unwrap_or(60)
    }

    pub fn get_clear_on_shrink(&self) -> bool {
        self.get_nested_bool("terminal", "clearOnShrink")
            .unwrap_or_else(|| std::env::var("PI_CLEAR_ON_SHRINK").is_ok_and(|value| value == "1"))
    }

    pub fn get_show_terminal_progress(&self) -> bool {
        self.get_nested_bool("terminal", "showTerminalProgress")
            .unwrap_or(false)
    }

    pub fn get_image_auto_resize(&self) -> bool {
        self.get_nested_bool("images", "autoResize").unwrap_or(true)
    }

    pub fn get_block_images(&self) -> bool {
        self.get_nested_bool("images", "blockImages")
            .unwrap_or(false)
    }

    pub fn get_enable_install_telemetry(&self) -> bool {
        self.get_bool("enableInstallTelemetry").unwrap_or(true)
    }

    pub fn get_http_idle_timeout_ms(&self) -> u64 {
        self.settings
            .get("httpIdleTimeoutMs")
            .and_then(parse_http_idle_timeout_value)
            .unwrap_or(DEFAULT_HTTP_IDLE_TIMEOUT_MS)
    }

    pub fn get_enabled_models(&self) -> Option<Vec<String>> {
        let values = self.get_string_array("enabledModels");
        (!values.is_empty()).then_some(values)
    }

    pub fn set_enabled_models(&mut self, patterns: Option<Vec<String>>) {
        match patterns {
            Some(patterns) => self.set_global_string_array("enabledModels", patterns),
            None => {
                object_mut(&mut self.global_settings).remove("enabledModels");
            }
        }
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_double_escape_action(&self) -> String {
        match self.get_string("doubleEscapeAction").as_deref() {
            Some("fork" | "tree" | "none") => {
                self.get_string("doubleEscapeAction").unwrap_or_default()
            }
            _ => "tree".to_string(),
        }
    }

    pub fn get_tree_filter_mode(&self) -> String {
        match self.get_string("treeFilterMode").as_deref() {
            Some("default" | "no-tools" | "user-only" | "labeled-only" | "all") => {
                self.get_string("treeFilterMode").unwrap_or_default()
            }
            _ => "default".to_string(),
        }
    }

    pub fn get_show_hardware_cursor(&self) -> bool {
        self.get_bool("showHardwareCursor")
            .unwrap_or_else(|| std::env::var("PI_HARDWARE_CURSOR").is_ok_and(|value| value == "1"))
    }

    pub fn set_show_hardware_cursor(&mut self, enabled: bool) {
        self.set_global("showHardwareCursor", Value::Bool(enabled));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_editor_padding_x(&self) -> i64 {
        self.settings
            .get("editorPaddingX")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    pub fn set_editor_padding_x(&mut self, padding: i64) {
        self.set_global("editorPaddingX", Value::from(padding.clamp(0, 3)));
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_autocomplete_max_visible(&self) -> i64 {
        self.settings
            .get("autocompleteMaxVisible")
            .and_then(Value::as_i64)
            .unwrap_or(5)
    }

    pub fn set_autocomplete_max_visible(&mut self, max_visible: i64) {
        self.set_global(
            "autocompleteMaxVisible",
            Value::from(max_visible.clamp(3, 20)),
        );
        self.save_scope(SettingsScope::Global);
    }

    pub fn get_code_block_indent(&self) -> String {
        self.settings
            .get("markdown")
            .and_then(|value| value.get("codeBlockIndent"))
            .and_then(Value::as_str)
            .unwrap_or("  ")
            .to_string()
    }

    pub fn get_warnings(&self) -> WarningSettings {
        self.settings
            .get("warnings")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    pub fn set_warnings(&mut self, warnings: WarningSettings) {
        self.set_global(
            "warnings",
            serde_json::to_value(warnings).expect("warning settings should encode"),
        );
        self.save_scope(SettingsScope::Global);
    }

    fn load_scope(&mut self, scope: SettingsScope) -> Value {
        match self.storage.read(scope) {
            Ok(Some(content)) => serde_json::from_str::<Value>(&content)
                .map(migrate_settings)
                .unwrap_or_else(|error| {
                    self.errors.push(SettingsError {
                        scope,
                        message: error.to_string(),
                    });
                    Value::Object(Map::new())
                }),
            Ok(None) => Value::Object(Map::new()),
            Err(error) => {
                self.errors.push(SettingsError {
                    scope,
                    message: error,
                });
                Value::Object(Map::new())
            }
        }
    }

    fn save_scope(&mut self, scope: SettingsScope) {
        self.settings = deep_merge_settings(&self.global_settings, &self.project_settings);
        let value = match scope {
            SettingsScope::Global => &self.global_settings,
            SettingsScope::Project => &self.project_settings,
        };
        let content = serde_json::to_string_pretty(value).expect("settings should encode");
        if let Err(error) = self.storage.write(scope, content) {
            self.errors.push(SettingsError {
                scope,
                message: error,
            });
        }
    }

    fn get_string(&self, key: &str) -> Option<String> {
        self.settings
            .get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        self.settings.get(key).and_then(Value::as_bool)
    }

    fn get_array(&self, key: &str) -> Vec<Value> {
        self.settings
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn get_string_array(&self, key: &str) -> Vec<String> {
        self.get_array(key)
            .into_iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect()
    }

    fn get_nested_bool(&self, parent: &str, key: &str) -> Option<bool> {
        self.settings.get(parent)?.get(key)?.as_bool()
    }

    fn get_nested_u64(&self, parent: &str, key: &str) -> Option<u64> {
        self.settings.get(parent)?.get(key)?.as_u64()
    }

    fn get_nested_path_u64(&self, path: &[&str]) -> Option<u64> {
        let mut value = &self.settings;
        for key in path {
            value = value.get(*key)?;
        }
        value.as_u64()
    }

    fn set_global(&mut self, key: &str, value: Value) {
        object_mut(&mut self.global_settings).insert(key.to_string(), value);
    }

    fn set_project(&mut self, key: &str, value: Value) {
        object_mut(&mut self.project_settings).insert(key.to_string(), value);
    }

    fn set_global_string_array(&mut self, key: &str, values: Vec<String>) {
        self.set_global(
            key,
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }

    fn set_project_string_array(&mut self, key: &str, values: Vec<String>) {
        self.set_project(
            key,
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }

    fn set_global_nested(&mut self, parent: &str, key: &str, value: Value) {
        let object = object_mut(&mut self.global_settings);
        let parent_value = object
            .entry(parent.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        object_mut(parent_value).insert(key.to_string(), value);
    }
}

fn parse_http_idle_timeout_value(value: &Value) -> Option<u64> {
    match value {
        Value::String(value) => parse_http_idle_timeout_ms(value.as_str()),
        Value::Number(value) => value
            .as_u64()
            .and_then(|value| parse_http_idle_timeout_ms(value)),
        _ => None,
    }
}

fn value_to_settings(value: &Value) -> Settings {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn value_array(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("value should be object")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pm-agent-settings-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn migrates_legacy_settings() {
        let migrated = migrate_settings(json!({
            "queueMode": "all",
            "websockets": true,
            "skills": {
                "enableSkillCommands": false,
                "customDirectories": ["/skills"]
            },
            "retry": {
                "maxDelayMs": 1234
            }
        }));

        assert_eq!(migrated["steeringMode"], "all");
        assert_eq!(migrated["transport"], "websocket");
        assert_eq!(migrated["enableSkillCommands"], false);
        assert_eq!(migrated["skills"][0], "/skills");
        assert_eq!(migrated["retry"]["provider"]["maxRetryDelayMs"], 1234);
    }

    #[test]
    fn migrates_commands_directory_to_prompts_like_pi() {
        let dir = temp_dir("commands-to-prompts");
        let commands_dir = dir.join("commands");
        let prompts_dir = dir.join("prompts");
        std::fs::create_dir_all(&commands_dir).expect("commands dir should be created");
        std::fs::write(commands_dir.join("review.md"), "review")
            .expect("command prompt should be written");

        assert!(migrate_commands_to_prompts(&dir).expect("migration should succeed"));

        assert!(!commands_dir.exists());
        assert!(prompts_dir.join("review.md").exists());
    }

    #[test]
    fn keeps_commands_when_prompts_already_exists_like_pi() {
        let dir = temp_dir("commands-to-prompts-existing");
        let commands_dir = dir.join("commands");
        let prompts_dir = dir.join("prompts");
        std::fs::create_dir_all(&commands_dir).expect("commands dir should be created");
        std::fs::create_dir_all(&prompts_dir).expect("prompts dir should be created");
        std::fs::write(commands_dir.join("legacy.md"), "legacy")
            .expect("legacy command should be written");
        std::fs::write(prompts_dir.join("current.md"), "current")
            .expect("current prompt should be written");

        assert!(!migrate_commands_to_prompts(&dir).expect("migration should succeed"));

        assert!(commands_dir.join("legacy.md").exists());
        assert!(prompts_dir.join("current.md").exists());
    }

    #[test]
    fn project_settings_override_and_merge_global_settings() {
        let mut storage = InMemorySettingsStorage::new();
        storage
            .write(
                SettingsScope::Global,
                json!({
                    "defaultProvider": "openai",
                    "compaction": { "enabled": true, "reserveTokens": 10 }
                })
                .to_string(),
            )
            .expect("global write should work");
        storage
            .write(
                SettingsScope::Project,
                json!({
                    "compaction": { "enabled": false }
                })
                .to_string(),
            )
            .expect("project write should work");

        let manager = SettingsManager::from_storage(storage);
        assert_eq!(manager.get_default_provider().as_deref(), Some("openai"));
        assert_eq!(manager.get_compaction_settings(), (false, 10, 20_000));
    }

    #[test]
    fn writes_global_setting_updates() {
        let storage = InMemorySettingsStorage::new();
        let mut manager = SettingsManager::from_storage(storage);
        manager.set_default_model_and_provider("anthropic".to_string(), "claude".to_string());
        assert_eq!(manager.get_default_provider().as_deref(), Some("anthropic"));
        assert_eq!(manager.get_default_model().as_deref(), Some("claude"));
    }

    #[test]
    fn clear_on_shrink_falls_back_to_pi_env_like_pi_settings() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let previous = std::env::var_os("PI_CLEAR_ON_SHRINK");
        unsafe {
            std::env::set_var("PI_CLEAR_ON_SHRINK", "1");
        }

        let manager = SettingsManager::in_memory(json!({}));
        assert!(manager.get_clear_on_shrink());

        let manager = SettingsManager::in_memory(json!({
            "terminal": {
                "clearOnShrink": false
            }
        }));
        assert!(!manager.get_clear_on_shrink());

        unsafe {
            match previous {
                Some(value) => std::env::set_var("PI_CLEAR_ON_SHRINK", value),
                None => std::env::remove_var("PI_CLEAR_ON_SHRINK"),
            }
        }
    }

    #[test]
    fn ui_tuning_getters_match_pi_settings_defaults_and_overrides() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let previous = std::env::var_os("PI_HARDWARE_CURSOR");
        unsafe {
            std::env::remove_var("PI_HARDWARE_CURSOR");
        }

        let manager = SettingsManager::in_memory(json!({}));
        assert!(!manager.get_show_hardware_cursor());
        assert_eq!(manager.get_editor_padding_x(), 0);
        assert_eq!(manager.get_autocomplete_max_visible(), 5);
        assert_eq!(manager.get_code_block_indent(), "  ");
        assert_eq!(manager.get_warnings().anthropic_extra_usage, None);

        let manager = SettingsManager::in_memory(json!({
            "showHardwareCursor": true,
            "editorPaddingX": 9,
            "autocompleteMaxVisible": 1,
            "markdown": {
                "codeBlockIndent": "\t"
            },
            "warnings": {
                "anthropicExtraUsage": false
            }
        }));
        assert!(manager.get_show_hardware_cursor());
        assert_eq!(manager.get_editor_padding_x(), 9);
        assert_eq!(manager.get_autocomplete_max_visible(), 1);
        assert_eq!(manager.get_code_block_indent(), "\t");
        assert_eq!(manager.get_warnings().anthropic_extra_usage, Some(false));

        unsafe {
            match previous {
                Some(value) => std::env::set_var("PI_HARDWARE_CURSOR", value),
                None => std::env::remove_var("PI_HARDWARE_CURSOR"),
            }
        }
    }

    #[test]
    fn hardware_cursor_falls_back_to_pi_env_like_pi_settings() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let previous = std::env::var_os("PI_HARDWARE_CURSOR");
        unsafe {
            std::env::set_var("PI_HARDWARE_CURSOR", "1");
        }

        let manager = SettingsManager::in_memory(json!({}));
        assert!(manager.get_show_hardware_cursor());

        let manager = SettingsManager::in_memory(json!({
            "showHardwareCursor": false
        }));
        assert!(!manager.get_show_hardware_cursor());

        unsafe {
            match previous {
                Some(value) => std::env::set_var("PI_HARDWARE_CURSOR", value),
                None => std::env::remove_var("PI_HARDWARE_CURSOR"),
            }
        }
    }

    #[test]
    fn ui_tuning_setters_clamp_values_like_pi_settings() {
        let mut manager = SettingsManager::in_memory(json!({}));

        manager.set_show_hardware_cursor(true);
        manager.set_editor_padding_x(9);
        manager.set_autocomplete_max_visible(1);
        manager.set_warnings(WarningSettings {
            anthropic_extra_usage: Some(false),
        });

        assert!(manager.get_show_hardware_cursor());
        assert_eq!(manager.get_editor_padding_x(), 3);
        assert_eq!(manager.get_autocomplete_max_visible(), 3);
        assert_eq!(manager.get_warnings().anthropic_extra_usage, Some(false));
    }
}
