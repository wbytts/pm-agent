use super::defaults::{build_fallback_model, canonical_provider};
use super::pattern::parse_model_pattern;
use super::types::{CodingModelRegistry, ResolveCliModelResult};

pub fn resolve_cli_model(
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    model_registry: &impl CodingModelRegistry,
) -> ResolveCliModelResult {
    let Some(cli_model) = cli_model else {
        return ResolveCliModelResult {
            model: None,
            thinking_level: None,
            warning: None,
            error: None,
        };
    };

    let available_models = model_registry.all_models();
    if available_models.is_empty() {
        return ResolveCliModelResult {
            model: None,
            thinking_level: None,
            warning: None,
            error: Some(
                "No models available. Check your installation or add models to models.json."
                    .to_string(),
            ),
        };
    }

    let mut provider =
        cli_provider.and_then(|provider| canonical_provider(provider, &available_models));
    if cli_provider.is_some() && provider.is_none() {
        return ResolveCliModelResult {
            model: None,
            thinking_level: None,
            warning: None,
            error: Some(format!(
                "Unknown provider \"{}\". Use --list-models to see available providers/models.",
                cli_provider.unwrap_or_default()
            )),
        };
    }

    let mut pattern = cli_model.to_string();
    let mut inferred_provider = false;
    if provider.is_none() {
        if let Some((maybe_provider, model_id)) = cli_model.split_once('/') {
            if let Some(canonical) = canonical_provider(maybe_provider, &available_models) {
                provider = Some(canonical);
                pattern = model_id.to_string();
                inferred_provider = true;
            }
        }
    }

    if provider.is_none() {
        let lower = cli_model.to_lowercase();
        if let Some(exact) = available_models.iter().find(|model| {
            model.id.to_lowercase() == lower
                || format!("{}/{}", model.provider, model.id).to_lowercase() == lower
        }) {
            return ResolveCliModelResult {
                model: Some(exact.clone()),
                thinking_level: None,
                warning: None,
                error: None,
            };
        }
    }

    if let Some(provider) = &provider {
        let prefix = format!("{provider}/");
        if cli_provider.is_some() && cli_model.to_lowercase().starts_with(&prefix.to_lowercase()) {
            pattern = cli_model[prefix.len()..].to_string();
        }
    }

    let candidates = provider.as_ref().map_or_else(
        || available_models.clone(),
        |provider| {
            available_models
                .iter()
                .filter(|model| model.provider == *provider)
                .cloned()
                .collect::<Vec<_>>()
        },
    );
    let parsed = parse_model_pattern(&pattern, &candidates, false);
    if let Some(model) = parsed.model {
        return ResolveCliModelResult {
            model: Some(model),
            thinking_level: parsed.thinking_level,
            warning: parsed.warning,
            error: None,
        };
    }

    if inferred_provider {
        let fallback = parse_model_pattern(cli_model, &available_models, false);
        if fallback.model.is_some() {
            return ResolveCliModelResult {
                model: fallback.model,
                thinking_level: fallback.thinking_level,
                warning: fallback.warning,
                error: None,
            };
        }
    }

    if let Some(provider) = &provider {
        if let Some(fallback_model) = build_fallback_model(provider, &pattern, &available_models) {
            let warning = parsed.warning.map_or_else(
                || format!("Model \"{pattern}\" not found for provider \"{provider}\". Using custom model id."),
                |warning| format!("{warning} Model \"{pattern}\" not found for provider \"{provider}\". Using custom model id."),
            );
            return ResolveCliModelResult {
                model: Some(fallback_model),
                thinking_level: None,
                warning: Some(warning),
                error: None,
            };
        }
    }

    let display = provider.as_ref().map_or_else(
        || cli_model.to_string(),
        |provider| format!("{provider}/{pattern}"),
    );
    ResolveCliModelResult {
        model: None,
        thinking_level: None,
        warning: parsed.warning,
        error: Some(format!(
            "Model \"{display}\" not found. Use --list-models to see available models."
        )),
    }
}
