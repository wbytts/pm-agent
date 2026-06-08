use tui::components::Input;
use tui::{fuzzy_filter, KeybindingsManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSelectorMode {
    Login,
    Logout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSelectorCredentialType {
    OAuth,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSelectorProvider {
    pub id: String,
    pub name: String,
    pub auth_type: AuthSelectorCredentialType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatusSource {
    Environment,
    Runtime,
    Fallback,
    ModelsJsonKey,
    ModelsJsonCommand,
    None,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthSelectorStatus {
    pub source: Option<AuthStatusSource>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthSelectorAction {
    None,
    Select(String),
    Cancel,
}

pub struct OAuthSelectorState {
    mode: AuthSelectorMode,
    all_providers: Vec<AuthSelectorProvider>,
    filtered_providers: Vec<AuthSelectorProvider>,
    selected_index: usize,
    search_input: Input,
}

impl OAuthSelectorState {
    pub fn new(mode: AuthSelectorMode, providers: Vec<AuthSelectorProvider>) -> Self {
        Self {
            mode,
            filtered_providers: providers.clone(),
            all_providers: providers,
            selected_index: 0,
            search_input: Input::new(),
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn query(&self) -> &str {
        self.search_input.value()
    }

    pub fn filtered_providers(&self) -> &[AuthSelectorProvider] {
        &self.filtered_providers
    }

    pub fn selected_provider(&self) -> Option<&AuthSelectorProvider> {
        self.filtered_providers.get(self.selected_index)
    }

    pub fn set_query(&mut self, query: &str) {
        self.search_input.set_value(query);
        self.filter_providers(query);
    }

    pub fn move_selection(&mut self, direction: isize) {
        if self.filtered_providers.is_empty() || direction == 0 {
            return;
        }
        if direction < 0 {
            self.selected_index = self.selected_index.saturating_sub(1);
        } else {
            self.selected_index = (self.selected_index + 1).min(self.filtered_providers.len() - 1);
        }
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> OAuthSelectorAction {
        if keybindings.matches(key_data, "tui.select.up") {
            self.move_selection(-1);
            OAuthSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.down") {
            self.move_selection(1);
            OAuthSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.confirm") {
            self.selected_provider()
                .map(|provider| OAuthSelectorAction::Select(provider.id.clone()))
                .unwrap_or(OAuthSelectorAction::None)
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            OAuthSelectorAction::Cancel
        } else {
            self.search_input.handle_input(key_data, keybindings);
            let query = self.search_input.value().to_string();
            self.filter_providers(&query);
            OAuthSelectorAction::None
        }
    }

    pub fn empty_message(&self) -> Option<&'static str> {
        if !self.filtered_providers.is_empty() {
            return None;
        }
        Some(if self.all_providers.is_empty() {
            match self.mode {
                AuthSelectorMode::Login => "No providers available",
                AuthSelectorMode::Logout => "No providers logged in. Use /login first.",
            }
        } else {
            "No matching providers"
        })
    }

    pub fn status_indicator(
        provider: &AuthSelectorProvider,
        credential: Option<AuthSelectorCredentialType>,
        status: AuthSelectorStatus,
    ) -> String {
        if credential == Some(provider.auth_type) {
            return " ✓ configured".to_string();
        }
        if let Some(credential) = credential {
            let label = match credential {
                AuthSelectorCredentialType::OAuth => "subscription configured",
                AuthSelectorCredentialType::ApiKey => "API key configured",
            };
            return format!(" • {label}");
        }
        if provider.auth_type != AuthSelectorCredentialType::ApiKey {
            return " • unconfigured".to_string();
        }

        match status.source.unwrap_or(AuthStatusSource::None) {
            AuthStatusSource::Environment => {
                format!(
                    " ✓ env: {}",
                    status.label.unwrap_or_else(|| "API key".to_string())
                )
            }
            AuthStatusSource::Runtime => " ✓ runtime API key".to_string(),
            AuthStatusSource::Fallback => " ✓ custom API key".to_string(),
            AuthStatusSource::ModelsJsonKey => " ✓ key in models.json".to_string(),
            AuthStatusSource::ModelsJsonCommand => " ✓ command in models.json".to_string(),
            AuthStatusSource::None => " • unconfigured".to_string(),
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_index = self
            .selected_index
            .min(self.filtered_providers.len().saturating_sub(1));
    }

    fn filter_providers(&mut self, query: &str) {
        self.filtered_providers = if query.is_empty() {
            self.all_providers.clone()
        } else {
            fuzzy_filter(&self.all_providers, query, |provider| {
                format!(
                    "{} {} {}",
                    provider.name,
                    provider.id,
                    provider.auth_type.as_pi_key()
                )
            })
        };
        self.clamp_selection();
    }
}

impl AuthSelectorCredentialType {
    fn as_pi_key(self) -> &'static str {
        match self {
            AuthSelectorCredentialType::OAuth => "oauth",
            AuthSelectorCredentialType::ApiKey => "api_key",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuthSelectorCredentialType, AuthSelectorMode, AuthSelectorProvider, AuthSelectorStatus,
        AuthStatusSource, OAuthSelectorAction, OAuthSelectorState,
    };
    use crate::keybindings::app_keybindings;
    use std::collections::BTreeMap;
    use tui::KeybindingsManager;

    #[test]
    fn oauth_selector_filters_providers_with_fuzzy_text_and_clamps_selection() {
        let mut state = state(
            AuthSelectorMode::Login,
            vec![
                provider("anthropic", "Anthropic", "oauth"),
                provider("openai", "OpenAI", "api_key"),
            ],
        );

        state.move_selection(1);
        state.set_query("api open");

        assert_eq!(state.selected_index(), 0);
        assert_eq!(
            state
                .filtered_providers()
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            vec!["openai"]
        );
    }

    #[test]
    fn oauth_selector_handles_navigation_select_cancel_and_text_input_like_pi() {
        let mut state = state(
            AuthSelectorMode::Login,
            vec![
                provider("a", "Alpha", "oauth"),
                provider("b", "Beta", "api_key"),
            ],
        );
        let keybindings = keybindings();

        assert_eq!(
            state.handle_input("\x1b[B", &keybindings),
            OAuthSelectorAction::None
        );
        assert_eq!(
            state
                .selected_provider()
                .map(|provider| provider.id.as_str()),
            Some("b")
        );
        assert_eq!(
            state.handle_input("\r", &keybindings),
            OAuthSelectorAction::Select("b".to_string())
        );
        assert_eq!(
            state.handle_input("\x1b", &keybindings),
            OAuthSelectorAction::Cancel
        );

        state.handle_input("alp", &keybindings);
        assert_eq!(state.query(), "alp");
        assert_eq!(
            state
                .selected_provider()
                .map(|provider| provider.id.as_str()),
            Some("a")
        );
    }

    #[test]
    fn oauth_selector_delegates_query_editing_to_tui_input() {
        let mut state = state(
            AuthSelectorMode::Login,
            vec![
                provider("alpha", "Alpha", "oauth"),
                provider("beta", "Beta", "api_key"),
            ],
        );
        let keybindings = keybindings();

        state.handle_input("alp", &keybindings);
        state.handle_input("\x7f", &keybindings);

        assert_eq!(state.query(), "al");
        assert_eq!(
            state
                .filtered_providers()
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha"]
        );
    }

    #[test]
    fn oauth_selector_empty_messages_match_mode_and_filter_state() {
        let empty_login = state(AuthSelectorMode::Login, Vec::new());
        let empty_logout = state(AuthSelectorMode::Logout, Vec::new());
        let mut no_match = state(
            AuthSelectorMode::Login,
            vec![provider("a", "Alpha", "oauth")],
        );
        no_match.set_query("zzz");

        assert_eq!(empty_login.empty_message(), Some("No providers available"));
        assert_eq!(
            empty_logout.empty_message(),
            Some("No providers logged in. Use /login first.")
        );
        assert_eq!(no_match.empty_message(), Some("No matching providers"));
    }

    #[test]
    fn oauth_selector_status_indicators_match_pi_messages() {
        let oauth = provider("anthropic", "Anthropic", "oauth");
        let api_key = provider("openai", "OpenAI", "api_key");

        assert_eq!(
            OAuthSelectorState::status_indicator(
                &oauth,
                Some(AuthSelectorCredentialType::OAuth),
                AuthSelectorStatus::default()
            ),
            " ✓ configured"
        );
        assert_eq!(
            OAuthSelectorState::status_indicator(
                &oauth,
                Some(AuthSelectorCredentialType::ApiKey),
                AuthSelectorStatus::default()
            ),
            " • API key configured"
        );
        assert_eq!(
            OAuthSelectorState::status_indicator(
                &api_key,
                None,
                AuthSelectorStatus {
                    source: Some(AuthStatusSource::Environment),
                    label: Some("OPENAI_API_KEY".to_string()),
                }
            ),
            " ✓ env: OPENAI_API_KEY"
        );
        assert_eq!(
            OAuthSelectorState::status_indicator(&oauth, None, AuthSelectorStatus::default()),
            " • unconfigured"
        );
    }

    fn state(mode: AuthSelectorMode, providers: Vec<AuthSelectorProvider>) -> OAuthSelectorState {
        OAuthSelectorState::new(mode, providers)
    }

    fn provider(id: &str, name: &str, auth_type: &str) -> AuthSelectorProvider {
        AuthSelectorProvider {
            id: id.to_string(),
            name: name.to_string(),
            auth_type: match auth_type {
                "oauth" => AuthSelectorCredentialType::OAuth,
                "api_key" => AuthSelectorCredentialType::ApiKey,
                _ => panic!("unknown auth type"),
            },
        }
    }

    fn keybindings() -> KeybindingsManager {
        KeybindingsManager::new(app_keybindings(), BTreeMap::new())
    }
}
