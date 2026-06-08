use ai::Model;

pub fn default_model_for_provider(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "amazon-bedrock" => "us.anthropic.claude-opus-4-6-v1",
        "anthropic" => "claude-opus-4-7",
        "openai" => "gpt-5.4",
        "azure-openai-responses" => "gpt-5.4",
        "openai-codex" => "gpt-5.5",
        "deepseek" => "deepseek-v4-pro",
        "google" | "google-vertex" => "gemini-3.1-pro-preview",
        "github-copilot" => "gpt-5.4",
        "openrouter" => "moonshotai/kimi-k2.6",
        "vercel-ai-gateway" => "zai/glm-5.1",
        "xai" => "grok-4.20-0309-reasoning",
        "groq" => "openai/gpt-oss-120b",
        "cerebras" => "zai-glm-4.7",
        "zai" => "glm-5.1",
        "mistral" => "devstral-medium-latest",
        "minimax" | "minimax-cn" => "MiniMax-M2.7",
        "moonshotai" | "moonshotai-cn" => "kimi-k2.6",
        "huggingface" => "moonshotai/Kimi-K2.6",
        "fireworks" => "accounts/fireworks/models/kimi-k2p6",
        "together" => "moonshotai/Kimi-K2.6",
        "opencode" | "opencode-go" => "kimi-k2.6",
        "kimi-coding" => "kimi-for-coding",
        "cloudflare-workers-ai" => "@cf/moonshotai/kimi-k2.6",
        "cloudflare-ai-gateway" => "workers-ai/@cf/moonshotai/kimi-k2.6",
        "xiaomi" | "xiaomi-token-plan-cn" | "xiaomi-token-plan-ams" | "xiaomi-token-plan-sgp" => {
            "mimo-v2.5-pro"
        }
        _ => return None,
    })
}

pub(super) fn canonical_provider(provider: &str, models: &[Model]) -> Option<String> {
    models
        .iter()
        .find(|model| model.provider.eq_ignore_ascii_case(provider))
        .map(|model| model.provider.clone())
}

pub(super) fn build_fallback_model(
    provider: &str,
    model_id: &str,
    available_models: &[Model],
) -> Option<Model> {
    let provider_models = available_models
        .iter()
        .filter(|model| model.provider == provider)
        .collect::<Vec<_>>();
    let base_model = default_model_for_provider(provider)
        .and_then(|default_id| {
            provider_models
                .iter()
                .find(|model| model.id == default_id)
                .copied()
        })
        .or_else(|| provider_models.first().copied())?;
    let mut model = base_model.clone();
    model.id = model_id.to_string();
    model.display_name = model_id.to_string();
    Some(model)
}

pub(super) fn preferred_default_model(available_models: &[Model]) -> Option<Model> {
    for provider in [
        "amazon-bedrock",
        "anthropic",
        "openai",
        "azure-openai-responses",
        "openai-codex",
        "deepseek",
        "google",
        "google-vertex",
        "github-copilot",
        "openrouter",
        "vercel-ai-gateway",
        "xai",
        "groq",
        "cerebras",
        "zai",
        "mistral",
        "minimax",
        "minimax-cn",
        "moonshotai",
        "moonshotai-cn",
        "huggingface",
        "fireworks",
        "together",
        "opencode",
        "opencode-go",
        "kimi-coding",
        "cloudflare-workers-ai",
        "cloudflare-ai-gateway",
        "xiaomi",
        "xiaomi-token-plan-cn",
        "xiaomi-token-plan-ams",
        "xiaomi-token-plan-sgp",
    ] {
        if let Some(default_id) = default_model_for_provider(provider) {
            if let Some(model) = available_models
                .iter()
                .find(|model| model.provider == provider && model.id == default_id)
            {
                return Some(model.clone());
            }
        }
    }
    available_models.first().cloned()
}
