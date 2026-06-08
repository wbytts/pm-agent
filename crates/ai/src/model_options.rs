use crate::types::{Model, ModelThinkingLevel, Usage, UsageCost};

const THINKING_LEVELS: [ModelThinkingLevel; 6] = [
    ModelThinkingLevel::Off,
    ModelThinkingLevel::Minimal,
    ModelThinkingLevel::Low,
    ModelThinkingLevel::Medium,
    ModelThinkingLevel::High,
    ModelThinkingLevel::XHigh,
];

pub fn calculate_cost(model: &Model, usage: &Usage) -> UsageCost {
    let input = (model.cost.input / 1_000_000.0) * usage.input as f64;
    let output = (model.cost.output / 1_000_000.0) * usage.output as f64;
    let cache_read = (model.cost.cache_read / 1_000_000.0) * usage.cache_read as f64;
    let cache_write = (model.cost.cache_write / 1_000_000.0) * usage.cache_write as f64;

    UsageCost {
        input,
        output,
        cache_read,
        cache_write,
        total: input + output + cache_read + cache_write,
    }
}

pub fn supported_thinking_levels(model: &Model) -> Vec<ModelThinkingLevel> {
    if !model
        .reasoning
        .as_ref()
        .is_some_and(|reasoning| reasoning.enabled)
    {
        return vec![ModelThinkingLevel::Off];
    }

    THINKING_LEVELS
        .into_iter()
        .filter(|level| {
            if model
                .thinking_level_map
                .get(level)
                .is_some_and(Option::is_none)
            {
                return false;
            }
            if *level == ModelThinkingLevel::XHigh {
                return model.thinking_level_map.contains_key(level);
            }
            true
        })
        .collect()
}

pub fn clamp_thinking_level(model: &Model, level: ModelThinkingLevel) -> ModelThinkingLevel {
    let available = supported_thinking_levels(model);
    if available.contains(&level) {
        return level;
    }

    let requested_index = THINKING_LEVELS
        .iter()
        .position(|candidate| *candidate == level)
        .unwrap_or(0);
    for candidate in THINKING_LEVELS.iter().skip(requested_index) {
        if available.contains(candidate) {
            return *candidate;
        }
    }
    for candidate in THINKING_LEVELS.iter().take(requested_index).rev() {
        if available.contains(candidate) {
            return *candidate;
        }
    }
    available
        .first()
        .copied()
        .unwrap_or(ModelThinkingLevel::Off)
}

pub fn models_are_equal(a: Option<&Model>, b: Option<&Model>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    a.id == b.id && a.provider == b.provider
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelCost, ModelReasoning};

    #[test]
    fn calculates_model_cost_per_million_tokens() {
        let mut model = model();
        model.cost = ModelCost {
            input: 2.0,
            output: 10.0,
            cache_read: 0.5,
            cache_write: 1.0,
        };
        let cost = calculate_cost(
            &model,
            &Usage {
                input: 1_000_000,
                output: 500_000,
                cache_read: 2_000_000,
                cache_write: 1_000_000,
                total_tokens: 4_500_000,
                cost: UsageCost::default(),
            },
        );

        assert_eq!(cost.total, 9.0);
    }

    #[test]
    fn clamps_unsupported_thinking_levels() {
        let mut model = model();
        assert_eq!(
            supported_thinking_levels(&model),
            vec![ModelThinkingLevel::Off]
        );

        model.reasoning = Some(ModelReasoning { enabled: true });
        model
            .thinking_level_map
            .insert(ModelThinkingLevel::Medium, None);

        assert!(!supported_thinking_levels(&model).contains(&ModelThinkingLevel::Medium));
        assert_eq!(
            clamp_thinking_level(&model, ModelThinkingLevel::Medium),
            ModelThinkingLevel::High
        );
    }

    #[test]
    fn compares_models_by_id_and_provider_like_pi_models_are_equal() {
        let mut first = model();
        first.id = "same".to_string();
        first.provider = "provider-a".to_string();
        first.api = "api-a".to_string();

        let mut same_identity = first.clone();
        same_identity.api = "different-api".to_string();
        same_identity.display_name = "Different display".to_string();

        let mut different_provider = first.clone();
        different_provider.provider = "provider-b".to_string();

        let mut different_id = first.clone();
        different_id.id = "different".to_string();

        assert!(models_are_equal(Some(&first), Some(&same_identity)));
        assert!(!models_are_equal(Some(&first), Some(&different_provider)));
        assert!(!models_are_equal(Some(&first), Some(&different_id)));
        assert!(!models_are_equal(Some(&first), None));
        assert!(!models_are_equal(None, Some(&first)));
    }

    fn model() -> Model {
        Model {
            id: "test".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Test".to_string(),
            context_window: 1,
            cost: ModelCost::default(),
            reasoning: None,
            thinking_level_map: Default::default(),
            ..Model::default()
        }
    }
}
