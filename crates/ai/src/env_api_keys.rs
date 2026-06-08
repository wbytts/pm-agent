use std::env;

pub fn provider_api_key_env_vars(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "github-copilot" => Some(&["COPILOT_GITHUB_TOKEN"]),
        "anthropic" => Some(&["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]),
        "openai" => Some(&["OPENAI_API_KEY"]),
        "azure-openai-responses" => Some(&["AZURE_OPENAI_API_KEY"]),
        "deepseek" => Some(&["DEEPSEEK_API_KEY"]),
        "google" => Some(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]),
        "google-vertex" => Some(&["GOOGLE_CLOUD_API_KEY"]),
        "groq" => Some(&["GROQ_API_KEY"]),
        "cerebras" => Some(&["CEREBRAS_API_KEY"]),
        "xai" => Some(&["XAI_API_KEY"]),
        "openrouter" => Some(&["OPENROUTER_API_KEY"]),
        "vercel-ai-gateway" => Some(&["AI_GATEWAY_API_KEY"]),
        "zai" => Some(&["ZAI_API_KEY"]),
        "mistral" => Some(&["MISTRAL_API_KEY"]),
        "minimax" => Some(&["MINIMAX_API_KEY"]),
        "minimax-cn" => Some(&["MINIMAX_CN_API_KEY"]),
        "moonshotai" | "moonshotai-cn" => Some(&["MOONSHOT_API_KEY"]),
        "huggingface" => Some(&["HF_TOKEN"]),
        "fireworks" => Some(&["FIREWORKS_API_KEY"]),
        "together" => Some(&["TOGETHER_API_KEY"]),
        "opencode" | "opencode-go" => Some(&["OPENCODE_API_KEY"]),
        "kimi-coding" => Some(&["KIMI_API_KEY"]),
        "cloudflare-workers-ai" | "cloudflare-ai-gateway" => Some(&["CLOUDFLARE_API_KEY"]),
        "xiaomi" => Some(&["XIAOMI_API_KEY"]),
        "xiaomi-token-plan-cn" => Some(&["XIAOMI_TOKEN_PLAN_CN_API_KEY"]),
        "xiaomi-token-plan-ams" => Some(&["XIAOMI_TOKEN_PLAN_AMS_API_KEY"]),
        "xiaomi-token-plan-sgp" => Some(&["XIAOMI_TOKEN_PLAN_SGP_API_KEY"]),
        _ => None,
    }
}

pub fn find_env_keys(provider: &str) -> Option<Vec<String>> {
    let found = provider_api_key_env_vars(provider)?
        .iter()
        .filter(|key| env::var_os(key).is_some())
        .map(|key| (*key).to_string())
        .collect::<Vec<_>>();
    (!found.is_empty()).then_some(found)
}

pub fn get_env_api_key(provider: &str) -> Option<String> {
    find_env_keys(provider)?
        .first()
        .and_then(|key| env::var(key).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_provider_env_keys() {
        assert_eq!(
            provider_api_key_env_vars("anthropic"),
            Some(&["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"][..])
        );
        assert_eq!(
            provider_api_key_env_vars("google"),
            Some(&["GEMINI_API_KEY", "GOOGLE_API_KEY"][..])
        );
        assert!(provider_api_key_env_vars("missing").is_none());
    }
}
