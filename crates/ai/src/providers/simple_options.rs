use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{Model, ModelThinkingLevel};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SimpleStreamOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub cache_retention: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_retry_delay_ms: Option<u64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub cache_retention: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_retry_delay_ms: Option<u64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingBudgets {
    pub minimal: usize,
    pub low: usize,
    pub medium: usize,
    pub high: usize,
}

impl Default for ThinkingBudgets {
    fn default() -> Self {
        Self {
            minimal: 1024,
            low: 2048,
            medium: 8192,
            high: 16384,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingTokenBudget {
    pub max_tokens: usize,
    pub thinking_budget: usize,
}

pub fn build_base_options(
    _model: &Model,
    options: Option<SimpleStreamOptions>,
    api_key: Option<String>,
) -> StreamOptions {
    let options = options.unwrap_or_default();
    StreamOptions {
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        api_key: api_key.or(options.api_key),
        cache_retention: options.cache_retention,
        session_id: options.session_id,
        headers: options.headers,
        timeout_ms: options.timeout_ms,
        max_retries: options.max_retries,
        max_retry_delay_ms: options.max_retry_delay_ms,
        metadata: options.metadata,
    }
}

pub fn clamp_reasoning(effort: Option<ModelThinkingLevel>) -> Option<ModelThinkingLevel> {
    match effort {
        Some(ModelThinkingLevel::XHigh) => Some(ModelThinkingLevel::High),
        other => other,
    }
}

pub fn adjust_max_tokens_for_thinking(
    base_max_tokens: Option<usize>,
    model_max_tokens: usize,
    reasoning_level: ModelThinkingLevel,
    custom_budgets: Option<ThinkingBudgets>,
) -> ThinkingTokenBudget {
    let budgets = custom_budgets.unwrap_or_default();
    let min_output_tokens = 1024;
    let level = clamp_reasoning(Some(reasoning_level)).unwrap_or(ModelThinkingLevel::Off);
    let mut thinking_budget = thinking_budget_for_level(level, budgets);
    let max_tokens = base_max_tokens
        .map(|base| base.saturating_add(thinking_budget).min(model_max_tokens))
        .unwrap_or(model_max_tokens);

    if max_tokens <= thinking_budget {
        thinking_budget = max_tokens.saturating_sub(min_output_tokens);
    }

    ThinkingTokenBudget {
        max_tokens,
        thinking_budget,
    }
}

fn thinking_budget_for_level(level: ModelThinkingLevel, budgets: ThinkingBudgets) -> usize {
    match level {
        ModelThinkingLevel::Minimal => budgets.minimal,
        ModelThinkingLevel::Low => budgets.low,
        ModelThinkingLevel::Medium => budgets.medium,
        ModelThinkingLevel::High | ModelThinkingLevel::XHigh => budgets.high,
        ModelThinkingLevel::Off => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_base_options_prefers_explicit_api_key() {
        let mut headers = BTreeMap::new();
        headers.insert("x-test".to_string(), "1".to_string());
        let options = SimpleStreamOptions {
            temperature: Some(0.2),
            max_tokens: Some(100),
            api_key: Some("from-options".to_string()),
            headers,
            ..SimpleStreamOptions::default()
        };

        let result = build_base_options(
            &Model::default(),
            Some(options),
            Some("explicit".to_string()),
        );

        assert_eq!(result.temperature, Some(0.2));
        assert_eq!(result.max_tokens, Some(100));
        assert_eq!(result.api_key.as_deref(), Some("explicit"));
        assert_eq!(result.headers.get("x-test").map(String::as_str), Some("1"));
    }

    #[test]
    fn clamps_xhigh_reasoning_to_high() {
        assert_eq!(
            clamp_reasoning(Some(ModelThinkingLevel::XHigh)),
            Some(ModelThinkingLevel::High)
        );
    }

    #[test]
    fn adjusts_max_tokens_to_include_thinking_budget() {
        let budget =
            adjust_max_tokens_for_thinking(Some(4096), 20_000, ModelThinkingLevel::Medium, None);

        assert_eq!(
            budget,
            ThinkingTokenBudget {
                max_tokens: 12_288,
                thinking_budget: 8192,
            }
        );
    }

    #[test]
    fn reduces_thinking_budget_when_output_window_is_too_small() {
        let budget =
            adjust_max_tokens_for_thinking(Some(256), 1100, ModelThinkingLevel::High, None);

        assert_eq!(
            budget,
            ThinkingTokenBudget {
                max_tokens: 1100,
                thinking_budget: 76,
            }
        );
    }
}
