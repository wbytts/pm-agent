use crate::settings_manager::{SettingsManager, SettingsStorage};
use ai::ModelThinkingLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingOption {
    pub value: ModelThinkingLevel,
    pub label: &'static str,
    pub description: &'static str,
}

pub fn thinking_options(available_levels: &[ModelThinkingLevel]) -> Vec<ThinkingOption> {
    available_levels
        .iter()
        .copied()
        .map(|level| ThinkingOption {
            value: level,
            label: thinking_level_key(level),
            description: thinking_level_description(level),
        })
        .collect()
}

pub fn thinking_selected_index(
    current_level: ModelThinkingLevel,
    available_levels: &[ModelThinkingLevel],
) -> Option<usize> {
    available_levels
        .iter()
        .position(|level| *level == current_level)
}

pub fn apply_thinking_selection<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    level: ModelThinkingLevel,
) {
    settings.set_default_thinking_level(thinking_level_key(level).to_string());
}

pub fn thinking_level_key(level: ModelThinkingLevel) -> &'static str {
    match level {
        ModelThinkingLevel::Off => "off",
        ModelThinkingLevel::Minimal => "minimal",
        ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High => "high",
        ModelThinkingLevel::XHigh => "xhigh",
    }
}

pub fn thinking_level_description(level: ModelThinkingLevel) -> &'static str {
    match level {
        ModelThinkingLevel::Off => "No reasoning",
        ModelThinkingLevel::Minimal => "Very brief reasoning (~1k tokens)",
        ModelThinkingLevel::Low => "Light reasoning (~2k tokens)",
        ModelThinkingLevel::Medium => "Moderate reasoning (~8k tokens)",
        ModelThinkingLevel::High => "Deep reasoning (~16k tokens)",
        ModelThinkingLevel::XHigh => "Maximum reasoning (~32k tokens)",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_thinking_selection, thinking_level_description, thinking_level_key, thinking_options,
        thinking_selected_index, ThinkingOption,
    };
    use crate::settings_manager::SettingsManager;
    use ai::ModelThinkingLevel;

    #[test]
    fn thinking_selector_options_match_pi_component() {
        let levels = [
            ModelThinkingLevel::Off,
            ModelThinkingLevel::Medium,
            ModelThinkingLevel::XHigh,
        ];

        assert_eq!(
            thinking_options(&levels),
            vec![
                ThinkingOption {
                    value: ModelThinkingLevel::Off,
                    label: "off",
                    description: "No reasoning"
                },
                ThinkingOption {
                    value: ModelThinkingLevel::Medium,
                    label: "medium",
                    description: "Moderate reasoning (~8k tokens)"
                },
                ThinkingOption {
                    value: ModelThinkingLevel::XHigh,
                    label: "xhigh",
                    description: "Maximum reasoning (~32k tokens)"
                }
            ]
        );
        assert_eq!(
            thinking_selected_index(ModelThinkingLevel::Medium, &levels),
            Some(1)
        );
        assert_eq!(
            thinking_selected_index(ModelThinkingLevel::Low, &levels),
            None
        );
    }

    #[test]
    fn thinking_level_keys_and_descriptions_cover_all_pi_levels() {
        let cases = [
            (ModelThinkingLevel::Off, "off", "No reasoning"),
            (
                ModelThinkingLevel::Minimal,
                "minimal",
                "Very brief reasoning (~1k tokens)",
            ),
            (
                ModelThinkingLevel::Low,
                "low",
                "Light reasoning (~2k tokens)",
            ),
            (
                ModelThinkingLevel::Medium,
                "medium",
                "Moderate reasoning (~8k tokens)",
            ),
            (
                ModelThinkingLevel::High,
                "high",
                "Deep reasoning (~16k tokens)",
            ),
            (
                ModelThinkingLevel::XHigh,
                "xhigh",
                "Maximum reasoning (~32k tokens)",
            ),
        ];

        for (level, key, description) in cases {
            assert_eq!(thinking_level_key(level), key);
            assert_eq!(thinking_level_description(level), description);
        }
    }

    #[test]
    fn thinking_selection_writes_default_thinking_level_setting() {
        let mut settings = SettingsManager::in_memory(serde_json::json!({}));

        apply_thinking_selection(&mut settings, ModelThinkingLevel::XHigh);

        assert_eq!(
            settings.get_default_thinking_level().as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            settings.global_settings().default_thinking_level.as_deref(),
            Some("xhigh")
        );
    }
}
