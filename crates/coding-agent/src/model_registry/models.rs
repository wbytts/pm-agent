use super::config::{
    ModelDefinition, ModelOverride, ModelsConfig, ProviderConfig, ProviderOverride,
};
use ai::{
    Model, ModelCost, ModelInputKind, ModelReasoning, ModelRegistry as AiModelRegistry,
    ModelThinkingLevel,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate_config(config: &ModelsConfig) -> Result<(), String> {
    let built_in_providers = AiModelRegistry::builtins()
        .providers()
        .into_iter()
        .collect::<BTreeSet<_>>();
    for (provider, provider_config) in &config.providers {
        let models = provider_config.models.as_deref().unwrap_or_default();
        let has_model_overrides = provider_config
            .model_overrides
            .as_ref()
            .is_some_and(|overrides| !overrides.is_empty());
        if models.is_empty()
            && provider_config.headers.is_none()
            && provider_config.name.is_none()
            && !has_model_overrides
        {
            return Err(format!(
                "Provider {provider}: must specify \"name\", \"headers\", \"modelOverrides\", or \"models\"."
            ));
        }
        if !built_in_providers.contains(provider)
            && !models.is_empty()
            && provider_config.api_key.is_none()
        {
            return Err(format!(
                "Provider {provider}: \"apiKey\" is required when defining custom models."
            ));
        }
        for model in models {
            if model.id.trim().is_empty() {
                return Err(format!("Provider {provider}: model missing \"id\""));
            }
            if provider_config.api.is_none()
                && model.api.is_none()
                && !built_in_providers.contains(provider)
            {
                return Err(format!(
                    "Provider {provider}, model {}: no \"api\" specified.",
                    model.id
                ));
            }
            if model.context_window == Some(0) {
                return Err(format!(
                    "Provider {provider}, model {}: invalid contextWindow",
                    model.id
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn parse_models(config: &ModelsConfig) -> Vec<Model> {
    let builtins = AiModelRegistry::builtins();
    let built_in_providers = builtins.providers().into_iter().collect::<BTreeSet<_>>();
    let mut models = Vec::new();
    for (provider, provider_config) in &config.providers {
        let Some(model_defs) = &provider_config.models else {
            continue;
        };
        for model_def in model_defs {
            let fallback = built_in_providers
                .contains(provider)
                .then(|| builtins.models(provider).into_iter().next())
                .flatten();
            let api = model_def
                .api
                .clone()
                .or_else(|| provider_config.api.clone())
                .or_else(|| fallback.as_ref().map(|model| model.api.clone()));
            if api.is_none() {
                continue;
            }
            let mut model = model_from_definition(provider, provider_config, model_def);
            if let Some(api) = api {
                model.api = api;
            }
            models.push(model);
        }
    }
    models
}

pub(super) fn model_from_definition(
    provider: &str,
    provider_config: &ProviderConfig,
    model_def: &ModelDefinition,
) -> Model {
    Model {
        id: model_def.id.clone(),
        provider: provider.to_string(),
        api: model_def
            .api
            .clone()
            .or_else(|| provider_config.api.clone())
            .unwrap_or_default(),
        display_name: model_def
            .display_name
            .clone()
            .or_else(|| model_def.name.clone())
            .unwrap_or_else(|| model_def.id.clone()),
        base_url: model_def
            .base_url
            .clone()
            .or_else(|| provider_config.base_url.clone()),
        context_window: model_def.context_window.unwrap_or(128_000),
        max_tokens: model_def.max_tokens,
        input: model_def
            .input
            .clone()
            .unwrap_or_else(|| vec![ModelInputKind::Text]),
        headers: model_def.headers.clone().unwrap_or_default(),
        cost: model_def.cost.clone().unwrap_or_default(),
        reasoning: model_def
            .reasoning
            .map(|enabled| ModelReasoning { enabled }),
        thinking_level_map: parse_thinking_level_map(model_def.thinking_level_map.as_ref()),
        compat: Default::default(),
    }
}

pub(super) fn merge_custom_models(
    mut builtins: Vec<Model>,
    custom_models: Vec<Model>,
) -> Vec<Model> {
    for custom_model in custom_models {
        if let Some(index) = builtins.iter().position(|model| {
            model.provider == custom_model.provider && model.id == custom_model.id
        }) {
            builtins[index] = custom_model;
        } else {
            builtins.push(custom_model);
        }
    }
    builtins
}

pub(super) fn apply_provider_override(
    mut model: Model,
    provider_override: &ProviderOverride,
) -> Model {
    if let Some(display_name) = &provider_override.display_name {
        model.display_name = display_name.clone();
    }
    if provider_override.base_url.is_some() {
        model.base_url = provider_override.base_url.clone();
    }
    model
}

pub(super) fn apply_model_override(mut model: Model, model_override: &ModelOverride) -> Model {
    if let Some(name) = model_override
        .display_name
        .clone()
        .or_else(|| model_override.name.clone())
    {
        model.display_name = name;
    }
    if let Some(reasoning) = model_override.reasoning {
        model.reasoning = Some(ModelReasoning { enabled: reasoning });
    }
    if let Some(map) = &model_override.thinking_level_map {
        model
            .thinking_level_map
            .extend(parse_thinking_level_map(Some(map)));
    }
    if let Some(context_window) = model_override.context_window {
        model.context_window = context_window;
    }
    if let Some(max_tokens) = model_override.max_tokens {
        model.max_tokens = Some(max_tokens);
    }
    if let Some(input) = &model_override.input {
        model.input = input.clone();
    }
    if let Some(headers) = &model_override.headers {
        model.headers.extend(headers.clone());
    }
    if let Some(cost) = &model_override.cost {
        model.cost = ModelCost {
            input: cost.input.unwrap_or(model.cost.input),
            output: cost.output.unwrap_or(model.cost.output),
            cache_read: cost.cache_read.unwrap_or(model.cost.cache_read),
            cache_write: cost.cache_write.unwrap_or(model.cost.cache_write),
        };
    }
    model
}

fn parse_thinking_level_map(
    value: Option<&BTreeMap<String, Option<String>>>,
) -> BTreeMap<ModelThinkingLevel, Option<String>> {
    value
        .into_iter()
        .flat_map(|map| map.iter())
        .filter_map(|(key, value)| parse_thinking_level(key).map(|level| (level, value.clone())))
        .collect()
}

fn parse_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    Some(match value {
        "off" => ModelThinkingLevel::Off,
        "minimal" => ModelThinkingLevel::Minimal,
        "low" => ModelThinkingLevel::Low,
        "medium" => ModelThinkingLevel::Medium,
        "high" => ModelThinkingLevel::High,
        "xhigh" => ModelThinkingLevel::XHigh,
        _ => return None,
    })
}
