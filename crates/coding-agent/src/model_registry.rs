use crate::auth_storage::{AuthSource, AuthStatus, AuthStorage, AuthStorageBackend};
use crate::model_resolver::CodingModelRegistry;
use crate::provider_display_names::built_in_provider_display_name;
use crate::resolve_config_value::{
    clear_config_value_cache, resolve_config_value_or_throw, resolve_config_value_uncached,
    resolve_headers_or_throw,
};
use ai::{Model, ModelRegistry as AiModelRegistry};
use config::{CustomModelsResult, ModelsConfig, ProviderOverride, ProviderRequestConfig};
use json::strip_json_comments;
use models::{
    apply_model_override, apply_provider_override, merge_custom_models, model_from_definition,
    parse_models, validate_config,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

mod config;
mod json;
mod models;

pub use crate::resolve_config_value::clear_config_value_cache as clear_api_key_cache;
pub use config::{ModelDefinition, ModelOverride, PartialModelCost, ProviderConfig};

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
                source: Some(AuthSource::Fallback),
                label: Some("models.json command".to_string()),
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
            source: Some(AuthSource::Fallback),
            label: Some("models.json key".to_string()),
        }
    }

    pub fn provider_display_name(&self, provider: &str) -> String {
        self.registered_providers
            .get(provider)
            .and_then(|config| config.name.clone())
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

    pub fn register_provider(&mut self, provider: impl Into<String>, config: ProviderConfig) {
        let provider = provider.into();
        self.apply_provider_config(&provider, &config);
        self.registered_providers.insert(provider, config);
    }

    pub fn unregister_provider(&mut self, provider: &str) {
        if self.registered_providers.remove(provider).is_some() {
            self.refresh();
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
            if provider_config.name.is_some() || provider_config.base_url.is_some() {
                result.provider_overrides.insert(
                    provider.clone(),
                    ProviderOverride {
                        display_name: provider_config.name.clone(),
                        base_url: provider_config.base_url.clone(),
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
        let api_key = self
            .auth_storage
            .api_key(&model.provider, false)
            .or_else(|| {
                provider_config
                    .and_then(|config| config.api_key.as_deref())
                    .map(|api_key| {
                        resolve_config_value_or_throw(
                            api_key,
                            &format!("API key for provider \"{}\"", model.provider),
                        )
                    })
                    .transpose()
                    .ok()
                    .flatten()
            });
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
        self.store_provider_request_config(provider, config);
        let Some(models) = &config.models else {
            return;
        };
        if models.is_empty() {
            return;
        }
        self.models.retain(|model| model.provider != provider);
        for model_def in models {
            self.store_model_headers(provider, &model_def.id, model_def.headers.clone());
            self.models
                .push(model_from_definition(provider, config, model_def));
        }
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-model-registry-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
