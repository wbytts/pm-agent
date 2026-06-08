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
