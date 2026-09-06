use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTheme {
    pub name: String,
    pub text: String,
    pub muted: String,
    pub dim: String,
    pub accent: String,
    pub page_bg: String,
    pub card_bg: String,
    pub info_bg: String,
    pub user_bg: String,
    pub assistant_bg: String,
    pub code_bg: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeExportColors {
    pub page_bg: Option<String>,
    pub card_bg: Option<String>,
    pub info_bg: Option<String>,
}

impl ExportTheme {
    pub fn resolve(name: Option<&str>) -> Self {
        match name.unwrap_or("dark") {
            "light" => Self::light(),
            _ => Self::dark(),
        }
    }

    pub fn css_vars(&self) -> String {
        [
            ("text", &self.text),
            ("muted", &self.muted),
            ("dim", &self.dim),
            ("accent", &self.accent),
            ("exportPageBg", &self.page_bg),
            ("exportCardBg", &self.card_bg),
            ("exportInfoBg", &self.info_bg),
            ("userMessageBg", &self.user_bg),
            ("assistantMessageBg", &self.assistant_bg),
            ("codeBg", &self.code_bg),
        ]
        .into_iter()
        .map(|(key, value)| format!("--{key}: {value};"))
        .collect::<Vec<_>>()
        .join("\n      ")
    }

    fn dark() -> Self {
        Self {
            name: "dark".to_string(),
            text: "#e7e7ea".to_string(),
            muted: "#9ca3af".to_string(),
            dim: "#3f4652".to_string(),
            accent: "#6ea8fe".to_string(),
            page_bg: "#15171c".to_string(),
            card_bg: "#1f232b".to_string(),
            info_bg: "#252b35".to_string(),
            user_bg: "#263244".to_string(),
            assistant_bg: "#20242c".to_string(),
            code_bg: "#111318".to_string(),
        }
    }

    fn light() -> Self {
        Self {
            name: "light".to_string(),
            text: "#1d2430".to_string(),
            muted: "#687386".to_string(),
            dim: "#d6dbe4".to_string(),
            accent: "#2563eb".to_string(),
            page_bg: "#f4f6fa".to_string(),
            card_bg: "#ffffff".to_string(),
            info_bg: "#eef2f7".to_string(),
            user_bg: "#e9f0ff".to_string(),
            assistant_bg: "#ffffff".to_string(),
            code_bg: "#f0f3f8".to_string(),
        }
    }
}

pub fn theme_export_colors_from_json(theme: &Value) -> ThemeExportColors {
    let Some(export) = theme.get("export").and_then(Value::as_object) else {
        return ThemeExportColors::default();
    };
    ThemeExportColors {
        page_bg: resolve_export_color(export.get("pageBg"), theme),
        card_bg: resolve_export_color(export.get("cardBg"), theme),
        info_bg: resolve_export_color(export.get("infoBg"), theme),
    }
}

fn resolve_export_color(value: Option<&Value>, theme: &Value) -> Option<String> {
    match resolve_theme_value(value?, theme, 0)? {
        ThemeValue::Ansi256(index) => Some(ansi256_to_hex(index)),
        ThemeValue::Text(value) if value.is_empty() => None,
        ThemeValue::Text(value) => Some(value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ThemeValue {
    Text(String),
    Ansi256(u8),
}

fn resolve_theme_value(value: &Value, theme: &Value, depth: usize) -> Option<ThemeValue> {
    if depth > 32 {
        return None;
    }
    if let Some(index) = value.as_u64().and_then(|value| u8::try_from(value).ok()) {
        return Some(ThemeValue::Ansi256(index));
    }
    let text = value.as_str()?;
    let Some(referenced) = theme
        .get("vars")
        .and_then(Value::as_object)
        .and_then(|vars| vars.get(text))
    else {
        return Some(ThemeValue::Text(text.to_string()));
    };
    resolve_theme_value(referenced, theme, depth + 1)
}

fn ansi256_to_hex(index: u8) -> String {
    const BASIC: [&str; 16] = [
        "#000000", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0",
        "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
    ];
    if index < 16 {
        return BASIC[index as usize].to_string();
    }
    if index < 232 {
        let cube_index = index - 16;
        let r = cube_index / 36;
        let g = (cube_index % 36) / 6;
        let b = cube_index % 6;
        return format!(
            "#{:02x}{:02x}{:02x}",
            ansi_cube_channel(r),
            ansi_cube_channel(g),
            ansi_cube_channel(b)
        );
    }
    let gray = 8 + (index - 232) * 10;
    format!("#{gray:02x}{gray:02x}{gray:02x}")
}

fn ansi_cube_channel(index: u8) -> u8 {
    if index == 0 {
        0
    } else {
        55 + index * 40
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_export_variable_references_like_pi() {
        let theme = json!({
            "name": "custom-export-vars",
            "vars": {
                "pageBgVar": "#112233",
                "pageBgAlias": "pageBgVar",
                "infoBgVar": "#445566",
                "cardBgVar": "#223344"
            },
            "export": {
                "pageBg": "pageBgAlias",
                "cardBg": "cardBgVar",
                "infoBg": "infoBgVar"
            }
        });

        assert_eq!(
            theme_export_colors_from_json(&theme),
            ThemeExportColors {
                page_bg: Some("#112233".to_string()),
                card_bg: Some("#223344".to_string()),
                info_bg: Some("#445566".to_string()),
            }
        );
    }

    #[test]
    fn resolves_recursive_vars_and_ansi256_export_values_like_pi() {
        let theme = json!({
            "name": "custom-export-recursive",
            "vars": {
                "deepPageBg": "#abcdef",
                "pageBgAlias": "deepPageBg",
                "cardBgAnsi": 24
            },
            "export": {
                "pageBg": "pageBgAlias",
                "cardBg": "cardBgAnsi",
                "infoBg": ""
            }
        });

        assert_eq!(
            theme_export_colors_from_json(&theme),
            ThemeExportColors {
                page_bg: Some("#abcdef".to_string()),
                card_bg: Some("#005f87".to_string()),
                info_bg: None,
            }
        );
    }

    #[test]
    fn keeps_literal_export_hex_values_like_pi() {
        let theme = json!({
            "name": "custom-export-literal",
            "export": {
                "pageBg": "#010203"
            }
        });

        assert_eq!(
            theme_export_colors_from_json(&theme).page_bg,
            Some("#010203".to_string())
        );
    }
}
