use crate::settings_manager::{SettingsManager, SettingsStorage};
use std::collections::BTreeMap;

pub fn is_install_telemetry_enabled<S: SettingsStorage>(
    settings_manager: &SettingsManager<S>,
    telemetry_env: Option<&str>,
) -> bool {
    telemetry_env.map_or_else(
        || settings_manager.get_enable_install_telemetry(),
        is_truthy_env_flag,
    )
}

fn is_truthy_env_flag(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

pub fn attribution_headers(
    provider: &str,
    base_url: &str,
    session_id: Option<&str>,
    install_telemetry_enabled: bool,
) -> Option<BTreeMap<String, String>> {
    if let Some(session_id) = session_id.filter(|_| {
        provider == "opencode" || provider == "opencode-go" || base_url.contains("opencode.ai")
    }) {
        return Some(BTreeMap::from([
            ("x-opencode-session".to_string(), session_id.to_string()),
            ("x-opencode-client".to_string(), "pi".to_string()),
        ]));
    }

    if !install_telemetry_enabled {
        return None;
    }

    if provider == "openrouter" || base_url.contains("openrouter.ai") {
        return Some(BTreeMap::from([
            ("HTTP-Referer".to_string(), "https://pi.dev".to_string()),
            ("X-OpenRouter-Title".to_string(), "pi".to_string()),
            (
                "X-OpenRouter-Categories".to_string(),
                "cli-agent".to_string(),
            ),
        ]));
    }

    if provider == "cloudflare-workers-ai"
        || provider == "cloudflare-ai-gateway"
        || base_url.contains("api.cloudflare.com")
        || base_url.contains("gateway.ai.cloudflare.com")
    {
        return Some(BTreeMap::from([(
            "User-Agent".to_string(),
            "pi-coding-agent".to_string(),
        )]));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_manager::SettingsManager;
    use serde_json::json;

    #[test]
    fn env_overrides_settings() {
        let manager = SettingsManager::in_memory(json!({}));
        assert!(is_install_telemetry_enabled(&manager, Some("true")));
        assert!(!is_install_telemetry_enabled(&manager, Some("0")));
    }

    #[test]
    fn falls_back_to_settings() {
        let manager = SettingsManager::in_memory(json!({ "enableInstallTelemetry": true }));
        assert!(is_install_telemetry_enabled(&manager, None));
    }

    #[test]
    fn attribution_headers_match_pi_sdk_provider_rules() {
        let headers = attribution_headers(
            "opencode-go",
            "https://api.example.test",
            Some("session-123"),
            false,
        )
        .expect("opencode headers");
        assert_eq!(
            headers.get("x-opencode-session").map(String::as_str),
            Some("session-123")
        );
        assert_eq!(
            headers.get("x-opencode-client").map(String::as_str),
            Some("pi")
        );

        assert_eq!(
            attribution_headers("openrouter", "https://openrouter.ai/api/v1", None, false),
            None
        );

        let openrouter = attribution_headers("custom", "https://openrouter.ai/api/v1", None, true)
            .expect("openrouter headers");
        assert_eq!(
            openrouter.get("HTTP-Referer").map(String::as_str),
            Some("https://pi.dev")
        );
        assert_eq!(
            openrouter.get("X-OpenRouter-Title").map(String::as_str),
            Some("pi")
        );
        assert_eq!(
            openrouter
                .get("X-OpenRouter-Categories")
                .map(String::as_str),
            Some("cli-agent")
        );

        let cloudflare =
            attribution_headers("custom", "https://gateway.ai.cloudflare.com/v1", None, true)
                .expect("cloudflare headers");
        assert_eq!(
            cloudflare.get("User-Agent").map(String::as_str),
            Some("pi-coding-agent")
        );
    }
}
