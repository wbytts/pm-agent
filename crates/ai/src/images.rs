use crate::image_registry::ImagesProviderRegistry;
use crate::types::{AiResult, AssistantImages, ImagesContext, ImagesModel, ImagesProvider};

pub fn generate_images(
    model: ImagesModel,
    context: ImagesContext,
    providers: &ImagesProviderRegistry,
) -> AiResult<AssistantImages> {
    let provider = providers.provider_for(&model)?;
    provider.generate_images(model, context)
}

pub fn generate_images_with_builtins(
    model: ImagesModel,
    context: ImagesContext,
) -> AiResult<AssistantImages> {
    let providers = ImagesProviderRegistry::builtins();
    generate_images(model, context, &providers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_registry::RegisteredImagesProvider;
    use crate::providers::{OpenRouterImagesConfig, OpenRouterImagesProvider};
    use crate::types::{ContentBlock, ModelCost, ModelInputKind};

    #[test]
    fn missing_openrouter_key_fails_before_network() {
        let model = ImagesModel {
            id: "openrouter/auto".to_string(),
            provider: "openrouter".to_string(),
            api: "openrouter-images".to_string(),
            display_name: "Auto Router".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            input: vec![ModelInputKind::Text],
            output: vec![ModelInputKind::Image],
            headers: Default::default(),
            cost: ModelCost::default(),
        };
        let mut providers = ImagesProviderRegistry::new();
        providers.register(
            RegisteredImagesProvider::OpenRouter(OpenRouterImagesProvider::new(
                OpenRouterImagesConfig {
                    api_key: Some(String::new()),
                },
            )),
            None,
        );

        let error = generate_images(
            model,
            ImagesContext {
                input: vec![ContentBlock::Text {
                    text: "generate".to_string(),
                }],
            },
            &providers,
        )
        .expect_err("missing key should fail first");
        assert!(error.to_string().contains("OPENROUTER_API_KEY"));
    }
}
