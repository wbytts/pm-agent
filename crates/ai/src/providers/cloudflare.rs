use std::collections::BTreeMap;
use std::env;

use crate::{AiError, AiResult, Model};

pub const CLOUDFLARE_WORKERS_AI_BASE_URL: &str =
    "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1";
pub const CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/compat";
pub const CLOUDFLARE_AI_GATEWAY_OPENAI_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/openai";
pub const CLOUDFLARE_AI_GATEWAY_ANTHROPIC_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/anthropic";

pub fn is_cloudflare_provider(provider: &str) -> bool {
    matches!(provider, "cloudflare-workers-ai" | "cloudflare-ai-gateway")
}

pub fn resolve_cloudflare_base_url(model: &Model) -> AiResult<String> {
    let base_url = model.base_url.as_deref().unwrap_or_default();
    resolve_cloudflare_base_url_from_str(model.provider.as_str(), base_url)
}

pub fn resolve_cloudflare_base_url_from_str(provider: &str, base_url: &str) -> AiResult<String> {
    resolve_cloudflare_base_url_with_env(provider, base_url, |name| env::var(name).ok())
}

pub fn resolve_cloudflare_base_url_with_values(
    provider: &str,
    base_url: &str,
    values: &BTreeMap<String, String>,
) -> AiResult<String> {
    resolve_cloudflare_base_url_with_env(provider, base_url, |name| values.get(name).cloned())
}

fn resolve_cloudflare_base_url_with_env(
    provider: &str,
    base_url: &str,
    get_env: impl Fn(&str) -> Option<String>,
) -> AiResult<String> {
    if !base_url.contains('{') {
        return Ok(base_url.to_string());
    }
    let mut output = String::new();
    let chars = base_url.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '{' {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let Some(end_offset) = chars[index + 1..].iter().position(|ch| *ch == '}') else {
            output.push(chars[index]);
            index += 1;
            continue;
        };
        let end = index + 1 + end_offset;
        let name = chars[index + 1..end].iter().collect::<String>();
        if !is_env_placeholder(&name) {
            output.push_str(&chars[index..=end].iter().collect::<String>());
            index = end + 1;
            continue;
        }
        let value = get_env(&name).ok_or_else(|| {
            AiError::InvalidResponse(format!(
                "{name} is required for provider {provider} but is not set."
            ))
        })?;
        output.push_str(&value);
        index = end + 1;
    }
    Ok(output)
}

fn is_env_placeholder(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cloudflare_providers() {
        assert!(is_cloudflare_provider("cloudflare-workers-ai"));
        assert!(is_cloudflare_provider("cloudflare-ai-gateway"));
        assert!(!is_cloudflare_provider("openai"));
    }

    #[test]
    fn resolves_base_url_placeholders() {
        let mut values = BTreeMap::new();
        values.insert("CLOUDFLARE_ACCOUNT_ID".to_string(), "account".to_string());
        values.insert("CLOUDFLARE_GATEWAY_ID".to_string(), "gateway".to_string());

        assert_eq!(
            resolve_cloudflare_base_url_with_values(
                "cloudflare-ai-gateway",
                CLOUDFLARE_AI_GATEWAY_OPENAI_BASE_URL,
                &values
            )
            .expect("resolve"),
            "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"
        );
    }

    #[test]
    fn resolves_runtime_base_url_without_model_base_url() {
        let mut values = BTreeMap::new();
        values.insert("CLOUDFLARE_ACCOUNT_ID".to_string(), "account".to_string());

        assert_eq!(
            resolve_cloudflare_base_url_with_values(
                "cloudflare-workers-ai",
                CLOUDFLARE_WORKERS_AI_BASE_URL,
                &values
            )
            .expect("resolve"),
            "https://api.cloudflare.com/client/v4/accounts/account/ai/v1"
        );
    }

    #[test]
    fn reports_missing_placeholder_value() {
        let error = resolve_cloudflare_base_url_with_values(
            "cloudflare-ai-gateway",
            CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL,
            &BTreeMap::new(),
        )
        .expect_err("missing value");
        assert!(error.to_string().contains("CLOUDFLARE_ACCOUNT_ID"));
    }
}
