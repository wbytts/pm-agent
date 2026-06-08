use tui::{components::Input, KeybindingsManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginDialogLine {
    Blank,
    Text(String),
    Accent(String),
    Dim(String),
    Warning(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginDialogAction {
    None,
    OpenUrl(String),
    SubmitInput(String),
    Cancel(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginAuthOptions {
    pub auto_open_browser: bool,
}

impl Default for LoginAuthOptions {
    fn default() -> Self {
        Self {
            auto_open_browser: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginDeviceCodeInfo {
    pub verification_uri: String,
    pub user_code: String,
}

pub struct LoginDialogState {
    title: String,
    content_lines: Vec<LoginDialogLine>,
    input: Input,
    waiting_for_input: bool,
    cancelled: bool,
}

impl LoginDialogState {
    pub fn new(
        provider_id: impl Into<String>,
        provider_name_override: Option<&str>,
        title_override: Option<&str>,
    ) -> Self {
        let provider_id = provider_id.into();
        let provider_name = provider_name_override.unwrap_or(&provider_id);
        let title = title_override
            .map(str::to_string)
            .unwrap_or_else(|| format!("Login to {provider_name}"));

        Self {
            title,
            content_lines: Vec::new(),
            input: Input::new(),
            waiting_for_input: false,
            cancelled: false,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn content_lines(&self) -> &[LoginDialogLine] {
        &self.content_lines
    }

    pub fn waiting_for_input(&self) -> bool {
        self.waiting_for_input
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn show_auth(
        &mut self,
        url: impl Into<String>,
        instructions: Option<&str>,
        options: LoginAuthOptions,
    ) -> LoginDialogAction {
        let url = url.into();
        self.content_lines.clear();
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .push(LoginDialogLine::Accent(url.clone()));
        self.content_lines
            .push(LoginDialogLine::Dim(click_hint(&url)));
        if let Some(instructions) = instructions {
            self.content_lines.push(LoginDialogLine::Blank);
            self.content_lines
                .push(LoginDialogLine::Warning(instructions.to_string()));
        }

        if options.auto_open_browser {
            LoginDialogAction::OpenUrl(url)
        } else {
            LoginDialogAction::None
        }
    }

    pub fn show_device_code(&mut self, info: LoginDeviceCodeInfo) -> LoginDialogAction {
        self.content_lines.clear();
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .push(LoginDialogLine::Accent(info.verification_uri.clone()));
        self.content_lines
            .push(LoginDialogLine::Dim(click_hint(&info.verification_uri)));
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines.push(LoginDialogLine::Warning(format!(
            "Enter code: {}",
            info.user_code
        )));
        LoginDialogAction::OpenUrl(info.verification_uri)
    }

    pub fn show_manual_input(&mut self, prompt: impl Into<String>) {
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines.push(LoginDialogLine::Dim(prompt.into()));
        self.content_lines
            .push(LoginDialogLine::Text("(escape to cancel)".to_string()));
        self.input.set_value("");
        self.waiting_for_input = true;
    }

    pub fn show_prompt(&mut self, message: impl Into<String>, placeholder: Option<&str>) {
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .push(LoginDialogLine::Text(message.into()));
        if let Some(placeholder) = placeholder {
            self.content_lines
                .push(LoginDialogLine::Dim(format!("e.g., {placeholder}")));
        }
        self.content_lines.push(LoginDialogLine::Text(
            "(escape to cancel, enter to submit)".to_string(),
        ));
        self.input.set_value("");
        self.waiting_for_input = true;
    }

    pub fn show_info(&mut self, lines: Vec<String>) {
        self.content_lines.clear();
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .extend(lines.into_iter().map(LoginDialogLine::Text));
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .push(LoginDialogLine::Text("(escape to close)".to_string()));
    }

    pub fn show_waiting(&mut self, message: impl Into<String>) {
        self.content_lines.push(LoginDialogLine::Blank);
        self.content_lines
            .push(LoginDialogLine::Dim(message.into()));
        self.content_lines
            .push(LoginDialogLine::Text("(escape to cancel)".to_string()));
    }

    pub fn show_progress(&mut self, message: impl Into<String>) {
        self.content_lines
            .push(LoginDialogLine::Dim(message.into()));
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> LoginDialogAction {
        if keybindings.matches(key_data, "tui.select.cancel") {
            self.cancelled = true;
            self.waiting_for_input = false;
            return LoginDialogAction::Cancel("Login cancelled".to_string());
        }

        if self.waiting_for_input
            && (keybindings.matches(key_data, "tui.select.confirm") || key_data == "\n")
        {
            self.waiting_for_input = false;
            return LoginDialogAction::SubmitInput(self.input.value().to_string());
        }

        self.input.handle_input(key_data, keybindings);
        LoginDialogAction::None
    }
}

pub fn click_hint(url: &str) -> String {
    let hint = if cfg!(target_os = "macos") {
        "Cmd+click to open"
    } else {
        "Ctrl+click to open"
    };
    format!("{hint}: {url}")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::keybindings::app_keybindings;
    use tui::KeybindingsManager;

    fn kb() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }

    #[test]
    fn login_dialog_show_auth_renders_link_hints_and_open_action() {
        let mut dialog = LoginDialogState::new("github-copilot", Some("GitHub Copilot"), None);

        let action = dialog.show_auth(
            "https://example.test/login",
            Some("Follow the browser flow"),
            LoginAuthOptions {
                auto_open_browser: true,
            },
        );

        assert_eq!(dialog.title(), "Login to GitHub Copilot");
        assert_eq!(
            action,
            LoginDialogAction::OpenUrl("https://example.test/login".to_string())
        );
        assert_eq!(
            dialog.content_lines(),
            &[
                LoginDialogLine::Blank,
                LoginDialogLine::Accent("https://example.test/login".to_string()),
                LoginDialogLine::Dim(click_hint("https://example.test/login")),
                LoginDialogLine::Blank,
                LoginDialogLine::Warning("Follow the browser flow".to_string()),
            ]
        );
    }

    #[test]
    fn login_dialog_device_code_renders_code_and_open_action() {
        let mut dialog = LoginDialogState::new("github", None, Some("Authorize GitHub"));

        let action = dialog.show_device_code(LoginDeviceCodeInfo {
            verification_uri: "https://github.com/login/device".to_string(),
            user_code: "ABCD-1234".to_string(),
        });

        assert_eq!(dialog.title(), "Authorize GitHub");
        assert_eq!(
            action,
            LoginDialogAction::OpenUrl("https://github.com/login/device".to_string())
        );
        assert!(dialog.content_lines().contains(&LoginDialogLine::Warning(
            "Enter code: ABCD-1234".to_string()
        )));
    }

    #[test]
    fn login_dialog_manual_input_appends_prompt_and_submits_input() {
        let mut dialog = LoginDialogState::new("openai", Some("OpenAI"), None);

        dialog.show_manual_input("Paste callback URL");
        let typing = dialog.handle_input("https://callback.test/?code=abc", &kb());
        let submit = dialog.handle_input("\r", &kb());

        assert_eq!(typing, LoginDialogAction::None);
        assert_eq!(
            submit,
            LoginDialogAction::SubmitInput("https://callback.test/?code=abc".to_string())
        );
        assert!(!dialog.waiting_for_input());
        assert!(dialog
            .content_lines()
            .contains(&LoginDialogLine::Dim("Paste callback URL".to_string())));
    }

    #[test]
    fn login_dialog_prompt_preserves_previous_content_and_resets_input() {
        let mut dialog = LoginDialogState::new("anthropic", Some("Anthropic"), None);
        dialog.show_auth("https://auth.test", None, LoginAuthOptions::default());
        dialog.show_manual_input("Old input");
        dialog.handle_input("stale", &kb());

        dialog.show_prompt("Enter code", Some("code-123"));
        let submit = dialog.handle_input("fresh", &kb());
        let submit = if submit == LoginDialogAction::None {
            dialog.handle_input("\r", &kb())
        } else {
            submit
        };

        assert_eq!(submit, LoginDialogAction::SubmitInput("fresh".to_string()));
        assert!(dialog
            .content_lines()
            .contains(&LoginDialogLine::Text("Enter code".to_string())));
        assert!(dialog
            .content_lines()
            .contains(&LoginDialogLine::Dim("e.g., code-123".to_string())));
        assert!(dialog
            .content_lines()
            .contains(&LoginDialogLine::Accent("https://auth.test".to_string())));
    }

    #[test]
    fn login_dialog_info_waiting_progress_and_cancel_match_pi_flow() {
        let mut dialog = LoginDialogState::new("provider", None, None);

        dialog.show_info(vec!["Line one".to_string(), "Line two".to_string()]);
        assert_eq!(
            dialog.content_lines(),
            &[
                LoginDialogLine::Blank,
                LoginDialogLine::Text("Line one".to_string()),
                LoginDialogLine::Text("Line two".to_string()),
                LoginDialogLine::Blank,
                LoginDialogLine::Text("(escape to close)".to_string()),
            ]
        );

        dialog.show_waiting("Waiting for confirmation");
        dialog.show_progress("Still waiting");
        assert!(dialog.content_lines().contains(&LoginDialogLine::Dim(
            "Waiting for confirmation".to_string()
        )));
        assert!(dialog
            .content_lines()
            .contains(&LoginDialogLine::Dim("Still waiting".to_string())));

        assert_eq!(
            dialog.handle_input("\x1b", &kb()),
            LoginDialogAction::Cancel("Login cancelled".to_string())
        );
        assert!(dialog.cancelled());
    }
}
