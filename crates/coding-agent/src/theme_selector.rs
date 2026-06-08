use crate::resource_loader::Theme;
use crate::settings_manager::{SettingsManager, SettingsStorage};
use std::collections::BTreeSet;

const BUILTIN_THEME_NAMES: [&str; 2] = ["dark", "light"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

pub fn theme_options(current_theme: &str, registered_themes: &[Theme]) -> Vec<ThemeOption> {
    available_theme_names(registered_themes)
        .into_iter()
        .map(|name| ThemeOption {
            description: (name == current_theme).then(|| "(current)".to_string()),
            label: name.clone(),
            value: name,
        })
        .collect()
}

pub fn available_theme_names(registered_themes: &[Theme]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for name in BUILTIN_THEME_NAMES {
        names.insert(name.to_string());
    }
    for theme in registered_themes {
        if !theme.name.is_empty() {
            names.insert(theme.name.clone());
        }
    }
    names.into_iter().collect()
}

pub fn theme_selected_index(current_theme: &str, registered_themes: &[Theme]) -> Option<usize> {
    available_theme_names(registered_themes)
        .iter()
        .position(|name| name == current_theme)
}

pub fn apply_theme_selection<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    theme_name: impl Into<String>,
) {
    settings.set_theme(theme_name.into());
}

#[cfg(test)]
mod tests {
    use super::{
        apply_theme_selection, available_theme_names, theme_options, theme_selected_index,
        ThemeOption,
    };
    use crate::resource_loader::Theme;
    use crate::settings_manager::SettingsManager;
    use serde_json::json;

    #[test]
    fn theme_selector_options_merge_builtin_and_registered_themes_like_pi() {
        let themes = vec![
            theme("solarized"),
            theme("dark"),
            theme("work"),
            theme("solarized"),
        ];

        assert_eq!(
            available_theme_names(&themes),
            vec![
                "dark".to_string(),
                "light".to_string(),
                "solarized".to_string(),
                "work".to_string()
            ]
        );
        assert_eq!(
            theme_options("solarized", &themes),
            vec![
                ThemeOption {
                    value: "dark".to_string(),
                    label: "dark".to_string(),
                    description: None,
                },
                ThemeOption {
                    value: "light".to_string(),
                    label: "light".to_string(),
                    description: None,
                },
                ThemeOption {
                    value: "solarized".to_string(),
                    label: "solarized".to_string(),
                    description: Some("(current)".to_string()),
                },
                ThemeOption {
                    value: "work".to_string(),
                    label: "work".to_string(),
                    description: None,
                },
            ]
        );
    }

    #[test]
    fn theme_selector_preselects_current_theme_when_available() {
        let themes = vec![theme("work")];

        assert_eq!(theme_selected_index("light", &themes), Some(1));
        assert_eq!(theme_selected_index("work", &themes), Some(2));
        assert_eq!(theme_selected_index("missing", &themes), None);
    }

    #[test]
    fn theme_selection_writes_global_theme_setting() {
        let mut settings = SettingsManager::in_memory(json!({}));

        apply_theme_selection(&mut settings, "light");

        assert_eq!(settings.get_theme().as_deref(), Some("light"));
        assert_eq!(settings.global_settings().theme.as_deref(), Some("light"));
    }

    fn theme(name: &str) -> Theme {
        Theme {
            name: name.to_string(),
            path: format!("/themes/{name}.json"),
            content: json!({ "name": name }),
            source_info: None,
        }
    }
}
