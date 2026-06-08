use super::types::{models_are_equal, ParsedModelResult, ScopedModel};
use ai::{Model, ModelThinkingLevel};

pub fn find_exact_model_reference_match(
    model_reference: &str,
    available_models: &[Model],
) -> Option<Model> {
    let trimmed = model_reference.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.to_lowercase();

    let canonical_matches = available_models
        .iter()
        .filter(|model| format!("{}/{}", model.provider, model.id).to_lowercase() == normalized)
        .cloned()
        .collect::<Vec<_>>();
    if canonical_matches.len() == 1 {
        return canonical_matches.into_iter().next();
    }
    if canonical_matches.len() > 1 {
        return None;
    }

    if let Some((provider, model_id)) = trimmed.split_once('/') {
        if !provider.trim().is_empty() && !model_id.trim().is_empty() {
            let provider_matches = available_models
                .iter()
                .filter(|model| {
                    model.provider.eq_ignore_ascii_case(provider.trim())
                        && model.id.eq_ignore_ascii_case(model_id.trim())
                })
                .cloned()
                .collect::<Vec<_>>();
            if provider_matches.len() == 1 {
                return provider_matches.into_iter().next();
            }
            if provider_matches.len() > 1 {
                return None;
            }
        }
    }

    let id_matches = available_models
        .iter()
        .filter(|model| model.id.to_lowercase() == normalized)
        .cloned()
        .collect::<Vec<_>>();
    (id_matches.len() == 1).then(|| id_matches[0].clone())
}

pub fn parse_model_pattern(
    pattern: &str,
    available_models: &[Model],
    allow_invalid_thinking_level_fallback: bool,
) -> ParsedModelResult {
    if let Some(model) = try_match_model(pattern, available_models) {
        return ParsedModelResult {
            model: Some(model),
            thinking_level: None,
            warning: None,
        };
    }

    let Some(last_colon_index) = pattern.rfind(':') else {
        return ParsedModelResult {
            model: None,
            thinking_level: None,
            warning: None,
        };
    };
    let prefix = &pattern[..last_colon_index];
    let suffix = &pattern[last_colon_index + 1..];

    if let Some(thinking_level) = parse_thinking_level(suffix) {
        let result = parse_model_pattern(
            prefix,
            available_models,
            allow_invalid_thinking_level_fallback,
        );
        if result.model.is_some() {
            let has_warning = result.warning.is_some();
            return ParsedModelResult {
                thinking_level: if has_warning {
                    None
                } else {
                    Some(thinking_level)
                },
                ..result
            };
        }
        return result;
    }

    if !allow_invalid_thinking_level_fallback {
        return ParsedModelResult {
            model: None,
            thinking_level: None,
            warning: None,
        };
    }

    let result = parse_model_pattern(
        prefix,
        available_models,
        allow_invalid_thinking_level_fallback,
    );
    if result.model.is_some() {
        return ParsedModelResult {
            model: result.model,
            thinking_level: None,
            warning: Some(format!(
                "Invalid thinking level \"{suffix}\" in pattern \"{pattern}\". Using default instead."
            )),
        };
    }
    result
}

pub fn resolve_model_scope(patterns: &[String], available_models: &[Model]) -> Vec<ScopedModel> {
    let mut scoped_models = Vec::new();

    for pattern in patterns {
        if contains_glob(pattern) {
            let (glob_pattern, thinking_level) = split_optional_thinking_suffix(pattern);
            for model in available_models {
                let full_id = format!("{}/{}", model.provider, model.id);
                if (glob_match(&full_id, &glob_pattern) || glob_match(&model.id, &glob_pattern))
                    && !scoped_models
                        .iter()
                        .any(|item: &ScopedModel| models_are_equal(&item.model, model))
                {
                    scoped_models.push(ScopedModel {
                        model: model.clone(),
                        thinking_level,
                    });
                }
            }
            continue;
        }

        let parsed = parse_model_pattern(pattern, available_models, true);
        if let Some(model) = parsed.model {
            if !scoped_models
                .iter()
                .any(|item: &ScopedModel| models_are_equal(&item.model, &model))
            {
                scoped_models.push(ScopedModel {
                    model,
                    thinking_level: parsed.thinking_level,
                });
            }
        }
    }

    scoped_models
}

pub(super) fn try_match_model(model_pattern: &str, available_models: &[Model]) -> Option<Model> {
    if let Some(exact_match) = find_exact_model_reference_match(model_pattern, available_models) {
        return Some(exact_match);
    }

    let lower = model_pattern.to_lowercase();
    let mut matches = available_models
        .iter()
        .filter(|model| {
            model.id.to_lowercase().contains(&lower)
                || model.display_name.to_lowercase().contains(&lower)
        })
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }

    matches.sort_by(|a, b| {
        let a_alias = is_alias(&a.id);
        let b_alias = is_alias(&b.id);
        b_alias.cmp(&a_alias).then_with(|| b.id.cmp(&a.id))
    });
    matches.into_iter().next()
}

fn is_alias(id: &str) -> bool {
    id.ends_with("-latest") || !has_date_suffix(id)
}

fn has_date_suffix(id: &str) -> bool {
    let Some(suffix) = id.rsplit('-').next() else {
        return false;
    };
    suffix.len() == 8 && suffix.chars().all(|character| character.is_ascii_digit())
}

pub(super) fn parse_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn split_optional_thinking_suffix(pattern: &str) -> (String, Option<ModelThinkingLevel>) {
    let Some(index) = pattern.rfind(':') else {
        return (pattern.to_string(), None);
    };
    let suffix = &pattern[index + 1..];
    if let Some(thinking_level) = parse_thinking_level(suffix) {
        (pattern[..index].to_string(), Some(thinking_level))
    } else {
        (pattern.to_string(), None)
    }
}

fn contains_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn glob_match(value: &str, pattern: &str) -> bool {
    glob_match_chars(
        &value.to_lowercase().chars().collect::<Vec<_>>(),
        &pattern.to_lowercase().chars().collect::<Vec<_>>(),
    )
}

fn glob_match_chars(value: &[char], pattern: &[char]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    match pattern[0] {
        '*' => {
            glob_match_chars(value, &pattern[1..])
                || (!value.is_empty() && glob_match_chars(&value[1..], pattern))
        }
        '?' => !value.is_empty() && glob_match_chars(&value[1..], &pattern[1..]),
        character => {
            !value.is_empty()
                && value[0] == character
                && glob_match_chars(&value[1..], &pattern[1..])
        }
    }
}
