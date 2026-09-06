use crate::anthropic_subscription_warning::{
    should_warn_about_anthropic_subscription_auth, AnthropicCredentialKind,
    AnthropicSubscriptionWarningInput,
};
use crate::auth_storage::{
    AuthCredential, AuthSource, AuthStatus, AuthStorage, AuthStorageBackend,
};
use crate::model_resolver::CodingModelRegistry;
use crate::provider_display_names::built_in_provider_display_name;
use crate::resolve_config_value::{
    clear_config_value_cache, resolve_config_value_or_throw, resolve_config_value_uncached,
    resolve_headers_or_throw,
};
use crate::settings_manager::WarningSettings;
use ai::utils::{
    get_oauth_provider, register_oauth_provider, unregister_oauth_provider, OAuthCredentials,
    OAuthLoginCallbacks, OAuthProviderInterface,
};
use ai::{
    Model, ModelRegistry as AiModelRegistry, ProviderRegistry, RegisteredDynamicProvider,
    RegisteredProvider, StreamEvent,
};
use config::{
    CustomModelsResult, ModelsConfig, OAuthProviderConfig, ProviderOverride, ProviderRequestConfig,
    StreamSimpleConfig as RegistryStreamSimpleConfig,
};
use json::strip_json_comments;
use models::{
    apply_model_override, apply_provider_override, merge_custom_models, parse_models,
    validate_config,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod config;
mod json;
mod models;

pub use crate::resolve_config_value::clear_config_value_cache as clear_api_key_cache;
pub use config::{
    ModelDefinition, ModelOverride, PartialModelCost, ProviderConfig, StreamSimpleConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequestAuth {
    pub ok: bool,
    pub api_key: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub error: Option<String>,
}

pub struct ModelRegistry<B: AuthStorageBackend> {
    models: Vec<Model>,
    provider_request_configs: BTreeMap<String, ProviderRequestConfig>,
    model_request_headers: BTreeMap<String, BTreeMap<String, String>>,
    registered_providers: BTreeMap<String, ProviderConfig>,
    load_error: Option<String>,
    pub auth_storage: AuthStorage<B>,
    models_json_path: Option<PathBuf>,
}

impl<B: AuthStorageBackend> ModelRegistry<B> {
    pub fn create(auth_storage: AuthStorage<B>, models_json_path: impl Into<PathBuf>) -> Self {
        Self::new(auth_storage, Some(models_json_path.into()))
    }

    pub fn in_memory(auth_storage: AuthStorage<B>) -> Self {
        Self::new(auth_storage, None)
    }

    fn new(auth_storage: AuthStorage<B>, models_json_path: Option<PathBuf>) -> Self {
        let mut registry = Self {
            models: Vec::new(),
            provider_request_configs: BTreeMap::new(),
            model_request_headers: BTreeMap::new(),
            registered_providers: BTreeMap::new(),
            load_error: None,
            auth_storage,
            models_json_path,
        };
        registry.load_models();
        registry
    }

    pub fn refresh(&mut self) {
        self.provider_request_configs.clear();
        self.model_request_headers.clear();
        self.load_error = None;
        clear_config_value_cache();
        self.load_models();

        for (provider, config) in self.registered_providers.clone() {
            self.apply_provider_config(&provider, &config);
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn get_all(&self) -> Vec<Model> {
        self.models.clone()
    }

    #[cfg(test)]
    pub fn with_models(mut self, models: Vec<Model>) -> Self {
        self.models = models;
        self
    }

    pub fn get_available(&self) -> Vec<Model> {
        self.models
            .iter()
            .filter(|model| self.has_configured_auth(model))
            .cloned()
            .collect()
    }

    pub fn find(&self, provider: &str, model_id: &str) -> Option<Model> {
        self.models
            .iter()
            .find(|model| model.provider == provider && model.id == model_id)
            .cloned()
    }

    pub fn has_configured_auth(&self, model: &Model) -> bool {
        self.auth_storage.has_auth(&model.provider)
            || self
                .provider_request_configs
                .get(&model.provider)
                .is_some_and(|config| config.api_key.is_some())
    }

    pub fn get_api_key_and_headers(&self, model: &Model) -> ResolvedRequestAuth {
        match self.try_get_api_key_and_headers(model) {
            Ok((api_key, headers)) => ResolvedRequestAuth {
                ok: true,
                api_key,
                headers,
                error: None,
            },
            Err(error) => ResolvedRequestAuth {
                ok: false,
                api_key: None,
                headers: None,
                error: Some(error),
            },
        }
    }

    pub fn provider_auth_status(&self, provider: &str) -> AuthStatus {
        let auth_status = self.auth_storage.auth_status(provider);
        if auth_status.source.is_some() {
            return auth_status;
        }
        let Some(api_key) = self
            .provider_request_configs
            .get(provider)
            .and_then(|config| config.api_key.as_deref())
        else {
            return auth_status;
        };
        if api_key.starts_with('!') {
            return AuthStatus {
                configured: true,
                source: Some(AuthSource::ModelsJsonCommand),
                label: None,
            };
        }
        if std::env::var(api_key).is_ok() {
            return AuthStatus {
                configured: true,
                source: Some(AuthSource::Environment),
                label: Some(api_key.to_string()),
            };
        }
        AuthStatus {
            configured: true,
            source: Some(AuthSource::ModelsJsonKey),
            label: None,
        }
    }

    pub fn provider_display_name(&self, provider: &str) -> String {
        self.registered_providers
            .get(provider)
            .and_then(|config| config.name.clone())
            .or_else(|| get_oauth_provider(provider).map(|provider| provider.name().to_string()))
            .or_else(|| built_in_provider_display_name(provider).map(str::to_string))
            .unwrap_or_else(|| provider.to_string())
    }

    pub fn api_key_for_provider(&self, provider: &str) -> Option<String> {
        self.auth_storage.api_key(provider, false).or_else(|| {
            self.provider_request_configs
                .get(provider)
                .and_then(|config| config.api_key.as_deref())
                .and_then(resolve_config_value_uncached)
        })
    }

    pub fn should_warn_about_anthropic_subscription_auth(
        &self,
        warnings: &WarningSettings,
        warning_already_shown: bool,
        model_provider: &str,
    ) -> bool {
        if warnings.anthropic_extra_usage == Some(false)
            || warning_already_shown
            || model_provider != "anthropic"
        {
            return false;
        }

        let stored_credential_kind = match self.auth_storage.get("anthropic") {
            Some(AuthCredential::OAuth { .. }) => Some(AnthropicCredentialKind::OAuth),
            Some(AuthCredential::ApiKey { .. }) => Some(AnthropicCredentialKind::ApiKey),
            None => None,
        };
        let api_key = (stored_credential_kind != Some(AnthropicCredentialKind::OAuth))
            .then(|| self.api_key_for_provider(model_provider))
            .flatten();

        should_warn_about_anthropic_subscription_auth(AnthropicSubscriptionWarningInput {
            warnings,
            warning_already_shown,
            model_provider: Some(model_provider),
            stored_credential_kind,
            api_key: api_key.as_deref(),
        })
    }

    pub fn register_provider(&mut self, provider: impl Into<String>, config: ProviderConfig) {
        self.try_register_provider(provider, config)
            .expect("provider config should be valid");
    }

    pub fn try_register_provider(
        &mut self,
        provider: impl Into<String>,
        config: ProviderConfig,
    ) -> Result<(), String> {
        let provider = provider.into();
        self.validate_registered_provider_config(&provider, &config)?;
        self.upsert_registered_provider(provider, config);
        self.refresh();
        Ok(())
    }

    pub fn unregister_provider(&mut self, provider: &str) {
        if self.registered_providers.remove(provider).is_some() {
            unregister_oauth_provider(provider);
            self.refresh();
        }
    }

    pub fn apply_registered_api_providers(&self, providers: &mut ProviderRegistry) {
        for (provider, config) in &self.registered_providers {
            if let (Some(api), Some(stream_simple)) = (&config.api, &config.stream_simple) {
                providers.register(
                    RegisteredProvider::Dynamic(RegisteredDynamicProvider::new(
                        api.clone(),
                        dynamic_stream_simple(stream_simple.clone()),
                    )),
                    Some(dynamic_provider_source_id(provider)),
                );
            }
        }
    }

    pub fn unregister_api_provider(&self, providers: &mut ProviderRegistry, provider: &str) {
        providers.unregister_source(&dynamic_provider_source_id(provider));
        if let Some(api) = self
            .registered_providers
            .get(provider)
            .and_then(|config| config.api.as_deref())
        {
            providers.restore_builtin_api(api);
        }
    }

    fn load_models(&mut self) {
        let custom = self
            .models_json_path
            .clone()
            .map(|path| self.load_custom_models(&path))
            .unwrap_or_default();
        self.load_error = custom.error.clone();

        let builtins =
            self.load_builtin_models(&custom.provider_overrides, &custom.model_overrides);
        self.models = merge_custom_models(builtins, custom.models);
    }

    fn load_builtin_models(
        &self,
        provider_overrides: &BTreeMap<String, ProviderOverride>,
        model_overrides: &BTreeMap<String, BTreeMap<String, ModelOverride>>,
    ) -> Vec<Model> {
        AiModelRegistry::builtins()
            .all_models()
            .into_iter()
            .map(|model| {
                let model = provider_overrides
                    .get(&model.provider)
                    .map(|override_config| apply_provider_override(model.clone(), override_config))
                    .unwrap_or(model);
                model_overrides
                    .get(&model.provider)
                    .and_then(|overrides| overrides.get(&model.id))
                    .map(|override_config| apply_model_override(model.clone(), override_config))
                    .unwrap_or(model)
            })
            .collect()
    }

    fn load_custom_models(&mut self, path: &Path) -> CustomModelsResult {
        if !path.exists() {
            return CustomModelsResult::default();
        }
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                return CustomModelsResult {
                    error: Some(format!(
                        "Failed to load models.json: {error}\n\nFile: {}",
                        path.display()
                    )),
                    ..CustomModelsResult::default()
                }
            }
        };
        let config = match serde_json::from_str::<ModelsConfig>(&strip_json_comments(&content)) {
            Ok(config) => config,
            Err(error) => {
                return CustomModelsResult {
                    error: Some(format!(
                        "Failed to parse models.json: {error}\n\nFile: {}",
                        path.display()
                    )),
                    ..CustomModelsResult::default()
                }
            }
        };
        if let Err(error) = validate_config(&config) {
            return CustomModelsResult {
                error: Some(format!("{error}\n\nFile: {}", path.display())),
                ..CustomModelsResult::default()
            };
        }

        let mut result = CustomModelsResult::default();
        for (provider, provider_config) in &config.providers {
            if provider_config.name.is_some()
                || provider_config.base_url.is_some()
                || provider_config.compat.is_some()
            {
                result.provider_overrides.insert(
                    provider.clone(),
                    ProviderOverride {
                        display_name: provider_config.name.clone(),
                        base_url: provider_config.base_url.clone(),
                        compat: provider_config.compat.clone(),
                    },
                );
            }
            self.store_provider_request_config(provider, provider_config);
            if let Some(overrides) = &provider_config.model_overrides {
                for (model_id, model_override) in overrides {
                    self.store_model_headers(provider, model_id, model_override.headers.clone());
                }
                result
                    .model_overrides
                    .insert(provider.clone(), overrides.clone());
            }
        }
        result.models = parse_models(&config);
        result
    }

    fn try_get_api_key_and_headers(
        &self,
        model: &Model,
    ) -> Result<(Option<String>, Option<BTreeMap<String, String>>), String> {
        let provider_config = self.provider_request_configs.get(&model.provider);
        let api_key = match self.auth_storage.api_key(&model.provider, false) {
            Some(api_key) => Some(api_key),
            None => provider_config
                .and_then(|config| config.api_key.as_deref())
                .map(|api_key| {
                    resolve_config_value_or_throw(
                        api_key,
                        &format!("API key for provider \"{}\"", model.provider),
                    )
                })
                .transpose()?,
        };
        let provider_headers = resolve_headers_or_throw(
            provider_config.and_then(|config| config.headers.as_ref()),
            &format!("provider \"{}\"", model.provider),
        )?;
        let model_headers = resolve_headers_or_throw(
            self.model_request_headers
                .get(&model_request_key(&model.provider, &model.id)),
            &format!("model \"{}/{}\"", model.provider, model.id),
        )?;

        let mut headers = model.headers.clone();
        if let Some(provider_headers) = provider_headers {
            headers.extend(provider_headers);
        }
        if let Some(model_headers) = model_headers {
            headers.extend(model_headers);
        }
        if provider_config.and_then(|config| config.auth_header) == Some(true) {
            let Some(api_key) = &api_key else {
                return Err(format!("No API key found for \"{}\"", model.provider));
            };
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
        }

        Ok((api_key, (!headers.is_empty()).then_some(headers)))
    }

    fn store_provider_request_config(&mut self, provider: &str, config: &ProviderConfig) {
        if config.api_key.is_none() && config.headers.is_none() && config.auth_header.is_none() {
            return;
        }
        self.provider_request_configs.insert(
            provider.to_string(),
            ProviderRequestConfig {
                api_key: config.api_key.clone(),
                headers: config.headers.clone(),
                auth_header: config.auth_header,
            },
        );
    }

    fn store_model_headers(
        &mut self,
        provider: &str,
        model_id: &str,
        headers: Option<BTreeMap<String, String>>,
    ) {
        let key = model_request_key(provider, model_id);
        if let Some(headers) = headers.filter(|headers| !headers.is_empty()) {
            self.model_request_headers.insert(key, headers);
        } else {
            self.model_request_headers.remove(&key);
        }
    }

    fn apply_provider_config(&mut self, provider: &str, config: &ProviderConfig) {
        if let Some(oauth) = &config.oauth {
            register_oauth_provider(Arc::new(RegisteredOAuthProvider {
                id: provider.to_string(),
                config: oauth.clone(),
            }));
        }
        self.store_provider_request_config(provider, config);
        let has_models = config
            .models
            .as_ref()
            .is_some_and(|models| !models.is_empty());
        if !has_models {
            if config.base_url.is_some() || config.compat.is_some() {
                let override_config = ProviderOverride {
                    display_name: config.name.clone(),
                    base_url: config.base_url.clone(),
                    compat: config.compat.clone(),
                };
                self.models = self
                    .models
                    .drain(..)
                    .map(|model| {
                        if model.provider == provider {
                            apply_provider_override(model, &override_config)
                        } else {
                            model
                        }
                    })
                    .collect();
            }
            return;
        }

        let models_config = ModelsConfig {
            providers: BTreeMap::from([(provider.to_string(), config.clone())]),
        };
        self.models.retain(|model| model.provider != provider);
        for model in parse_models(&models_config) {
            self.store_model_headers(provider, &model.id, config_model_headers(config, &model.id));
            self.models.push(model);
        }
    }

    fn validate_registered_provider_config(
        &self,
        provider: &str,
        config: &ProviderConfig,
    ) -> Result<(), String> {
        if config.stream_simple.is_some() && config.api.is_none() {
            return Err(format!(
                "Provider {provider}: \"api\" is required when registering streamSimple."
            ));
        }
        let Some(models) = &config.models else {
            return Ok(());
        };
        if models.is_empty() {
            return Ok(());
        }
        if config.base_url.is_none() {
            return Err(format!(
                "Provider {provider}: \"baseUrl\" is required when defining models."
            ));
        }
        if config.api_key.is_none() {
            return Err(format!(
                "Provider {provider}: \"apiKey\" is required when defining models."
            ));
        }
        for model in models {
            if model.api.is_none() && config.api.is_none() {
                return Err(format!(
                    "Provider {provider}, model {}: no \"api\" specified.",
                    model.id
                ));
            }
        }
        Ok(())
    }

    fn upsert_registered_provider(&mut self, provider: String, config: ProviderConfig) {
        let Some(existing) = self.registered_providers.get_mut(&provider) else {
            self.registered_providers.insert(provider, config);
            return;
        };
        if config.name.is_some() {
            existing.name = config.name;
        }
        if config.api_key.is_some() {
            existing.api_key = config.api_key;
        }
        if config.api.is_some() {
            existing.api = config.api;
        }
        if config.base_url.is_some() {
            existing.base_url = config.base_url;
        }
        if config.headers.is_some() {
            existing.headers = config.headers;
        }
        if config.compat.is_some() {
            existing.compat = config.compat;
        }
        if config.auth_header.is_some() {
            existing.auth_header = config.auth_header;
        }
        if config.models.is_some() {
            existing.models = config.models;
        }
        if config.model_overrides.is_some() {
            existing.model_overrides = config.model_overrides;
        }
        if config.oauth.is_some() {
            existing.oauth = config.oauth;
        }
        if config.stream_simple.is_some() {
            existing.stream_simple = config.stream_simple;
        }
    }
}

struct RegisteredOAuthProvider {
    id: String,
    config: OAuthProviderConfig,
}

impl OAuthProviderInterface for RegisteredOAuthProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn login(&self, _callbacks: &mut dyn OAuthLoginCallbacks) -> ai::AiResult<OAuthCredentials> {
        Err(ai::AiError::InvalidResponse(format!(
            "OAuth login callback is not available for dynamically registered provider {}",
            self.id
        )))
    }

    fn refresh_token(&self, credentials: &OAuthCredentials) -> ai::AiResult<OAuthCredentials> {
        Ok(credentials.clone())
    }

    fn get_api_key(&self, credentials: &OAuthCredentials) -> String {
        credentials.access.clone()
    }
}

fn config_model_headers(
    config: &ProviderConfig,
    model_id: &str,
) -> Option<BTreeMap<String, String>> {
    config
        .models
        .as_ref()
        .and_then(|models| models.iter().find(|model| model.id == model_id))
        .and_then(|model| model.headers.clone())
}

fn dynamic_provider_source_id(provider: &str) -> String {
    format!("provider:{provider}")
}

fn dynamic_stream_simple(
    config: RegistryStreamSimpleConfig,
) -> impl Fn(ai::StreamRequest) -> ai::AiResult<Vec<StreamEvent>> + Send + Sync + 'static {
    move |request| {
        if let Some(handler) = &config.handler {
            return handler(request);
        }
        Ok(vec![StreamEvent::TextDelta {
            text: config.text.clone().unwrap_or_default(),
        }])
    }
}

impl<B: AuthStorageBackend> CodingModelRegistry for ModelRegistry<B> {
    fn all_models(&self) -> Vec<Model> {
        self.get_all()
    }

    fn available_models(&self) -> Vec<Model> {
        self.get_available()
    }
}

fn model_request_key(provider: &str, model_id: &str) -> String {
    format!("{provider}:{model_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_storage::{AuthStorageData, InMemoryAuthStorageBackend};
    use crate::model_registry::config::{OAuthProviderConfig, StreamSimpleConfig};
    use ai::LanguageModelProvider;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn loads_builtin_models_without_models_json() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::in_memory(storage);
        assert!(registry.find("local", "echo").is_some());
    }

    #[test]
    fn loads_custom_models_and_headers() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "DEMO_API_KEY",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "headers": {"X-Demo": "literal"},
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");

        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry.find("demo", "demo-1").expect("model should load");
        assert_eq!(model.display_name, "Demo 1");
        assert_eq!(model.context_window, 4096);
        let auth = registry.get_api_key_and_headers(&model);
        assert!(auth.ok);
        assert_eq!(
            auth.headers
                .and_then(|headers| headers.get("X-Demo").cloned()),
            Some("literal".to_string())
        );
    }

    #[test]
    fn builtin_provider_custom_models_inherit_api_and_base_url_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "models": [
                    {"id": "fake-provider/fake-model", "name": "Fake Model"}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");

        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry
            .find("openrouter", "fake-provider/fake-model")
            .expect("model should load");

        assert_eq!(model.api, "openai-completions");
        assert_eq!(
            model.base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
    }

    #[test]
    fn non_builtin_custom_models_require_base_url_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "my-custom-provider": {
                  "apiKey": "literal-key",
                  "api": "openai-completions",
                  "models": [
                    {"id": "demo-1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");

        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert!(registry.find("my-custom-provider", "demo-1").is_none());
        assert!(registry
            .error()
            .is_some_and(|error| error.contains("\"baseUrl\" is required")));
    }

    #[test]
    fn request_auth_includes_model_builtin_headers_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::in_memory(storage);
        let mut model = Model {
            provider: "local".to_string(),
            id: "echo".to_string(),
            ..Model::default()
        };
        model
            .headers
            .insert("X-Model".to_string(), "builtin".to_string());

        let auth = registry.get_api_key_and_headers(&model);

        assert!(auth.ok);
        assert_eq!(
            auth.headers
                .and_then(|headers| headers.get("X-Model").cloned()),
            Some("builtin".to_string())
        );
    }

    #[test]
    fn provider_headers_resolve_at_request_time_like_pi() {
        std::env::set_var("PM_AGENT_TEST_DYNAMIC_HEADER", "first");
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "literal-key",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "headers": {"X-Dynamic": "PM_AGENT_TEST_DYNAMIC_HEADER"},
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry.find("demo", "demo-1").expect("model should load");

        let first = registry.get_api_key_and_headers(&model);
        std::env::set_var("PM_AGENT_TEST_DYNAMIC_HEADER", "second");
        let second = registry.get_api_key_and_headers(&model);

        assert_eq!(
            first
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Dynamic"))
                .map(String::as_str),
            Some("first")
        );
        assert_eq!(
            second
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Dynamic"))
                .map(String::as_str),
            Some("second")
        );
        std::env::remove_var("PM_AGENT_TEST_DYNAMIC_HEADER");
    }

    #[test]
    fn model_override_headers_resolve_at_request_time_like_pi() {
        std::env::set_var("PM_AGENT_TEST_DYNAMIC_MODEL_HEADER", "first");
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "literal-key",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ],
                  "modelOverrides": {
                    "demo-1": {
                      "headers": {"X-Model-Dynamic": "PM_AGENT_TEST_DYNAMIC_MODEL_HEADER"}
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry.find("demo", "demo-1").expect("model should load");

        let first = registry.get_api_key_and_headers(&model);
        std::env::set_var("PM_AGENT_TEST_DYNAMIC_MODEL_HEADER", "second");
        let second = registry.get_api_key_and_headers(&model);

        assert_eq!(
            first
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Model-Dynamic"))
                .map(String::as_str),
            Some("first")
        );
        assert_eq!(
            second
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Model-Dynamic"))
                .map(String::as_str),
            Some("second")
        );
        std::env::remove_var("PM_AGENT_TEST_DYNAMIC_MODEL_HEADER");
    }

    #[test]
    fn provider_compat_applies_to_custom_models_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "literal-key",
                  "api": "openai-completions",
                  "baseUrl": "https://example.com/v1",
                  "compat": {
                    "supportsUsageInStreaming": false,
                    "maxTokensField": "max_tokens"
                  },
                  "models": [
                    {"id": "demo-1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let model = registry.find("demo", "demo-1").expect("model should load");

        assert_eq!(
            model.compat.get("supportsUsageInStreaming"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            model.compat.get("maxTokensField"),
            Some(&serde_json::json!("max_tokens"))
        );
    }

    #[test]
    fn model_compat_overrides_provider_compat_for_custom_models_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "literal-key",
                  "api": "openai-completions",
                  "baseUrl": "https://example.com/v1",
                  "compat": {
                    "supportsUsageInStreaming": false,
                    "maxTokensField": "max_tokens"
                  },
                  "models": [
                    {
                      "id": "demo-1",
                      "contextWindow": 4096,
                      "compat": {
                        "supportsUsageInStreaming": true,
                        "maxTokensField": "max_completion_tokens"
                      }
                    }
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let model = registry.find("demo", "demo-1").expect("model should load");

        assert_eq!(
            model.compat.get("supportsUsageInStreaming"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            model.compat.get("maxTokensField"),
            Some(&serde_json::json!("max_completion_tokens"))
        );
    }

    #[test]
    fn provider_and_model_override_compat_apply_to_builtin_models_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "local": {
                  "compat": {
                    "supportsUsageInStreaming": false,
                    "supportsStrictMode": false
                  },
                  "modelOverrides": {
                    "echo": {
                      "compat": {
                        "supportsStrictMode": true,
                        "cacheControlFormat": "anthropic"
                      }
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let model = registry.find("local", "echo").expect("model should load");

        assert_eq!(
            model.compat.get("supportsUsageInStreaming"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            model.compat.get("supportsStrictMode"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            model.compat.get("cacheControlFormat"),
            Some(&serde_json::json!("anthropic"))
        );
    }

    #[test]
    fn model_override_deep_merges_nested_compat_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "local": {
                  "compat": {
                    "nested": {
                      "provider": true,
                      "shared": {
                        "left": "provider"
                      }
                    }
                  },
                  "modelOverrides": {
                    "echo": {
                      "compat": {
                        "nested": {
                          "model": true,
                          "shared": {
                            "right": "model"
                          }
                        }
                      }
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let model = registry.find("local", "echo").expect("model should load");
        assert_eq!(
            model.compat.get("nested"),
            Some(&serde_json::json!({
                "provider": true,
                "model": true,
                "shared": {
                    "left": "provider",
                    "right": "model"
                }
            }))
        );
    }

    #[test]
    fn registered_base_url_override_keeps_builtin_models_after_refresh_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "anthropic",
            ProviderConfig {
                base_url: Some("https://proxy.test/anthropic".to_string()),
                ..ProviderConfig::default()
            },
        );
        registry.refresh();

        let models: Vec<_> = registry
            .get_all()
            .into_iter()
            .filter(|model| model.provider == "anthropic")
            .collect();
        assert!(models.len() > 1);
        assert!(models
            .iter()
            .all(|model| model.base_url.as_deref() == Some("https://proxy.test/anthropic")));
    }

    #[test]
    fn registered_models_replace_builtin_provider_models_after_refresh_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "anthropic",
            ProviderConfig {
                base_url: Some("https://custom.test/anthropic".to_string()),
                api_key: Some("literal-key".to_string()),
                api: Some("anthropic-messages".to_string()),
                models: Some(vec![ModelDefinition {
                    id: "custom-claude".to_string(),
                    name: Some("Custom Claude".to_string()),
                    ..ModelDefinition::default()
                }]),
                ..ProviderConfig::default()
            },
        );
        registry.refresh();

        let models: Vec<_> = registry
            .get_all()
            .into_iter()
            .filter(|model| model.provider == "anthropic")
            .collect();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["custom-claude"]
        );
        assert_eq!(
            models[0].base_url.as_deref(),
            Some("https://custom.test/anthropic")
        );
    }

    #[test]
    fn registered_base_url_only_override_keeps_custom_provider_models_after_refresh_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "custom-provider",
            ProviderConfig {
                base_url: Some("https://custom.test/v1".to_string()),
                api_key: Some("literal-key".to_string()),
                api: Some("openai-completions".to_string()),
                models: Some(vec![
                    ModelDefinition {
                        id: "custom-a".to_string(),
                        name: Some("Custom A".to_string()),
                        ..ModelDefinition::default()
                    },
                    ModelDefinition {
                        id: "custom-b".to_string(),
                        name: Some("Custom B".to_string()),
                        ..ModelDefinition::default()
                    },
                ]),
                ..ProviderConfig::default()
            },
        );
        registry.register_provider(
            "custom-provider",
            ProviderConfig {
                base_url: Some("https://proxy.test/custom".to_string()),
                ..ProviderConfig::default()
            },
        );
        registry.refresh();

        let models: Vec<_> = registry
            .get_all()
            .into_iter()
            .filter(|model| model.provider == "custom-provider")
            .collect();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["custom-a", "custom-b"]
        );
        assert!(models
            .iter()
            .all(|model| model.base_url.as_deref() == Some("https://proxy.test/custom")));
    }

    #[test]
    fn registered_headers_only_override_keeps_custom_provider_models_after_refresh_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "custom-provider",
            ProviderConfig {
                base_url: Some("https://custom.test/v1".to_string()),
                api_key: Some("literal-key".to_string()),
                api: Some("openai-completions".to_string()),
                models: Some(vec![ModelDefinition {
                    id: "custom-a".to_string(),
                    name: Some("Custom A".to_string()),
                    ..ModelDefinition::default()
                }]),
                ..ProviderConfig::default()
            },
        );
        registry.register_provider(
            "custom-provider",
            ProviderConfig {
                headers: Some(BTreeMap::from([(
                    "x-proxy".to_string(),
                    "enabled".to_string(),
                )])),
                ..ProviderConfig::default()
            },
        );
        registry.refresh();

        let model = registry
            .find("custom-provider", "custom-a")
            .expect("custom model should survive refresh");
        assert_eq!(model.base_url.as_deref(), Some("https://custom.test/v1"));
        let auth = registry.get_api_key_and_headers(&model);
        assert!(auth.ok);
        assert_eq!(
            auth.headers
                .and_then(|headers| headers.get("x-proxy").cloned()),
            Some("enabled".to_string())
        );
    }

    #[test]
    fn model_override_applies_to_single_builtin_model_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "modelOverrides": {
                    "anthropic/claude-sonnet-4": {
                      "name": "Custom Sonnet Name"
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let sonnet = registry
            .find("openrouter", "anthropic/claude-sonnet-4")
            .expect("sonnet should exist");
        let opus = registry
            .find("openrouter", "anthropic/claude-opus-4")
            .expect("opus should exist");

        assert_eq!(sonnet.display_name, "Custom Sonnet Name");
        assert_ne!(opus.display_name, "Custom Sonnet Name");
    }

    #[test]
    fn nonexistent_model_override_is_ignored_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "modelOverrides": {
                    "nonexistent/model-id": {
                      "name": "This should not appear"
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert!(registry
            .find("openrouter", "nonexistent/model-id")
            .is_none());
        assert!(registry.error().is_none());
    }

    #[test]
    fn model_override_can_change_cost_fields_partially_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "modelOverrides": {
                    "anthropic/claude-sonnet-4": {
                      "cost": {"input": 99}
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let sonnet = registry
            .find("openrouter", "anthropic/claude-sonnet-4")
            .expect("sonnet should exist");

        assert_eq!(sonnet.cost.input, 99.0);
        assert!(sonnet.cost.output > 0.0);
    }

    #[test]
    fn refresh_picks_up_and_removes_model_override_changes_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "modelOverrides": {
                    "anthropic/claude-sonnet-4": {
                      "name": "First Name"
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::create(storage, path.clone());

        assert_eq!(
            registry
                .find("openrouter", "anthropic/claude-sonnet-4")
                .map(|model| model.display_name),
            Some("First Name".to_string())
        );

        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "modelOverrides": {
                    "anthropic/claude-sonnet-4": {
                      "name": "Second Name"
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be updated");
        registry.refresh();
        assert_eq!(
            registry
                .find("openrouter", "anthropic/claude-sonnet-4")
                .map(|model| model.display_name),
            Some("Second Name".to_string())
        );

        fs::write(&path, r#"{"providers": {}}"#).expect("models.json should be cleared");
        registry.refresh();
        assert_ne!(
            registry
                .find("openrouter", "anthropic/claude-sonnet-4")
                .map(|model| model.display_name),
            Some("Second Name".to_string())
        );
    }

    #[test]
    fn model_overrides_still_apply_when_provider_also_defines_models_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openrouter": {
                  "baseUrl": "https://my-proxy.example.com/v1",
                  "apiKey": "literal-key",
                  "api": "openai-completions",
                  "models": [
                    {
                      "id": "custom/openrouter-model",
                      "name": "Custom OpenRouter Model",
                      "contextWindow": 128000
                    }
                  ],
                  "modelOverrides": {
                    "anthropic/claude-sonnet-4": {
                      "name": "Overridden Built-in Sonnet"
                    }
                  }
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert!(registry
            .find("openrouter", "custom/openrouter-model")
            .is_some());
        assert_eq!(
            registry
                .find("openrouter", "anthropic/claude-sonnet-4")
                .map(|model| model.display_name),
            Some("Overridden Built-in Sonnet".to_string())
        );
    }

    #[test]
    fn refresh_reloads_merged_custom_models_from_disk_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "anthropic": {
                  "baseUrl": "https://first-proxy.example.com/v1",
                  "apiKey": "literal-key",
                  "api": "anthropic-messages",
                  "models": [
                    {"id": "claude-custom", "name": "Custom Claude"}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::create(storage, path.clone());

        assert!(registry.find("anthropic", "claude-custom").is_some());

        fs::write(
            &path,
            r#"{
              "providers": {
                "anthropic": {
                  "baseUrl": "https://second-proxy.example.com/v1",
                  "apiKey": "literal-key",
                  "api": "anthropic-messages",
                  "models": [
                    {"id": "claude-custom-2", "name": "Custom Claude 2"}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be updated");
        registry.refresh();

        assert!(registry.find("anthropic", "claude-custom").is_none());
        assert!(registry.find("anthropic", "claude-custom-2").is_some());
        assert!(registry
            .get_all()
            .iter()
            .any(|model| model.provider == "anthropic" && model.id.contains("claude")));
    }

    #[test]
    fn provider_display_name_resolves_registered_builtin_and_fallback_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        assert_eq!(registry.provider_display_name("openai"), "OpenAI");
        assert_eq!(
            registry.provider_display_name("unknown-provider"),
            "unknown-provider"
        );

        registry.register_provider(
            "named-provider",
            ProviderConfig {
                name: Some("Named Provider".to_string()),
                base_url: Some("https://provider.test/v1".to_string()),
                api_key: Some("literal-key".to_string()),
                api: Some("openai-completions".to_string()),
                models: Some(vec![ModelDefinition {
                    id: "demo-model".to_string(),
                    name: Some("Demo Model".to_string()),
                    ..ModelDefinition::default()
                }]),
                ..ProviderConfig::default()
            },
        );

        assert_eq!(
            registry.provider_display_name("named-provider"),
            "Named Provider"
        );
    }

    #[test]
    fn registered_oauth_provider_controls_display_name_like_pi() {
        ai::utils::reset_oauth_providers();
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "oauth-provider",
            ProviderConfig {
                oauth: Some(OAuthProviderConfig {
                    name: "OAuth Provider".to_string(),
                }),
                ..ProviderConfig::default()
            },
        );

        assert_eq!(
            ai::utils::get_oauth_provider("oauth-provider")
                .map(|provider| provider.name().to_string()),
            Some("OAuth Provider".to_string())
        );
        assert_eq!(
            registry.provider_display_name("oauth-provider"),
            "OAuth Provider"
        );

        registry.unregister_provider("oauth-provider");
        ai::utils::reset_oauth_providers();
    }

    #[test]
    fn unregister_provider_restores_builtin_oauth_provider_like_pi() {
        ai::utils::reset_oauth_providers();
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        registry.register_provider(
            "anthropic",
            ProviderConfig {
                oauth: Some(OAuthProviderConfig {
                    name: "Custom Anthropic OAuth".to_string(),
                }),
                ..ProviderConfig::default()
            },
        );
        assert_eq!(
            ai::utils::get_oauth_provider("anthropic").map(|provider| provider.name().to_string()),
            Some("Custom Anthropic OAuth".to_string())
        );

        registry.unregister_provider("anthropic");

        assert_ne!(
            ai::utils::get_oauth_provider("anthropic").map(|provider| provider.name().to_string()),
            Some("Custom Anthropic OAuth".to_string())
        );
        ai::utils::reset_oauth_providers();
    }

    #[test]
    fn invalid_stream_simple_registration_reports_pi_error() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);

        let error = registry
            .try_register_provider(
                "broken-provider",
                ProviderConfig {
                    stream_simple: Some(StreamSimpleConfig {
                        text: Some("unused".to_string()),
                        handler: None,
                    }),
                    ..ProviderConfig::default()
                },
            )
            .expect_err("streamSimple without api should fail");

        assert_eq!(
            error,
            "Provider broken-provider: \"api\" is required when registering streamSimple."
        );
    }

    #[test]
    fn registered_stream_simple_overrides_and_restores_api_provider_like_pi() {
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);
        let mut providers = ProviderRegistry::builtins();
        let model = Model {
            id: "echo".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Echo".to_string(),
            context_window: 32_000,
            ..Model::default()
        };

        registry.register_provider(
            "stream-override-provider",
            ProviderConfig {
                api: Some("local-echo".to_string()),
                stream_simple: Some(StreamSimpleConfig {
                    text: Some("custom streamSimple override".to_string()),
                    handler: None,
                }),
                ..ProviderConfig::default()
            },
        );
        registry.apply_registered_api_providers(&mut providers);

        let overridden = providers
            .provider_for(&model)
            .expect("provider")
            .stream(ai::StreamRequest {
                model: model.clone(),
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::new(),
            })
            .expect("stream");
        assert_eq!(
            overridden,
            vec![StreamEvent::TextDelta {
                text: "custom streamSimple override".to_string()
            }]
        );

        registry.unregister_api_provider(&mut providers, "stream-override-provider");
        registry.unregister_provider("stream-override-provider");

        let restored = providers
            .provider_for(&model)
            .expect("restored provider")
            .stream(ai::StreamRequest {
                model,
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::new(),
            })
            .expect("stream");
        assert_ne!(
            restored,
            vec![StreamEvent::TextDelta {
                text: "custom streamSimple override".to_string()
            }]
        );
    }

    #[test]
    fn request_auth_reports_provider_api_key_resolution_errors_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "!sh -c 'exit 7'",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");

        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry.find("demo", "demo-1").expect("model should load");

        let auth = registry.get_api_key_and_headers(&model);

        assert!(!auth.ok);
        assert_eq!(
            auth.error.as_deref(),
            Some("Failed to resolve API key for provider \"demo\" from shell command: sh -c 'exit 7'")
        );
    }

    #[test]
    fn provider_auth_status_reports_models_json_key_sources_like_pi() {
        let dir = temp_dir();
        let key_path = dir.join("models-key.json");
        fs::write(
            &key_path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "literal-key",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, key_path);
        assert_eq!(
            registry.provider_auth_status("demo").source,
            Some(AuthSource::ModelsJsonKey)
        );

        let command_path = dir.join("models-command.json");
        fs::write(
            &command_path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "!printf key",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, command_path);
        assert_eq!(
            registry.provider_auth_status("demo").source,
            Some(AuthSource::ModelsJsonCommand)
        );
    }

    #[test]
    fn api_key_for_provider_resolves_command_output_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            api_key_models_json("custom-provider", "!printf '  line1\\nline2  '"),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(
            registry.api_key_for_provider("custom-provider").as_deref(),
            Some("line1\nline2")
        );
    }

    #[test]
    fn anthropic_subscription_warning_uses_stored_oauth_like_pi() {
        let mut data = AuthStorageData::new();
        data.insert(
            "anthropic".to_string(),
            crate::auth_storage::AuthCredential::OAuth {
                data: BTreeMap::new(),
            },
        );
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(data);
        let registry = ModelRegistry::in_memory(storage);

        assert!(registry.should_warn_about_anthropic_subscription_auth(
            &crate::settings_manager::WarningSettings::default(),
            false,
            "anthropic",
        ));
    }

    #[test]
    fn anthropic_subscription_warning_uses_subscription_api_key_like_pi() {
        let mut data = AuthStorageData::new();
        data.insert(
            "anthropic".to_string(),
            crate::auth_storage::AuthCredential::ApiKey {
                key: "sk-ant-oat01-test".to_string(),
            },
        );
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(data);
        let registry = ModelRegistry::in_memory(storage);

        assert!(registry.should_warn_about_anthropic_subscription_auth(
            &crate::settings_manager::WarningSettings::default(),
            false,
            "anthropic",
        ));
    }

    #[test]
    fn anthropic_subscription_warning_skips_non_anthropic_without_api_key_lookup_like_pi() {
        let dir = temp_dir();
        let marker = dir.join("should-not-exist");
        let marker_path = sh_path(&marker);
        let path = dir.join("models.json");
        fs::write(
            &path,
            api_key_models_json(
                "custom-provider",
                &format!("!sh -c 'touch \"{marker_path}\"; printf key'"),
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert!(!registry.should_warn_about_anthropic_subscription_auth(
            &crate::settings_manager::WarningSettings::default(),
            false,
            "custom-provider",
        ));
        assert!(!marker.exists());
    }

    #[test]
    fn api_key_for_provider_supports_shell_features_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            api_key_models_json("custom-provider", "!echo 'hello world' | tr ' ' '-'"),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(
            registry.api_key_for_provider("custom-provider").as_deref(),
            Some("hello-world")
        );
    }

    #[test]
    fn api_key_for_provider_returns_none_for_failed_empty_or_missing_commands_like_pi() {
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            format!(
                r#"{{
                  "providers": {{
                    "failed-provider": {},
                    "empty-provider": {},
                    "missing-provider": {}
                  }}
                }}"#,
                api_key_provider_config("!exit 1"),
                api_key_provider_config("!printf ''"),
                api_key_provider_config("!nonexistent-command-12345")
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(registry.api_key_for_provider("failed-provider"), None);
        assert_eq!(registry.api_key_for_provider("empty-provider"), None);
        assert_eq!(registry.api_key_for_provider("missing-provider"), None);
    }

    #[test]
    fn api_key_for_provider_resolves_environment_and_literal_values_like_pi() {
        std::env::set_var("PM_AGENT_TEST_DIRECT_API_KEY", "env-api-key-value");
        std::env::remove_var("literal_api_key_value");
        let dir = temp_dir();
        let path = dir.join("models.json");
        fs::write(
            &path,
            format!(
                r#"{{
                  "providers": {{
                    "env-provider": {},
                    "literal-provider": {}
                  }}
                }}"#,
                api_key_provider_config("PM_AGENT_TEST_DIRECT_API_KEY"),
                api_key_provider_config("literal_api_key_value")
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(
            registry.api_key_for_provider("env-provider").as_deref(),
            Some("env-api-key-value")
        );
        assert_eq!(
            registry.api_key_for_provider("literal-provider").as_deref(),
            Some("literal_api_key_value")
        );
        std::env::remove_var("PM_AGENT_TEST_DIRECT_API_KEY");
    }

    #[test]
    fn api_key_for_provider_executes_command_on_every_lookup_like_pi() {
        let dir = temp_dir();
        let counter = dir.join("lookup-counter");
        fs::write(&counter, "0").expect("counter should be written");
        let counter_path = sh_path(&counter);
        let command_config = format!(
            "!sh -c 'count=$(cat \"{counter_path}\"); echo $((count + 1)) > \"{counter_path}\"; echo key-value'"
        );
        let path = dir.join("models.json");
        fs::write(
            &path,
            api_key_models_json("custom-provider", &command_config),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(
            registry.api_key_for_provider("custom-provider").as_deref(),
            Some("key-value")
        );
        assert_eq!(
            registry.api_key_for_provider("custom-provider").as_deref(),
            Some("key-value")
        );
        assert_eq!(
            registry.api_key_for_provider("custom-provider").as_deref(),
            Some("key-value")
        );
        assert_eq!(
            fs::read_to_string(counter).expect("counter should be readable"),
            "3\n"
        );
    }

    #[test]
    fn api_key_for_provider_retries_failed_commands_like_pi() {
        let dir = temp_dir();
        let counter = dir.join("failed-lookup-counter");
        fs::write(&counter, "0").expect("counter should be written");
        let counter_path = sh_path(&counter);
        let command_config = format!(
            "!sh -c 'count=$(cat \"{counter_path}\"); echo $((count + 1)) > \"{counter_path}\"; exit 1'"
        );
        let path = dir.join("models.json");
        fs::write(
            &path,
            api_key_models_json("custom-provider", &command_config),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        assert_eq!(registry.api_key_for_provider("custom-provider"), None);
        assert_eq!(registry.api_key_for_provider("custom-provider"), None);
        assert_eq!(
            fs::read_to_string(counter).expect("counter should be readable"),
            "2\n"
        );
    }

    #[test]
    fn provider_auth_status_reports_environment_api_key_without_executing_commands_like_pi() {
        std::env::set_var("PM_AGENT_TEST_STATUS_API_KEY", "status-key");
        let dir = temp_dir();
        let env_path = dir.join("models-env.json");
        fs::write(
            &env_path,
            r#"{
              "providers": {
                "demo": {
                  "apiKey": "PM_AGENT_TEST_STATUS_API_KEY",
                  "api": "openai-chat-completions",
                  "baseUrl": "https://example.com/v1",
                  "models": [
                    {"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}
                  ]
                }
              }
            }"#,
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, env_path);
        let status = registry.provider_auth_status("demo");
        assert_eq!(status.source, Some(AuthSource::Environment));
        assert_eq!(
            status.label.as_deref(),
            Some("PM_AGENT_TEST_STATUS_API_KEY")
        );
        std::env::remove_var("PM_AGENT_TEST_STATUS_API_KEY");

        let counter = dir.join("status-counter");
        fs::write(&counter, "0").expect("counter should be written");
        let command_path = sh_path(&counter);
        let command_config = format!("!sh -c 'echo 1 > \"{command_path}\"; echo key-value'");
        let command_models_path = dir.join("models-command-status.json");
        fs::write(
            &command_models_path,
            format!(
                r#"{{
                  "providers": {{
                    "demo": {{
                      "apiKey": {command_config:?},
                      "api": "openai-chat-completions",
                      "baseUrl": "https://example.com/v1",
                      "models": [
                        {{"id": "demo-1", "name": "Demo 1", "contextWindow": 4096}}
                      ]
                    }}
                  }}
                }}"#
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, command_models_path);
        assert_eq!(
            registry.provider_auth_status("demo").source,
            Some(AuthSource::ModelsJsonCommand)
        );
        assert_eq!(
            fs::read_to_string(counter).expect("counter should be readable"),
            "0"
        );
    }

    #[test]
    fn get_available_does_not_execute_command_backed_api_key_like_pi() {
        let dir = temp_dir();
        let counter = dir.join("available-counter");
        fs::write(&counter, "0").expect("counter should be written");
        let counter_path = sh_path(&counter);
        let command_config = format!(
            "!sh -c 'count=$(cat \"{counter_path}\"); echo $((count + 1)) > \"{counter_path}\"; echo key-value'"
        );
        let path = dir.join("models.json");
        fs::write(
            &path,
            format!(
                r#"{{
                  "providers": {{
                    "custom-provider": {{
                      "apiKey": {command_config:?},
                      "api": "openai-completions",
                      "baseUrl": "https://example.com/v1",
                      "models": [
                        {{"id": "test-model", "name": "Test Model", "contextWindow": 4096}}
                      ]
                    }}
                  }}
                }}"#
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);

        let available = registry.get_available();

        assert!(available
            .iter()
            .any(|model| model.provider == "custom-provider"));
        assert_eq!(
            fs::read_to_string(counter).expect("counter should be readable"),
            "0"
        );
    }

    #[test]
    fn auth_header_resolves_api_key_on_every_request_like_pi() {
        let dir = temp_dir();
        let token = dir.join("token");
        fs::write(&token, "token-1").expect("token should be written");
        let token_path = sh_path(&token);
        let command_config = format!("!sh -c 'cat \"{token_path}\"'");
        let path = dir.join("models.json");
        fs::write(
            &path,
            format!(
                r#"{{
                  "providers": {{
                    "custom-provider": {{
                      "apiKey": {command_config:?},
                      "api": "openai-completions",
                      "baseUrl": "https://example.com/v1",
                      "authHeader": true,
                      "models": [
                        {{"id": "test-model", "name": "Test Model", "contextWindow": 4096}}
                      ]
                    }}
                  }}
                }}"#
            ),
        )
        .expect("models.json should be written");
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let registry = ModelRegistry::create(storage, path);
        let model = registry
            .find("custom-provider", "test-model")
            .expect("model should exist");

        let first = registry.get_api_key_and_headers(&model);
        fs::write(&token, "token-2").expect("token should be updated");
        let second = registry.get_api_key_and_headers(&model);

        assert!(first.ok);
        assert_eq!(first.api_key.as_deref(), Some("token-1"));
        assert_eq!(
            first
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer token-1")
        );
        assert!(second.ok);
        assert_eq!(second.api_key.as_deref(), Some("token-2"));
        assert_eq!(
            second
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer token-2")
        );
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-model-registry-test-{id}-{counter}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn sh_path(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('"', "\\\"")
    }

    fn api_key_models_json(provider: &str, api_key: &str) -> String {
        format!(
            r#"{{
              "providers": {{
                {provider:?}: {}
              }}
            }}"#,
            api_key_provider_config(api_key)
        )
    }

    fn api_key_provider_config(api_key: &str) -> String {
        format!(
            r#"{{
              "apiKey": {api_key:?},
              "api": "openai-completions",
              "baseUrl": "https://example.com/v1",
              "models": [
                {{"id": "test-model", "name": "Test Model", "contextWindow": 4096}}
              ]
            }}"#
        )
    }
}
