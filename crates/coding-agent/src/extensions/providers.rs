use super::types::{ProviderConfig, ProviderModelConfig};
use crate::model_registry::{ModelDefinition, ProviderConfig as ModelProviderConfig};

pub fn to_model_provider_config(config: ProviderConfig) -> ModelProviderConfig {
    ModelProviderConfig {
        name: config.display_name.clone(),
        api_key: None,
        api: None,
        base_url: None,
        headers: None,
        auth_header: None,
        models: Some(config.models.into_iter().map(to_model_definition).collect()),
        model_overrides: None,
    }
}

fn to_model_definition(model: ProviderModelConfig) -> ModelDefinition {
    ModelDefinition {
        id: model.id,
        name: model.display_name.clone(),
        display_name: model.display_name,
        api: model.api,
        base_url: None,
        reasoning: None,
        thinking_level_map: None,
        input: None,
        cost: None,
        context_window: None,
        max_tokens: None,
        headers: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_extension_provider_config_to_model_registry_config() {
        let config = to_model_provider_config(ProviderConfig {
            display_name: Some("Demo".to_string()),
            models: vec![ProviderModelConfig {
                id: "demo-1".to_string(),
                display_name: Some("Demo 1".to_string()),
                api: Some("openai".to_string()),
            }],
        });

        assert_eq!(config.name.as_deref(), Some("Demo"));
        let models = config.models.expect("models");
        assert_eq!(models[0].id, "demo-1");
        assert_eq!(models[0].display_name.as_deref(), Some("Demo 1"));
        assert_eq!(models[0].api.as_deref(), Some("openai"));
    }
}
