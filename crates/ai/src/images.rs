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
    use crate::types::{ContentBlock, ImagesStopReason, ModelCost, ModelInputKind};

    #[test]
    fn missing_openrouter_key_returns_error_image_response_like_pi() {
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

        let response = generate_images(
            model,
            ImagesContext {
                input: vec![ContentBlock::Text {
                    text: "generate".to_string(),
                }],
            },
            &providers,
        )
        .expect("provider errors should be represented in AssistantImages");

        assert!(response.output.is_empty());
        assert!(matches!(response.stop_reason, ImagesStopReason::Error));
        assert!(response
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("OPENROUTER_API_KEY")));
    }
}
