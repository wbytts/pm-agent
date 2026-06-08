use super::cli::resolve_cli_model;
use super::defaults::preferred_default_model;
use super::types::{CodingModelRegistry, InitialModelResult, ScopedModel};
use ai::{Model, ModelThinkingLevel};

pub fn find_initial_model(
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    scoped_models: &[ScopedModel],
    is_continuing: bool,
    default_provider: Option<&str>,
    default_model_id: Option<&str>,
    default_thinking_level: Option<ModelThinkingLevel>,
    model_registry: &impl CodingModelRegistry,
) -> InitialModelResult {
    let default_thinking = default_thinking_level.unwrap_or(ModelThinkingLevel::Off);
    if cli_provider.is_some() && cli_model.is_some() {
        let resolved = resolve_cli_model(cli_provider, cli_model, model_registry);
        if let Some(model) = resolved.model {
            return InitialModelResult {
                model: Some(model),
                thinking_level: ModelThinkingLevel::Off,
                fallback_message: None,
            };
        }
    }

    if !scoped_models.is_empty() && !is_continuing {
        let first = &scoped_models[0];
        return InitialModelResult {
            model: Some(first.model.clone()),
            thinking_level: first.thinking_level.unwrap_or(default_thinking),
            fallback_message: None,
        };
    }

    if let (Some(provider), Some(model_id)) = (default_provider, default_model_id) {
        if let Some(model) = model_registry.find(provider, model_id) {
            return InitialModelResult {
                model: Some(model),
                thinking_level: default_thinking,
                fallback_message: None,
            };
        }
    }

    let available_models = model_registry.available_models();
    if let Some(model) = preferred_default_model(&available_models) {
        return InitialModelResult {
            model: Some(model),
            thinking_level: ModelThinkingLevel::Off,
            fallback_message: None,
        };
    }

    InitialModelResult {
        model: None,
        thinking_level: ModelThinkingLevel::Off,
        fallback_message: None,
    }
}

pub fn restore_model_from_session(
    saved_provider: &str,
    saved_model_id: &str,
    current_model: Option<Model>,
    model_registry: &impl CodingModelRegistry,
) -> InitialModelResult {
    let restored_model = model_registry.find(saved_provider, saved_model_id);
    if let Some(model) = restored_model.as_ref() {
        if model_registry.has_configured_auth(model) {
            return InitialModelResult {
                model: Some(model.clone()),
                thinking_level: ModelThinkingLevel::Off,
                fallback_message: None,
            };
        }
    }

    let reason = if restored_model.is_none() {
        "model no longer exists"
    } else {
        "no auth configured"
    };
    if let Some(model) = current_model {
        return InitialModelResult {
            fallback_message: Some(format!(
                "Could not restore model {saved_provider}/{saved_model_id} ({reason}). Using {}/{}.",
                model.provider, model.id
            )),
            model: Some(model),
            thinking_level: ModelThinkingLevel::Off,
        };
    }

    let available_models = model_registry.available_models();
    if let Some(model) = preferred_default_model(&available_models) {
        return InitialModelResult {
            fallback_message: Some(format!(
                "Could not restore model {saved_provider}/{saved_model_id} ({reason}). Using {}/{}.",
                model.provider, model.id
            )),
            model: Some(model),
            thinking_level: ModelThinkingLevel::Off,
        };
    }

    InitialModelResult {
        model: None,
        thinking_level: ModelThinkingLevel::Off,
        fallback_message: None,
    }
}
