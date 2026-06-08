use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub last_changelog_version: Option<String>,
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub default_thinking_level: Option<String>,
    pub transport: Option<String>,
    pub steering_mode: Option<String>,
    pub follow_up_mode: Option<String>,
    pub theme: Option<String>,
    pub compaction: Option<CompactionSettings>,
    pub branch_summary: Option<BranchSummarySettings>,
    pub retry: Option<RetrySettings>,
    pub hide_thinking_block: Option<bool>,
    pub shell_path: Option<String>,
    pub quiet_startup: Option<bool>,
    pub shell_command_prefix: Option<String>,
    pub npm_command: Option<Vec<String>>,
    pub collapse_changelog: Option<bool>,
    pub enable_install_telemetry: Option<bool>,
    pub packages: Option<Vec<Value>>,
    pub extensions: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub themes: Option<Vec<String>>,
    pub enable_skill_commands: Option<bool>,
    pub terminal: Option<TerminalSettings>,
    pub images: Option<ImageSettings>,
    pub enabled_models: Option<Vec<String>>,
    pub double_escape_action: Option<String>,
    pub tree_filter_mode: Option<String>,
    pub thinking_budgets: Option<Value>,
    pub editor_padding_x: Option<i64>,
    pub autocomplete_max_visible: Option<i64>,
    pub show_hardware_cursor: Option<bool>,
    pub markdown: Option<Value>,
    pub warnings: Option<Value>,
    pub session_dir: Option<String>,
    pub http_idle_timeout_ms: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_value(Value::Object(Map::new()))
            .expect("empty settings should deserialize")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSettings {
    pub enabled: Option<bool>,
    pub reserve_tokens: Option<u64>,
    pub keep_recent_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummarySettings {
    pub reserve_tokens: Option<u64>,
    pub skip_prompt: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetrySettings {
    pub enabled: Option<bool>,
    pub max_retries: Option<u64>,
    pub base_delay_ms: Option<u64>,
    pub provider: Option<ProviderRetrySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRetrySettings {
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u64>,
    pub max_retry_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSettings {
    pub show_images: Option<bool>,
    pub image_width_cells: Option<u64>,
    pub clear_on_shrink: Option<bool>,
    pub show_terminal_progress: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageSettings {
    pub auto_resize: Option<bool>,
    pub block_images: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WarningSettings {
    pub anthropic_extra_usage: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    Global,
    Project,
}

#[derive(Debug, Clone)]
pub struct SettingsError {
    pub scope: SettingsScope,
    pub message: String,
}
