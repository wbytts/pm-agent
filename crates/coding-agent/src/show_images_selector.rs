use crate::settings_manager::{SettingsManager, SettingsStorage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowImagesOption {
    pub value: bool,
    pub label: &'static str,
    pub description: &'static str,
}

pub fn show_images_options() -> Vec<ShowImagesOption> {
    vec![
        ShowImagesOption {
            value: true,
            label: "Yes",
            description: "Show images inline in terminal",
        },
        ShowImagesOption {
            value: false,
            label: "No",
            description: "Show text placeholder instead",
        },
    ]
}

pub fn show_images_selected_index(current_value: bool) -> usize {
    if current_value {
        0
    } else {
        1
    }
}

pub fn apply_show_images_selection<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    value: bool,
) {
    settings.set_show_images(value);
}

#[cfg(test)]
mod tests {
    use super::{
        apply_show_images_selection, show_images_options, show_images_selected_index,
        ShowImagesOption,
    };
    use crate::settings_manager::SettingsManager;

    #[test]
    fn show_images_selector_options_match_pi_component() {
        assert_eq!(
            show_images_options(),
            vec![
                ShowImagesOption {
                    value: true,
                    label: "Yes",
                    description: "Show images inline in terminal"
                },
                ShowImagesOption {
                    value: false,
                    label: "No",
                    description: "Show text placeholder instead"
                }
            ]
        );
        assert_eq!(show_images_selected_index(true), 0);
        assert_eq!(show_images_selected_index(false), 1);
    }

    #[test]
    fn show_images_selection_writes_global_terminal_setting() {
        let mut settings = SettingsManager::in_memory(serde_json::json!({}));

        apply_show_images_selection(&mut settings, false);

        assert!(!settings.get_show_images());
        assert_eq!(
            settings
                .global_settings()
                .terminal
                .as_ref()
                .and_then(|terminal| terminal.show_images),
            Some(false)
        );
    }
}
