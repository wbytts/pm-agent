use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::image_model_catalog::builtin_image_models;
use crate::providers::OpenRouterImagesProvider;
use crate::types::{
    validate_images_model, AiError, AiResult, AssistantImages, ImagesContext, ImagesModel,
    ImagesProvider,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesApiProviderInfo {
    pub api: String,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RegisteredImagesProvider {
    OpenRouter(OpenRouterImagesProvider),
}

impl RegisteredImagesProvider {
    pub fn api(&self) -> &'static str {
        match self {
            RegisteredImagesProvider::OpenRouter(_) => "openrouter-images",
        }
    }
}

impl ImagesProvider for RegisteredImagesProvider {
    fn generate_images(
        &self,
        model: ImagesModel,
        context: ImagesContext,
    ) -> AiResult<AssistantImages> {
        if model.api != self.api() {
            return Err(AiError::MismatchedApi {
                actual: model.api,
                expected: self.api().to_string(),
            });
        }
        match self {
            RegisteredImagesProvider::OpenRouter(provider) => {
                provider.generate_images(model, context)
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImagesProviderRegistry {
    providers: BTreeMap<String, (RegisteredImagesProvider, Option<String>)>,
}

impl ImagesProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::new();
        registry.register(
            RegisteredImagesProvider::OpenRouter(OpenRouterImagesProvider::from_env()),
            None,
        );
        registry
    }

    pub fn register(&mut self, provider: RegisteredImagesProvider, source_id: Option<String>) {
        self.providers
            .insert(provider.api().to_string(), (provider, source_id));
    }

    pub fn get(&self, api: &str) -> Option<RegisteredImagesProvider> {
        self.providers
            .get(api)
            .map(|(provider, _)| provider.clone())
    }

    pub fn provider_for(&self, model: &ImagesModel) -> AiResult<RegisteredImagesProvider> {
        validate_images_model(model)?;
        self.get(&model.api)
            .ok_or_else(|| AiError::UnknownApi(model.api.clone()))
    }

    pub fn list(&self) -> Vec<ImagesApiProviderInfo> {
        self.providers
            .iter()
            .map(|(api, (_, source_id))| ImagesApiProviderInfo {
                api: api.clone(),
                source_id: source_id.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImagesModelRegistry {
    models: BTreeMap<String, BTreeMap<String, ImagesModel>>,
}

impl ImagesModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::new();
        for model in builtin_image_models() {
            registry.register(model);
        }
        registry
    }

    pub fn register(&mut self, model: ImagesModel) {
        self.models
            .entry(model.provider.clone())
            .or_default()
            .insert(model.id.clone(), model);
    }

    pub fn get(&self, provider: &str, id: &str) -> Option<ImagesModel> {
        self.models
            .get(provider)
            .and_then(|models| models.get(id))
            .cloned()
    }

    pub fn providers(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }

    pub fn models(&self, provider: &str) -> Vec<ImagesModel> {
        self.models
            .get(provider)
            .map(|models| models.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn all_models(&self) -> Vec<ImagesModel> {
        self.models
            .values()
            .flat_map(|models| models.values().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_expose_openrouter_images() {
        let models = ImagesModelRegistry::builtins();
        let providers = ImagesProviderRegistry::builtins();
        let model = models
            .get("openrouter", "openrouter/auto")
            .expect("image model should exist");
        assert_eq!(
            providers.provider_for(&model).expect("provider").api(),
            "openrouter-images"
        );
    }
}
