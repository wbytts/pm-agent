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
    if let Some(key) =
        find_env_keys(provider).and_then(|keys| keys.first().and_then(|key| env::var(key).ok()))
    {
        return Some(key);
    }

    if provider == "amazon-bedrock" && has_bedrock_ambient_credentials() {
        return Some("<authenticated>".to_string());
    }

    None
}

fn has_bedrock_ambient_credentials() -> bool {
    env::var_os("AWS_PROFILE").is_some()
        || env::var_os("AWS_BEARER_TOKEN_BEDROCK").is_some()
        || env::var_os("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some()
        || env::var_os("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some()
        || env::var_os("AWS_WEB_IDENTITY_TOKEN_FILE").is_some()
        || (env::var_os("AWS_ACCESS_KEY_ID").is_some()
            && env::var_os("AWS_SECRET_ACCESS_KEY").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    const BEDROCK_ENV_KEYS: [&str; 7] = [
        "AWS_PROFILE",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        "AWS_WEB_IDENTITY_TOKEN_FILE",
    ];

    struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

    impl EnvRestore {
        fn clear(keys: &[&'static str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| (*key, env::var_os(key)))
                .collect::<Vec<_>>();
            for key in keys {
                env::remove_var(key);
            }
            Self(saved)
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    env::set_var(key, value);
                } else {
                    env::remove_var(key);
                }
            }
        }
    }

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

    #[test]
    fn bedrock_auth_uses_ambient_aws_credentials_without_reporting_env_keys_like_pi() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _restore = EnvRestore::clear(&BEDROCK_ENV_KEYS);

        env::set_var("AWS_PROFILE", "dev-profile");

        assert_eq!(find_env_keys("amazon-bedrock"), None);
        assert_eq!(
            get_env_api_key("amazon-bedrock").as_deref(),
            Some("<authenticated>")
        );

        env::remove_var("AWS_PROFILE");

        env::set_var("AWS_ACCESS_KEY_ID", "access");
        assert_eq!(get_env_api_key("amazon-bedrock"), None);

        env::set_var("AWS_SECRET_ACCESS_KEY", "secret");
        assert_eq!(
            get_env_api_key("amazon-bedrock").as_deref(),
            Some("<authenticated>")
        );

        env::remove_var("AWS_ACCESS_KEY_ID");
        env::remove_var("AWS_SECRET_ACCESS_KEY");
        env::set_var("AWS_BEARER_TOKEN_BEDROCK", "bearer");
        assert_eq!(
            get_env_api_key("amazon-bedrock").as_deref(),
            Some("<authenticated>")
        );

        env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
    }
}
