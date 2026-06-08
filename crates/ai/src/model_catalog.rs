use serde::Deserialize;
use std::collections::BTreeMap;

use crate::types::{
    Model, ModelCost, ModelInputKind, ModelReasoning, ModelThinkingLevel, ThinkingLevelMap,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    id: String,
    name: String,
    api: String,
    provider: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    thinking_level_map: BTreeMap<String, Option<String>>,
    #[serde(default)]
    compat: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    cost: ModelCost,
    context_window: usize,
    #[serde(default)]
    max_tokens: Option<usize>,
}

pub fn builtin_models() -> Vec<Model> {
    let catalog = serde_json::from_str::<BTreeMap<String, BTreeMap<String, CatalogModel>>>(
        include_str!("catalog/models.json"),
    )
    .expect("内置模型 catalog 必须是合法 JSON");

    catalog
        .into_values()
        .flat_map(|models| models.into_values().map(Model::from))
        .collect()
}

impl From<CatalogModel> for Model {
    fn from(model: CatalogModel) -> Self {
        Self {
            id: model.id,
            provider: model.provider,
            api: model.api,
            display_name: model.name,
            base_url: model.base_url.filter(|value| !value.is_empty()),
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            input: model.input.into_iter().map(input_kind).collect(),
            headers: model.headers,
            cost: model.cost,
            reasoning: model.reasoning.then_some(ModelReasoning { enabled: true }),
            thinking_level_map: thinking_level_map(model.thinking_level_map),
            compat: model.compat,
        }
    }
}

fn input_kind(value: String) -> ModelInputKind {
    match value.as_str() {
        "image" => ModelInputKind::Image,
        _ => ModelInputKind::Text,
    }
}

fn thinking_level_map(values: BTreeMap<String, Option<String>>) -> ThinkingLevelMap {
    values
        .into_iter()
        .filter_map(|(level, mapped)| thinking_level(&level).map(|level| (level, mapped)))
        .collect()
}

fn thinking_level(value: &str) -> Option<ModelThinkingLevel> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_generated_model_catalog() {
        let models = builtin_models();

        assert!(models.len() >= 900);
        assert!(models.iter().any(
            |model| model.provider == "amazon-bedrock" && model.id == "amazon.nova-2-lite-v1:0"
        ));
        assert!(models
            .iter()
            .any(|model| model.provider == "openai" && model.id == "gpt-5.4"));
        assert!(models
            .iter()
            .any(|model| model.provider == "github-copilot" && !model.headers.is_empty()));
    }

    #[test]
    fn marks_exact_anthropic_messages_adaptive_thinking_models_like_pi() {
        let mut flagged_models = builtin_models()
            .into_iter()
            .filter(|model| model.api == "anthropic-messages")
            .filter(|model| {
                model
                    .compat
                    .get("forceAdaptiveThinking")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            })
            .map(|model| format!("{}/{}", model.provider, model.id))
            .collect::<Vec<_>>();
        flagged_models.sort();

        let mut expected = vec![
            "anthropic/claude-opus-4-6",
            "anthropic/claude-opus-4-7",
            "anthropic/claude-sonnet-4-6",
            "cloudflare-ai-gateway/claude-opus-4-6",
            "cloudflare-ai-gateway/claude-opus-4-7",
            "cloudflare-ai-gateway/claude-sonnet-4-6",
            "github-copilot/claude-opus-4.6",
            "github-copilot/claude-opus-4.7",
            "github-copilot/claude-sonnet-4.6",
            "opencode/claude-opus-4-6",
            "opencode/claude-opus-4-7",
            "opencode/claude-sonnet-4-6",
            "vercel-ai-gateway/anthropic/claude-opus-4.6",
            "vercel-ai-gateway/anthropic/claude-opus-4.7",
            "vercel-ai-gateway/anthropic/claude-sonnet-4.6",
        ];
        expected.sort();

        assert_eq!(flagged_models, expected);
    }
}
