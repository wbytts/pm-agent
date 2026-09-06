use super::types::{ProviderConfig, ProviderModelConfig};
use crate::model_registry::{
    ModelDefinition, ProviderConfig as ModelProviderConfig, StreamSimpleConfig,
};

pub fn to_model_provider_config(config: ProviderConfig) -> ModelProviderConfig {
    ModelProviderConfig {
        name: config.name.or(config.display_name),
        api_key: config.api_key,
        api: config.api,
        base_url: config.base_url,
        headers: config.headers,
        compat: config.compat,
        auth_header: config.auth_header,
        models: config
            .models
            .map(|models| models.into_iter().map(to_model_definition).collect()),
        model_overrides: None,
        oauth: None,
        stream_simple: config.stream_simple.map(|handler| StreamSimpleConfig {
            text: None,
            handler: Some(handler),
        }),
    }
}

fn to_model_definition(model: ProviderModelConfig) -> ModelDefinition {
    ModelDefinition {
        id: model.id,
        name: model.name.or_else(|| model.display_name.clone()),
        display_name: model.display_name,
        api: model.api,
        base_url: model.base_url,
        reasoning: model.reasoning,
        thinking_level_map: model.thinking_level_map,
        input: model.input,
        cost: model.cost,
        context_window: model.context_window,
        max_tokens: model.max_tokens,
        headers: model.headers,
        compat: model.compat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{ModelCost, ModelInputKind};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn converts_extension_provider_config_to_model_registry_config() {
        let config = to_model_provider_config(ProviderConfig {
            name: Some("Demo".to_string()),
            display_name: Some("Demo".to_string()),
            base_url: Some("https://provider.test/v1".to_string()),
            api_key: Some("literal-key".to_string()),
            api: Some("openai-completions".to_string()),
            headers: Some(BTreeMap::from([(
                "x-provider".to_string(),
                "enabled".to_string(),
            )])),
            auth_header: Some(true),
            compat: Some(BTreeMap::from([(
                "supportsUsageInStreaming".to_string(),
                json!(false),
            )])),
            models: Some(vec![ProviderModelConfig {
                id: "demo-1".to_string(),
                name: Some("Demo One".to_string()),
                display_name: Some("Demo 1".to_string()),
                api: Some("anthropic-messages".to_string()),
                base_url: Some("https://model.test/v1".to_string()),
                reasoning: Some(true),
                thinking_level_map: Some(BTreeMap::from([(
                    "high".to_string(),
                    Some("max".to_string()),
                )])),
                input: Some(vec![ModelInputKind::Text, ModelInputKind::Image]),
                cost: Some(ModelCost {
                    input: 1.0,
                    output: 2.0,
                    cache_read: 0.1,
                    cache_write: 0.2,
                }),
                context_window: Some(1234),
                max_tokens: Some(567),
                headers: Some(BTreeMap::from([(
                    "x-model".to_string(),
                    "model".to_string(),
                )])),
                compat: Some(BTreeMap::from([(
                    "cacheControlFormat".to_string(),
                    json!("anthropic"),
                )])),
            }]),
            stream_simple: None,
        });

        assert_eq!(config.name.as_deref(), Some("Demo"));
        assert_eq!(config.api_key.as_deref(), Some("literal-key"));
        assert_eq!(config.api.as_deref(), Some("openai-completions"));
        assert_eq!(config.base_url.as_deref(), Some("https://provider.test/v1"));
        assert_eq!(config.auth_header, Some(true));
        assert_eq!(
            config
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-provider"))
                .map(String::as_str),
            Some("enabled")
        );
        assert_eq!(
            config
                .compat
                .as_ref()
                .and_then(|compat| compat.get("supportsUsageInStreaming")),
            Some(&json!(false))
        );
        let models = config.models.expect("models");
        assert_eq!(models[0].id, "demo-1");
        assert_eq!(models[0].name.as_deref(), Some("Demo One"));
        assert_eq!(models[0].display_name.as_deref(), Some("Demo 1"));
        assert_eq!(models[0].api.as_deref(), Some("anthropic-messages"));
        assert_eq!(models[0].base_url.as_deref(), Some("https://model.test/v1"));
        assert_eq!(models[0].reasoning, Some(true));
        assert_eq!(models[0].context_window, Some(1234));
        assert_eq!(models[0].max_tokens, Some(567));
        assert_eq!(
            models[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-model"))
                .map(String::as_str),
            Some("model")
        );
        assert_eq!(
            models[0]
                .compat
                .as_ref()
                .and_then(|compat| compat.get("cacheControlFormat")),
            Some(&json!("anthropic"))
        );
    }
}
