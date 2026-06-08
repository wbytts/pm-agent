use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{AiError, AiResult};

use super::anthropic::anthropic_oauth_provider;
use super::github_copilot::github_copilot_oauth_provider;
use super::openai_codex::openai_codex_oauth_provider;
use super::types::{OAuthCredentials, OAuthProviderId, OAuthProviderInfo, OAuthProviderInterface};

pub type SharedOAuthProvider = Arc<dyn OAuthProviderInterface>;

#[derive(Default)]
pub struct OAuthProviderRegistry {
    built_in_providers: BTreeMap<OAuthProviderId, SharedOAuthProvider>,
    providers: BTreeMap<OAuthProviderId, SharedOAuthProvider>,
}

impl OAuthProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::new();
        registry.register_built_in_provider(Arc::new(anthropic_oauth_provider()));
        registry.register_built_in_provider(Arc::new(github_copilot_oauth_provider()));
        registry.register_built_in_provider(Arc::new(openai_codex_oauth_provider()));
        registry
    }

    pub fn register_built_in_provider(&mut self, provider: SharedOAuthProvider) {
        let id = provider.id().to_string();
        self.built_in_providers
            .insert(id.clone(), Arc::clone(&provider));
        self.providers.insert(id, provider);
    }

    pub fn register_provider(&mut self, provider: SharedOAuthProvider) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    pub fn unregister_provider(&mut self, id: &str) {
        if let Some(built_in_provider) = self.built_in_providers.get(id) {
            self.providers
                .insert(id.to_string(), Arc::clone(built_in_provider));
        } else {
            self.providers.remove(id);
        }
    }

    pub fn reset_providers(&mut self) {
        self.providers.clear();
        for (id, provider) in &self.built_in_providers {
            self.providers.insert(id.clone(), Arc::clone(provider));
        }
    }

    pub fn get_provider(&self, id: &str) -> Option<SharedOAuthProvider> {
        self.providers.get(id).cloned()
    }

    pub fn get_providers(&self) -> Vec<SharedOAuthProvider> {
        self.providers.values().cloned().collect()
    }

    pub fn get_provider_info_list(&self) -> Vec<OAuthProviderInfo> {
        self.get_providers()
            .into_iter()
            .map(|provider| OAuthProviderInfo {
                id: provider.id().to_string(),
                name: provider.name().to_string(),
                available: true,
            })
            .collect()
    }

    pub fn refresh_oauth_token(
        &self,
        provider_id: &str,
        credentials: &OAuthCredentials,
    ) -> AiResult<OAuthCredentials> {
        let provider = self
            .get_provider(provider_id)
            .ok_or_else(|| unknown_provider_error(provider_id))?;
        provider.refresh_token(credentials)
    }

    pub fn get_oauth_api_key(
        &self,
        provider_id: &str,
        credentials: &BTreeMap<String, OAuthCredentials>,
        now_millis: u128,
    ) -> AiResult<Option<OAuthApiKeyResult>> {
        let provider = self
            .get_provider(provider_id)
            .ok_or_else(|| unknown_provider_error(provider_id))?;
        let Some(credentials) = credentials.get(provider_id) else {
            return Ok(None);
        };

        let credentials = if now_millis >= credentials.expires {
            provider.refresh_token(credentials).map_err(|_| {
                AiError::InvalidResponse(format!("Failed to refresh OAuth token for {provider_id}"))
            })?
        } else {
            credentials.clone()
        };
        let api_key = provider.get_api_key(&credentials);
        Ok(Some(OAuthApiKeyResult {
            new_credentials: credentials,
            api_key,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthApiKeyResult {
    pub new_credentials: OAuthCredentials,
    pub api_key: String,
}

pub fn register_built_in_oauth_provider(provider: SharedOAuthProvider) {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .register_built_in_provider(provider);
}

pub fn register_oauth_provider(provider: SharedOAuthProvider) {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .register_provider(provider);
}

pub fn unregister_oauth_provider(id: &str) {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .unregister_provider(id);
}

pub fn reset_oauth_providers() {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .reset_providers();
}

pub fn get_oauth_provider(id: &str) -> Option<SharedOAuthProvider> {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .get_provider(id)
}

pub fn get_oauth_providers() -> Vec<SharedOAuthProvider> {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .get_providers()
}

pub fn get_oauth_provider_info_list() -> Vec<OAuthProviderInfo> {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .get_provider_info_list()
}

pub fn refresh_oauth_token(
    provider_id: &str,
    credentials: &OAuthCredentials,
) -> AiResult<OAuthCredentials> {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .refresh_oauth_token(provider_id, credentials)
}

pub fn get_oauth_api_key(
    provider_id: &str,
    credentials: &BTreeMap<String, OAuthCredentials>,
) -> AiResult<Option<OAuthApiKeyResult>> {
    global_registry()
        .lock()
        .expect("oauth provider registry poisoned")
        .get_oauth_api_key(provider_id, credentials, current_time_millis())
}

fn global_registry() -> &'static Mutex<OAuthProviderRegistry> {
    static GLOBAL_REGISTRY: OnceLock<Mutex<OAuthProviderRegistry>> = OnceLock::new();
    GLOBAL_REGISTRY.get_or_init(|| Mutex::new(OAuthProviderRegistry::builtins()))
}

fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn unknown_provider_error(provider_id: &str) -> AiError {
    AiError::InvalidResponse(format!("Unknown OAuth provider: {provider_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::oauth::anthropic::ANTHROPIC_OAUTH_PROVIDER_ID;
    use crate::utils::oauth::github_copilot::GITHUB_COPILOT_PROVIDER_ID;
    use crate::utils::oauth::openai_codex::OPENAI_CODEX_PROVIDER_ID;
    use crate::utils::oauth::types::OAuthLoginCallbacks;
    use crate::{AiResult, Model};

    struct TestProvider {
        id: &'static str,
        name: &'static str,
        refreshed_access: &'static str,
    }

    impl OAuthProviderInterface for TestProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> &str {
            self.name
        }

        fn login(&self, _callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
            Ok(credentials("refresh", "access", 1000))
        }

        fn refresh_token(&self, credentials: &OAuthCredentials) -> AiResult<OAuthCredentials> {
            let mut next = credentials.clone();
            next.access = self.refreshed_access.to_string();
            next.expires = 10_000;
            Ok(next)
        }

        fn get_api_key(&self, credentials: &OAuthCredentials) -> String {
            credentials.access.clone()
        }

        fn modify_models(&self, models: Vec<Model>, _credentials: &OAuthCredentials) -> Vec<Model> {
            models
        }
    }

    #[test]
    fn registers_and_lists_provider_info() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_provider(Arc::new(TestProvider {
            id: "test",
            name: "Test",
            refreshed_access: "new-access",
        }));

        assert_eq!(
            registry.get_provider_info_list(),
            vec![OAuthProviderInfo {
                id: "test".to_string(),
                name: "Test".to_string(),
                available: true,
            }]
        );
    }

    #[test]
    fn unregistering_builtin_restores_original_provider() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_built_in_provider(Arc::new(TestProvider {
            id: "test",
            name: "Built-in",
            refreshed_access: "built-in-access",
        }));
        registry.register_provider(Arc::new(TestProvider {
            id: "test",
            name: "Custom",
            refreshed_access: "custom-access",
        }));

        registry.unregister_provider("test");

        let provider = registry.get_provider("test").expect("provider");
        assert_eq!(provider.name(), "Built-in");
    }

    #[test]
    fn builtins_include_pi_oauth_providers() {
        let registry = OAuthProviderRegistry::builtins();

        let anthropic = registry
            .get_provider(ANTHROPIC_OAUTH_PROVIDER_ID)
            .expect("anthropic provider");
        let github_copilot = registry
            .get_provider(GITHUB_COPILOT_PROVIDER_ID)
            .expect("github copilot provider");
        let openai_codex = registry
            .get_provider(OPENAI_CODEX_PROVIDER_ID)
            .expect("openai codex provider");

        assert_eq!(anthropic.name(), "Anthropic (Claude Pro/Max)");
        assert_eq!(github_copilot.name(), "GitHub Copilot");
        assert_eq!(openai_codex.name(), "ChatGPT Plus/Pro (Codex Subscription)");
    }

    #[test]
    fn global_registry_reset_restores_builtin_providers() {
        register_oauth_provider(Arc::new(TestProvider {
            id: ANTHROPIC_OAUTH_PROVIDER_ID,
            name: "Custom Anthropic",
            refreshed_access: "custom-access",
        }));

        unregister_oauth_provider(ANTHROPIC_OAUTH_PROVIDER_ID);

        let provider = get_oauth_provider(ANTHROPIC_OAUTH_PROVIDER_ID).expect("provider");
        assert_eq!(provider.name(), "Anthropic (Claude Pro/Max)");

        reset_oauth_providers();
    }

    #[test]
    fn refreshes_expired_credentials_before_returning_api_key() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_provider(Arc::new(TestProvider {
            id: "test",
            name: "Test",
            refreshed_access: "new-access",
        }));
        let credentials = BTreeMap::from([(
            "test".to_string(),
            credentials("refresh", "old-access", 1000),
        )]);

        let result = registry
            .get_oauth_api_key("test", &credentials, 2000)
            .expect("api key")
            .expect("credentials");

        assert_eq!(result.api_key, "new-access");
        assert_eq!(result.new_credentials.access, "new-access");
    }

    #[test]
    fn keeps_valid_credentials_without_refresh() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_provider(Arc::new(TestProvider {
            id: "test",
            name: "Test",
            refreshed_access: "new-access",
        }));
        let credentials = BTreeMap::from([(
            "test".to_string(),
            credentials("refresh", "valid-access", 3000),
        )]);

        let result = registry
            .get_oauth_api_key("test", &credentials, 2000)
            .expect("api key")
            .expect("credentials");

        assert_eq!(result.api_key, "valid-access");
        assert_eq!(result.new_credentials.access, "valid-access");
    }

    #[test]
    fn returns_none_when_credentials_are_missing() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_provider(Arc::new(TestProvider {
            id: "test",
            name: "Test",
            refreshed_access: "new-access",
        }));

        let result = registry
            .get_oauth_api_key("test", &BTreeMap::new(), 2000)
            .expect("api key");

        assert!(result.is_none());
    }

    #[test]
    fn reports_unknown_provider() {
        let registry = OAuthProviderRegistry::new();

        let error = registry
            .refresh_oauth_token("missing", &credentials("refresh", "access", 1000))
            .expect_err("unknown provider");

        assert!(error
            .to_string()
            .contains("Unknown OAuth provider: missing"));
    }

    fn credentials(refresh: &str, access: &str, expires: u128) -> OAuthCredentials {
        OAuthCredentials {
            refresh: refresh.to_string(),
            access: access.to_string(),
            expires,
            extra: BTreeMap::new(),
        }
    }
}
