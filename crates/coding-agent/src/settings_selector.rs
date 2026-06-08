use ai::ModelThinkingLevel;

use crate::http_dispatcher::{format_http_idle_timeout_ms, HTTP_IDLE_TIMEOUT_CHOICES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsCapabilities {
    pub images: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWarningSettings {
    pub anthropic_extra_usage: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessageMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTransport {
    Sse,
    WebSocket,
    WebSocketCached,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsDoubleEscapeAction {
    Fork,
    Tree,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTreeFilterMode {
    Default,
    NoTools,
    UserOnly,
    LabeledOnly,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSelectorConfig {
    pub auto_compact: bool,
    pub show_images: bool,
    pub image_width_cells: usize,
    pub auto_resize_images: bool,
    pub block_images: bool,
    pub enable_skill_commands: bool,
    pub steering_mode: SettingsMessageMode,
    pub follow_up_mode: SettingsMessageMode,
    pub transport: SettingsTransport,
    pub http_idle_timeout_ms: u64,
    pub thinking_level: ModelThinkingLevel,
    pub available_thinking_levels: Vec<ModelThinkingLevel>,
    pub current_theme: String,
    pub available_themes: Vec<String>,
    pub hide_thinking_block: bool,
    pub collapse_changelog: bool,
    pub enable_install_telemetry: bool,
    pub double_escape_action: SettingsDoubleEscapeAction,
    pub tree_filter_mode: SettingsTreeFilterMode,
    pub show_hardware_cursor: bool,
    pub editor_padding_x: usize,
    pub autocomplete_max_visible: usize,
    pub quiet_startup: bool,
    pub clear_on_shrink: bool,
    pub show_terminal_progress: bool,
    pub warnings: SettingsWarningSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSelectorItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub current_value: String,
    pub values: Vec<String>,
    pub has_submenu: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsChangeAction {
    AutoCompact(bool),
    ShowImages(bool),
    ImageWidthCells(usize),
    AutoResizeImages(bool),
    BlockImages(bool),
    EnableSkillCommands(bool),
    SteeringMode(SettingsMessageMode),
    FollowUpMode(SettingsMessageMode),
    Transport(SettingsTransport),
    HttpIdleTimeoutMs(u64),
    ThinkingLevel(ModelThinkingLevel),
    Theme(String),
    HideThinkingBlock(bool),
    CollapseChangelog(bool),
    QuietStartup(bool),
    EnableInstallTelemetry(bool),
    DoubleEscapeAction(SettingsDoubleEscapeAction),
    TreeFilterMode(SettingsTreeFilterMode),
    ShowHardwareCursor(bool),
    EditorPaddingX(usize),
    AutocompleteMaxVisible(usize),
    ClearOnShrink(bool),
    ShowTerminalProgress(bool),
    Warnings(SettingsWarningSettings),
}

pub fn build_settings_items(
    config: &SettingsSelectorConfig,
    capabilities: SettingsCapabilities,
) -> Vec<SettingsSelectorItem> {
    let mut items = vec![
        item(
            "autocompact",
            "Auto-compact",
            "Automatically compact context when it gets too large",
            bool_value(config.auto_compact),
            vec!["true", "false"],
        ),
        item(
            "steering-mode",
            "Steering mode",
            "Enter while streaming queues steering messages. 'one-at-a-time': deliver one, wait for response. 'all': deliver all at once.",
            message_mode_value(config.steering_mode),
            vec!["one-at-a-time", "all"],
        ),
        item(
            "follow-up-mode",
            "Follow-up mode",
            "Follow-up messages queue until agent stops. 'one-at-a-time': deliver one, wait for response. 'all': deliver all at once.",
            message_mode_value(config.follow_up_mode),
            vec!["one-at-a-time", "all"],
        ),
        item(
            "transport",
            "Transport",
            "Preferred transport for providers that support multiple transports",
            transport_value(config.transport),
            vec!["sse", "websocket", "websocket-cached", "auto"],
        ),
        item(
            "http-idle-timeout",
            "HTTP idle timeout",
            "Maximum idle gap while waiting for HTTP headers or body chunks. Disable for local models that pause longer than five minutes.",
            format_http_idle_timeout_ms(config.http_idle_timeout_ms),
            HTTP_IDLE_TIMEOUT_CHOICES
                .iter()
                .map(|choice| choice.label)
                .collect(),
        ),
        item(
            "hide-thinking",
            "Hide thinking",
            "Hide thinking blocks in assistant responses",
            bool_value(config.hide_thinking_block),
            vec!["true", "false"],
        ),
        item(
            "collapse-changelog",
            "Collapse changelog",
            "Show condensed changelog after updates",
            bool_value(config.collapse_changelog),
            vec!["true", "false"],
        ),
        item(
            "quiet-startup",
            "Quiet startup",
            "Disable verbose printing at startup",
            bool_value(config.quiet_startup),
            vec!["true", "false"],
        ),
        item(
            "install-telemetry",
            "Install telemetry",
            "Send an anonymous version/update ping after changelog-detected updates",
            bool_value(config.enable_install_telemetry),
            vec!["true", "false"],
        ),
        item(
            "double-escape-action",
            "Double-escape action",
            "Action when pressing Escape twice with empty editor",
            double_escape_value(config.double_escape_action),
            vec!["tree", "fork", "none"],
        ),
        item(
            "tree-filter-mode",
            "Tree filter mode",
            "Default filter when opening /tree",
            tree_filter_value(config.tree_filter_mode),
            vec!["default", "no-tools", "user-only", "labeled-only", "all"],
        ),
        submenu_item(
            "warnings",
            "Warnings",
            "Enable or disable individual warnings",
            "configure",
        ),
        submenu_item(
            "thinking",
            "Thinking level",
            "Reasoning depth for thinking-capable models",
            thinking_level_value(config.thinking_level),
        )
        .with_values(
            config
                .available_thinking_levels
                .iter()
                .map(|level| thinking_level_value(*level))
                .collect(),
        ),
        submenu_item(
            "theme",
            "Theme",
            "Color theme for the interface",
            config.current_theme.clone(),
        )
        .with_values(config.available_themes.clone()),
    ];

    if capabilities.images {
        items.insert(
            1,
            item(
                "show-images",
                "Show images",
                "Render images inline in terminal",
                bool_value(config.show_images),
                vec!["true", "false"],
            ),
        );
        items.insert(
            2,
            item(
                "image-width-cells",
                "Image width",
                "Preferred inline image width in terminal cells",
                config.image_width_cells.to_string(),
                vec!["60", "80", "120"],
            ),
        );
    }

    let insert_at = if capabilities.images { 3 } else { 1 };
    items.insert(
        insert_at,
        item(
            "auto-resize-images",
            "Auto-resize images",
            "Resize large images to 2000x2000 max for better model compatibility",
            bool_value(config.auto_resize_images),
            vec!["true", "false"],
        ),
    );
    insert_after(
        &mut items,
        "auto-resize-images",
        item(
            "block-images",
            "Block images",
            "Prevent images from being sent to LLM providers",
            bool_value(config.block_images),
            vec!["true", "false"],
        ),
    );
    insert_after(
        &mut items,
        "block-images",
        item(
            "skill-commands",
            "Skill commands",
            "Register skills as /skill:name commands",
            bool_value(config.enable_skill_commands),
            vec!["true", "false"],
        ),
    );
    insert_after(
        &mut items,
        "skill-commands",
        item(
            "show-hardware-cursor",
            "Show hardware cursor",
            "Show the terminal cursor while still positioning it for IME support",
            bool_value(config.show_hardware_cursor),
            vec!["true", "false"],
        ),
    );
    insert_after(
        &mut items,
        "show-hardware-cursor",
        item(
            "editor-padding",
            "Editor padding",
            "Horizontal padding for input editor (0-3)",
            config.editor_padding_x.to_string(),
            vec!["0", "1", "2", "3"],
        ),
    );
    insert_after(
        &mut items,
        "editor-padding",
        item(
            "autocomplete-max-visible",
            "Autocomplete max items",
            "Max visible items in autocomplete dropdown (3-20)",
            config.autocomplete_max_visible.to_string(),
            vec!["3", "5", "7", "10", "15", "20"],
        ),
    );
    insert_after(
        &mut items,
        "autocomplete-max-visible",
        item(
            "clear-on-shrink",
            "Clear on shrink",
            "Clear empty rows when content shrinks (may cause flicker)",
            bool_value(config.clear_on_shrink),
            vec!["true", "false"],
        ),
    );
    insert_after(
        &mut items,
        "clear-on-shrink",
        item(
            "terminal-progress",
            "Terminal progress",
            "Show OSC 9;4 progress indicators in the terminal tab bar",
            bool_value(config.show_terminal_progress),
            vec!["true", "false"],
        ),
    );

    items
}

pub fn settings_change_action(id: &str, new_value: &str) -> Option<SettingsChangeAction> {
    match id {
        "autocompact" => Some(SettingsChangeAction::AutoCompact(parse_bool(new_value)?)),
        "show-images" => Some(SettingsChangeAction::ShowImages(parse_bool(new_value)?)),
        "image-width-cells" => Some(SettingsChangeAction::ImageWidthCells(
            new_value.parse().ok()?,
        )),
        "auto-resize-images" => Some(SettingsChangeAction::AutoResizeImages(parse_bool(
            new_value,
        )?)),
        "block-images" => Some(SettingsChangeAction::BlockImages(parse_bool(new_value)?)),
        "skill-commands" => Some(SettingsChangeAction::EnableSkillCommands(parse_bool(
            new_value,
        )?)),
        "steering-mode" => Some(SettingsChangeAction::SteeringMode(parse_message_mode(
            new_value,
        )?)),
        "follow-up-mode" => Some(SettingsChangeAction::FollowUpMode(parse_message_mode(
            new_value,
        )?)),
        "transport" => Some(SettingsChangeAction::Transport(parse_transport(new_value)?)),
        "http-idle-timeout" => HTTP_IDLE_TIMEOUT_CHOICES
            .iter()
            .find(|choice| choice.label == new_value)
            .map(|choice| SettingsChangeAction::HttpIdleTimeoutMs(choice.timeout_ms)),
        "thinking" => Some(SettingsChangeAction::ThinkingLevel(parse_thinking_level(
            new_value,
        )?)),
        "theme" => Some(SettingsChangeAction::Theme(new_value.to_string())),
        "hide-thinking" => Some(SettingsChangeAction::HideThinkingBlock(parse_bool(
            new_value,
        )?)),
        "collapse-changelog" => Some(SettingsChangeAction::CollapseChangelog(parse_bool(
            new_value,
        )?)),
        "quiet-startup" => Some(SettingsChangeAction::QuietStartup(parse_bool(new_value)?)),
        "install-telemetry" => Some(SettingsChangeAction::EnableInstallTelemetry(parse_bool(
            new_value,
        )?)),
        "double-escape-action" => Some(SettingsChangeAction::DoubleEscapeAction(
            parse_double_escape_action(new_value)?,
        )),
        "tree-filter-mode" => Some(SettingsChangeAction::TreeFilterMode(
            parse_tree_filter_mode(new_value)?,
        )),
        "show-hardware-cursor" => Some(SettingsChangeAction::ShowHardwareCursor(parse_bool(
            new_value,
        )?)),
        "editor-padding" => Some(SettingsChangeAction::EditorPaddingX(
            new_value.parse().ok()?,
        )),
        "autocomplete-max-visible" => Some(SettingsChangeAction::AutocompleteMaxVisible(
            new_value.parse().ok()?,
        )),
        "clear-on-shrink" => Some(SettingsChangeAction::ClearOnShrink(parse_bool(new_value)?)),
        "terminal-progress" => Some(SettingsChangeAction::ShowTerminalProgress(parse_bool(
            new_value,
        )?)),
        _ => None,
    }
}

pub struct WarningSettingsState {
    state: SettingsWarningSettings,
}

impl WarningSettingsState {
    pub fn new(state: SettingsWarningSettings) -> Self {
        Self { state }
    }

    pub fn item(&self) -> SettingsSelectorItem {
        item(
            "anthropic-extra-usage",
            "Anthropic extra usage",
            "Warn when Anthropic subscription auth may use paid extra usage",
            bool_value(self.state.anthropic_extra_usage.unwrap_or(true)),
            vec!["true", "false"],
        )
    }

    pub fn change(&mut self, new_value: &str) -> SettingsChangeAction {
        self.state.anthropic_extra_usage = Some(new_value == "true");
        SettingsChangeAction::Warnings(self.state.clone())
    }
}

impl SettingsSelectorItem {
    fn with_values(mut self, values: Vec<String>) -> Self {
        self.values = values;
        self
    }
}

fn item(
    id: &str,
    label: &str,
    description: &str,
    current_value: impl Into<String>,
    values: Vec<&str>,
) -> SettingsSelectorItem {
    SettingsSelectorItem {
        id: id.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        current_value: current_value.into(),
        values: values.into_iter().map(str::to_string).collect(),
        has_submenu: false,
    }
}

fn submenu_item(
    id: &str,
    label: &str,
    description: &str,
    current_value: impl Into<String>,
) -> SettingsSelectorItem {
    SettingsSelectorItem {
        id: id.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        current_value: current_value.into(),
        values: Vec::new(),
        has_submenu: true,
    }
}

fn insert_after(
    items: &mut Vec<SettingsSelectorItem>,
    target_id: &str,
    item: SettingsSelectorItem,
) {
    let index = items
        .iter()
        .position(|candidate| candidate.id == target_id)
        .map(|index| index + 1)
        .unwrap_or(items.len());
    items.insert(index, item);
}

fn bool_value(value: bool) -> String {
    value.to_string()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn message_mode_value(value: SettingsMessageMode) -> String {
    match value {
        SettingsMessageMode::All => "all",
        SettingsMessageMode::OneAtATime => "one-at-a-time",
    }
    .to_string()
}

fn parse_message_mode(value: &str) -> Option<SettingsMessageMode> {
    match value {
        "all" => Some(SettingsMessageMode::All),
        "one-at-a-time" => Some(SettingsMessageMode::OneAtATime),
        _ => None,
    }
}

fn transport_value(value: SettingsTransport) -> String {
    match value {
        SettingsTransport::Sse => "sse",
        SettingsTransport::WebSocket => "websocket",
        SettingsTransport::WebSocketCached => "websocket-cached",
        SettingsTransport::Auto => "auto",
    }
    .to_string()
}

fn parse_transport(value: &str) -> Option<SettingsTransport> {
    match value {
        "sse" => Some(SettingsTransport::Sse),
        "websocket" => Some(SettingsTransport::WebSocket),
        "websocket-cached" => Some(SettingsTransport::WebSocketCached),
        "auto" => Some(SettingsTransport::Auto),
        _ => None,
    }
}

fn thinking_level_value(value: ModelThinkingLevel) -> String {
    match value {
        ModelThinkingLevel::Off => "off",
        ModelThinkingLevel::Minimal => "minimal",
        ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High => "high",
        ModelThinkingLevel::XHigh => "xhigh",
    }
    .to_string()
}

fn parse_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn double_escape_value(value: SettingsDoubleEscapeAction) -> String {
    match value {
        SettingsDoubleEscapeAction::Fork => "fork",
        SettingsDoubleEscapeAction::Tree => "tree",
        SettingsDoubleEscapeAction::None => "none",
    }
    .to_string()
}

fn parse_double_escape_action(value: &str) -> Option<SettingsDoubleEscapeAction> {
    match value {
        "fork" => Some(SettingsDoubleEscapeAction::Fork),
        "tree" => Some(SettingsDoubleEscapeAction::Tree),
        "none" => Some(SettingsDoubleEscapeAction::None),
        _ => None,
    }
}

fn tree_filter_value(value: SettingsTreeFilterMode) -> String {
    match value {
        SettingsTreeFilterMode::Default => "default",
        SettingsTreeFilterMode::NoTools => "no-tools",
        SettingsTreeFilterMode::UserOnly => "user-only",
        SettingsTreeFilterMode::LabeledOnly => "labeled-only",
        SettingsTreeFilterMode::All => "all",
    }
    .to_string()
}

fn parse_tree_filter_mode(value: &str) -> Option<SettingsTreeFilterMode> {
    match value {
        "default" => Some(SettingsTreeFilterMode::Default),
        "no-tools" => Some(SettingsTreeFilterMode::NoTools),
        "user-only" => Some(SettingsTreeFilterMode::UserOnly),
        "labeled-only" => Some(SettingsTreeFilterMode::LabeledOnly),
        "all" => Some(SettingsTreeFilterMode::All),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ai::ModelThinkingLevel;

    use super::*;

    #[test]
    fn settings_selector_builds_pi_order_with_image_capability() {
        let config = sample_config();

        let with_images = build_settings_items(&config, SettingsCapabilities { images: true });
        let without_images = build_settings_items(&config, SettingsCapabilities { images: false });

        assert_eq!(
            ids(&with_images)[..12],
            [
                "autocompact",
                "show-images",
                "image-width-cells",
                "auto-resize-images",
                "block-images",
                "skill-commands",
                "show-hardware-cursor",
                "editor-padding",
                "autocomplete-max-visible",
                "clear-on-shrink",
                "terminal-progress",
                "steering-mode",
            ]
        );
        assert!(!ids(&without_images).contains(&"show-images"));
        assert!(!ids(&without_images).contains(&"image-width-cells"));
        assert_eq!(ids(&without_images)[1], "auto-resize-images");
    }

    #[test]
    fn settings_selector_items_include_timeout_thinking_theme_and_warnings_submenus() {
        let config = sample_config();
        let items = build_settings_items(&config, SettingsCapabilities { images: true });

        let timeout = item(&items, "http-idle-timeout");
        assert_eq!(timeout.current_value, "5 min");
        assert_eq!(
            timeout.values,
            vec!["30 sec", "1 min", "2 min", "5 min", "disabled"]
        );

        let thinking = item(&items, "thinking");
        assert!(thinking.has_submenu);
        assert_eq!(thinking.current_value, "medium");

        let theme = item(&items, "theme");
        assert!(theme.has_submenu);
        assert_eq!(theme.current_value, "dark");

        let warnings = item(&items, "warnings");
        assert!(warnings.has_submenu);
        assert_eq!(warnings.current_value, "configure");
    }

    #[test]
    fn settings_selector_change_action_maps_item_values_like_pi_callbacks() {
        assert_eq!(
            settings_change_action("autocompact", "false"),
            Some(SettingsChangeAction::AutoCompact(false))
        );
        assert_eq!(
            settings_change_action("image-width-cells", "120"),
            Some(SettingsChangeAction::ImageWidthCells(120))
        );
        assert_eq!(
            settings_change_action("transport", "websocket-cached"),
            Some(SettingsChangeAction::Transport(
                SettingsTransport::WebSocketCached
            ))
        );
        assert_eq!(
            settings_change_action("http-idle-timeout", "disabled"),
            Some(SettingsChangeAction::HttpIdleTimeoutMs(0))
        );
        assert_eq!(
            settings_change_action("thinking", "xhigh"),
            Some(SettingsChangeAction::ThinkingLevel(
                ModelThinkingLevel::XHigh
            ))
        );
        assert_eq!(
            settings_change_action("tree-filter-mode", "labeled-only"),
            Some(SettingsChangeAction::TreeFilterMode(
                SettingsTreeFilterMode::LabeledOnly
            ))
        );
        assert_eq!(settings_change_action("http-idle-timeout", "missing"), None);
    }

    #[test]
    fn warning_settings_submenu_defaults_true_and_updates_state() {
        let mut state = WarningSettingsState::new(SettingsWarningSettings {
            anthropic_extra_usage: None,
        });

        assert_eq!(state.item().current_value, "true");
        assert_eq!(
            state.change("false"),
            SettingsChangeAction::Warnings(SettingsWarningSettings {
                anthropic_extra_usage: Some(false)
            })
        );
        assert_eq!(state.item().current_value, "false");
    }

    fn sample_config() -> SettingsSelectorConfig {
        SettingsSelectorConfig {
            auto_compact: true,
            show_images: true,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            enable_skill_commands: true,
            steering_mode: SettingsMessageMode::OneAtATime,
            follow_up_mode: SettingsMessageMode::All,
            transport: SettingsTransport::Sse,
            http_idle_timeout_ms: 300_000,
            thinking_level: ModelThinkingLevel::Medium,
            available_thinking_levels: vec![
                ModelThinkingLevel::Off,
                ModelThinkingLevel::Medium,
                ModelThinkingLevel::XHigh,
            ],
            current_theme: "dark".to_string(),
            available_themes: vec!["dark".to_string(), "light".to_string()],
            hide_thinking_block: false,
            collapse_changelog: true,
            enable_install_telemetry: false,
            double_escape_action: SettingsDoubleEscapeAction::Tree,
            tree_filter_mode: SettingsTreeFilterMode::Default,
            show_hardware_cursor: true,
            editor_padding_x: 1,
            autocomplete_max_visible: 10,
            quiet_startup: false,
            clear_on_shrink: true,
            show_terminal_progress: false,
            warnings: SettingsWarningSettings {
                anthropic_extra_usage: Some(true),
            },
        }
    }

    fn ids(items: &[SettingsSelectorItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    fn item<'a>(items: &'a [SettingsSelectorItem], id: &str) -> &'a SettingsSelectorItem {
        items
            .iter()
            .find(|item| item.id == id)
            .expect("setting item")
    }
}
