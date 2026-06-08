use std::collections::BTreeMap;

use serde::Deserialize;

use crate::registry::ModelRegistry;
use crate::{AiError, AiResult, Model};

use super::device_code::{
    poll_oauth_device_code_flow, OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult,
    CANCEL_MESSAGE,
};
use super::types::{
    OAuthCredentials, OAuthDeviceCodeInfo, OAuthLoginCallbacks, OAuthPrompt, OAuthProviderInterface,
};

pub const GITHUB_COPILOT_PROVIDER_ID: &str = "github-copilot";
pub const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
pub const GITHUB_COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.35.0";
pub const GITHUB_COPILOT_EDITOR_VERSION: &str = "vscode/1.107.0";
pub const GITHUB_COPILOT_EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.35.0";
pub const GITHUB_COPILOT_INTEGRATION_ID: &str = "vscode-chat";
pub const GITHUB_COPILOT_DEFAULT_BASE_URL: &str = "https://api.individual.githubcopilot.com";
pub const GITHUB_COPILOT_ENTERPRISE_URL_KEY: &str = "enterpriseUrl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubCopilotUrls {
    pub device_code_url: String,
    pub access_token_url: String,
    pub copilot_token_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubCopilotDeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval_seconds: Option<u64>,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubCopilotModelEnableResult {
    pub model_id: String,
    pub success: bool,
}

pub struct GitHubCopilotOAuthProvider;

impl OAuthProviderInterface for GitHubCopilotOAuthProvider {
    fn id(&self) -> &str {
        GITHUB_COPILOT_PROVIDER_ID
    }

    fn name(&self) -> &str {
        "GitHub Copilot"
    }

    fn login(&self, callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
        login_github_copilot(callbacks)
    }

    fn refresh_token(&self, credentials: &OAuthCredentials) -> AiResult<OAuthCredentials> {
        let enterprise_domain = credentials
            .extra
            .get(GITHUB_COPILOT_ENTERPRISE_URL_KEY)
            .and_then(|value| normalize_domain(value));
        refresh_github_copilot_token(credentials.refresh.as_str(), enterprise_domain.as_deref())
    }

    fn get_api_key(&self, credentials: &OAuthCredentials) -> String {
        credentials.access.clone()
    }

    fn modify_models(&self, models: Vec<Model>, credentials: &OAuthCredentials) -> Vec<Model> {
        let enterprise_domain = credentials
            .extra
            .get(GITHUB_COPILOT_ENTERPRISE_URL_KEY)
            .and_then(|value| normalize_domain(value));
        let base_url = get_github_copilot_base_url(
            Some(credentials.access.as_str()),
            enterprise_domain.as_deref(),
        );
        models
            .into_iter()
            .map(|mut model| {
                if model.provider == GITHUB_COPILOT_PROVIDER_ID {
                    model.base_url = Some(base_url.clone());
                }
                model
            })
            .collect()
    }
}

pub fn github_copilot_oauth_provider() -> GitHubCopilotOAuthProvider {
    GitHubCopilotOAuthProvider
}

pub fn normalize_domain(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.');

    if is_valid_hostname(host) {
        Some(host.to_ascii_lowercase())
    } else {
        None
    }
}

pub fn get_github_copilot_urls(domain: &str) -> GitHubCopilotUrls {
    GitHubCopilotUrls {
        device_code_url: format!("https://{domain}/login/device/code"),
        access_token_url: format!("https://{domain}/login/oauth/access_token"),
        copilot_token_url: format!("https://api.{domain}/copilot_internal/v2/token"),
    }
}

pub fn github_copilot_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "User-Agent".to_string(),
            GITHUB_COPILOT_USER_AGENT.to_string(),
        ),
        (
            "Editor-Version".to_string(),
            GITHUB_COPILOT_EDITOR_VERSION.to_string(),
        ),
        (
            "Editor-Plugin-Version".to_string(),
            GITHUB_COPILOT_EDITOR_PLUGIN_VERSION.to_string(),
        ),
        (
            "Copilot-Integration-Id".to_string(),
            GITHUB_COPILOT_INTEGRATION_ID.to_string(),
        ),
    ])
}

pub fn login_github_copilot(callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
    let input = callbacks.on_prompt(OAuthPrompt {
        message: "GitHub Enterprise URL/domain (blank for github.com)".to_string(),
        placeholder: Some("company.ghe.com".to_string()),
        allow_empty: true,
    })?;
    if callbacks.is_cancelled() {
        return Err(AiError::InvalidResponse(CANCEL_MESSAGE.to_string()));
    }

    let trimmed = input.trim();
    let enterprise_domain = normalize_domain(input.as_str());
    if !trimmed.is_empty() && enterprise_domain.is_none() {
        return Err(AiError::InvalidResponse(
            "Invalid GitHub Enterprise URL/domain".to_string(),
        ));
    }
    let domain = enterprise_domain.as_deref().unwrap_or("github.com");
    let device = start_github_copilot_device_flow(domain)?;
    callbacks.on_device_code(OAuthDeviceCodeInfo {
        user_code: device.user_code.clone(),
        verification_uri: device.verification_uri.clone(),
        interval_seconds: device.interval_seconds,
        expires_in_seconds: Some(device.expires_in_seconds),
    });

    let github_access_token =
        poll_for_github_access_token(domain, &device, || callbacks.is_cancelled())?;
    let credentials =
        refresh_github_copilot_token(github_access_token.as_str(), enterprise_domain.as_deref())?;
    callbacks.on_progress("Enabling models...");
    let model_ids = ModelRegistry::builtins()
        .models(GITHUB_COPILOT_PROVIDER_ID)
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();
    enable_all_github_copilot_models(
        credentials.access.as_str(),
        enterprise_domain.as_deref(),
        &model_ids,
        |model_id, success| {
            let status = if success { "enabled" } else { "failed" };
            callbacks.on_progress(format!("{model_id}: {status}").as_str());
        },
    );
    Ok(credentials)
}

pub fn start_github_copilot_device_flow(domain: &str) -> AiResult<GitHubCopilotDeviceCode> {
    let urls = get_github_copilot_urls(domain);
    let response = reqwest::blocking::Client::new()
        .post(urls.device_code_url.as_str())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", GITHUB_COPILOT_USER_AGENT)
        .form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("scope", "read:user"),
        ])
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    response_body(response).and_then(|body| parse_github_copilot_device_code_response(&body))
}

pub fn parse_github_copilot_device_code_response(body: &str) -> AiResult<GitHubCopilotDeviceCode> {
    let response = serde_json::from_str::<GitHubCopilotDeviceCodeResponse>(body)
        .map_err(|_| AiError::InvalidResponse("Invalid device code response".to_string()))?;
    let device_code = response
        .device_code
        .filter(|value| !value.trim().is_empty());
    let user_code = response.user_code.filter(|value| !value.trim().is_empty());
    let verification_uri = response
        .verification_uri
        .filter(|value| !value.trim().is_empty());
    match (
        device_code,
        user_code,
        verification_uri,
        response.expires_in,
    ) {
        (Some(device_code), Some(user_code), Some(verification_uri), Some(expires_in_seconds)) => {
            Ok(GitHubCopilotDeviceCode {
                device_code,
                user_code,
                verification_uri,
                interval_seconds: response.interval,
                expires_in_seconds,
            })
        }
        _ => Err(AiError::InvalidResponse(
            "Invalid device code response fields".to_string(),
        )),
    }
}

pub fn poll_for_github_access_token(
    domain: &str,
    device: &GitHubCopilotDeviceCode,
    is_cancelled: impl Fn() -> bool,
) -> AiResult<String> {
    let urls = get_github_copilot_urls(domain);
    poll_oauth_device_code_flow(OAuthDeviceCodePollOptions {
        interval_seconds: device.interval_seconds,
        expires_in_seconds: Some(device.expires_in_seconds),
        poll: || {
            if is_cancelled() {
                return OAuthDeviceCodePollResult::Failed {
                    message: CANCEL_MESSAGE.to_string(),
                };
            }
            match request_github_access_token(urls.access_token_url.as_str(), &device.device_code) {
                Ok(result) => result,
                Err(error) => OAuthDeviceCodePollResult::Failed {
                    message: error.to_string(),
                },
            }
        },
        is_cancelled: None,
    })
}

fn request_github_access_token(
    access_token_url: &str,
    device_code: &str,
) -> AiResult<OAuthDeviceCodePollResult> {
    let response = reqwest::blocking::Client::new()
        .post(access_token_url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", GITHUB_COPILOT_USER_AGENT)
        .form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    response_body(response).and_then(|body| parse_github_device_token_response(&body))
}

pub fn parse_github_device_token_response(body: &str) -> AiResult<OAuthDeviceCodePollResult> {
    let value = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|_| AiError::InvalidResponse("Invalid device token response".to_string()))?;
    let Some(object) = value.as_object() else {
        return Err(AiError::InvalidResponse(
            "Invalid device token response".to_string(),
        ));
    };
    if let Some(access_token) = object.get("access_token").and_then(|value| value.as_str()) {
        if !access_token.trim().is_empty() {
            return Ok(OAuthDeviceCodePollResult::Complete {
                access_token: access_token.to_string(),
            });
        }
    }
    if let Some(error) = object.get("error").and_then(|value| value.as_str()) {
        return Ok(match error {
            "authorization_pending" => OAuthDeviceCodePollResult::Pending,
            "slow_down" => OAuthDeviceCodePollResult::SlowDown,
            other => {
                let description = object
                    .get("error_description")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(": {value}"))
                    .unwrap_or_default();
                OAuthDeviceCodePollResult::Failed {
                    message: format!("Device flow failed: {other}{description}"),
                }
            }
        });
    }
    Err(AiError::InvalidResponse(
        "Invalid device token response".to_string(),
    ))
}

pub fn enable_github_copilot_model(
    token: &str,
    model_id: &str,
    enterprise_domain: Option<&str>,
) -> bool {
    let url = github_copilot_model_policy_url(token, model_id, enterprise_domain);
    reqwest::blocking::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", GITHUB_COPILOT_USER_AGENT)
        .header("Editor-Version", GITHUB_COPILOT_EDITOR_VERSION)
        .header(
            "Editor-Plugin-Version",
            GITHUB_COPILOT_EDITOR_PLUGIN_VERSION,
        )
        .header("Copilot-Integration-Id", GITHUB_COPILOT_INTEGRATION_ID)
        .header("openai-intent", "chat-policy")
        .header("x-interaction-type", "chat-policy")
        .json(&serde_json::json!({ "state": "enabled" }))
        .send()
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

pub fn enable_all_github_copilot_models(
    token: &str,
    enterprise_domain: Option<&str>,
    model_ids: &[String],
    mut on_progress: impl FnMut(&str, bool),
) -> Vec<GitHubCopilotModelEnableResult> {
    model_ids
        .iter()
        .map(|model_id| {
            let success = enable_github_copilot_model(token, model_id.as_str(), enterprise_domain);
            on_progress(model_id, success);
            GitHubCopilotModelEnableResult {
                model_id: model_id.clone(),
                success,
            }
        })
        .collect()
}

pub fn github_copilot_model_policy_url(
    token: &str,
    model_id: &str,
    enterprise_domain: Option<&str>,
) -> String {
    let base_url = get_github_copilot_base_url(Some(token), enterprise_domain);
    format!(
        "{}/models/{model_id}/policy",
        base_url.trim_end_matches('/')
    )
}

pub fn refresh_github_copilot_token(
    refresh_token: &str,
    enterprise_domain: Option<&str>,
) -> AiResult<OAuthCredentials> {
    let domain = enterprise_domain.unwrap_or("github.com");
    let urls = get_github_copilot_urls(domain);
    let response = reqwest::blocking::Client::new()
        .get(urls.copilot_token_url.as_str())
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {refresh_token}"))
        .header("User-Agent", GITHUB_COPILOT_USER_AGENT)
        .header("Editor-Version", GITHUB_COPILOT_EDITOR_VERSION)
        .header(
            "Editor-Plugin-Version",
            GITHUB_COPILOT_EDITOR_PLUGIN_VERSION,
        )
        .header("Copilot-Integration-Id", GITHUB_COPILOT_INTEGRATION_ID)
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    response_body(response).and_then(|body| {
        parse_github_copilot_token_response(refresh_token, enterprise_domain, &body)
    })
}

pub fn parse_github_copilot_token_response(
    refresh_token: &str,
    enterprise_domain: Option<&str>,
    body: &str,
) -> AiResult<OAuthCredentials> {
    let response = serde_json::from_str::<GitHubCopilotTokenResponse>(body)
        .map_err(|_| AiError::InvalidResponse("Invalid Copilot token response".to_string()))?;
    let token = response
        .token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            AiError::InvalidResponse("Invalid Copilot token response fields".to_string())
        })?;
    let expires_at = response.expires_at.ok_or_else(|| {
        AiError::InvalidResponse("Invalid Copilot token response fields".to_string())
    })?;
    let expires = expires_at
        .saturating_mul(1000)
        .saturating_sub(5 * 60 * 1000);
    Ok(github_copilot_credentials(
        refresh_token,
        token,
        expires,
        enterprise_domain.map(str::to_string),
    ))
}

pub fn get_base_url_from_token(token: &str) -> Option<String> {
    token
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| {
            if key == "proxy-ep" && !value.trim().is_empty() {
                let value = value.trim();
                let api_host = value
                    .strip_prefix("proxy.")
                    .map(|rest| format!("api.{rest}"))
                    .unwrap_or_else(|| value.to_string());
                Some(format!("https://{api_host}"))
            } else {
                None
            }
        })
}

pub fn get_github_copilot_base_url(token: Option<&str>, enterprise_domain: Option<&str>) -> String {
    if let Some(token_base_url) = token.and_then(get_base_url_from_token) {
        return token_base_url;
    }
    if let Some(enterprise_domain) = enterprise_domain.filter(|domain| !domain.trim().is_empty()) {
        return format!("https://copilot-api.{enterprise_domain}");
    }
    GITHUB_COPILOT_DEFAULT_BASE_URL.to_string()
}

pub fn github_copilot_credentials(
    refresh: impl Into<String>,
    access: impl Into<String>,
    expires: u128,
    enterprise_domain: Option<String>,
) -> OAuthCredentials {
    let mut extra = BTreeMap::new();
    if let Some(enterprise_domain) = enterprise_domain {
        extra.insert(
            GITHUB_COPILOT_ENTERPRISE_URL_KEY.to_string(),
            enterprise_domain,
        );
    }
    OAuthCredentials {
        refresh: refresh.into(),
        access: access.into(),
        expires,
        extra,
    }
}

fn is_valid_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(is_valid_hostname_label)
}

fn is_valid_hostname_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn response_body(response: reqwest::blocking::Response) -> AiResult<String> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| AiError::Http(error.to_string()))?;
    if !status.is_success() {
        return Err(AiError::Http(format!("status={status}, body={body}")));
    }
    Ok(body)
}

#[derive(Debug, Deserialize)]
struct GitHubCopilotDeviceCodeResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubCopilotTokenResponse {
    token: Option<String>,
    expires_at: Option<u128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_enterprise_domain_inputs() {
        assert_eq!(
            normalize_domain(" https://Company.GHE.com/path?q=1 "),
            Some("company.ghe.com".to_string())
        );
        assert_eq!(
            normalize_domain("company.ghe.com:8443"),
            Some("company.ghe.com".to_string())
        );
        assert_eq!(normalize_domain(""), None);
        assert_eq!(normalize_domain("not a domain"), None);
    }

    #[test]
    fn builds_github_oauth_urls() {
        assert_eq!(
            get_github_copilot_urls("github.com"),
            GitHubCopilotUrls {
                device_code_url: "https://github.com/login/device/code".to_string(),
                access_token_url: "https://github.com/login/oauth/access_token".to_string(),
                copilot_token_url: "https://api.github.com/copilot_internal/v2/token".to_string(),
            }
        );
    }

    #[test]
    fn extracts_base_url_from_copilot_proxy_endpoint() {
        assert_eq!(
            get_base_url_from_token("tid=1;exp=2;proxy-ep=proxy.individual.githubcopilot.com;"),
            Some("https://api.individual.githubcopilot.com".to_string())
        );
    }

    #[test]
    fn resolves_base_url_with_fallbacks() {
        assert_eq!(
            get_github_copilot_base_url(
                Some("proxy-ep=proxy.business.githubcopilot.com"),
                Some("company.ghe.com"),
            ),
            "https://api.business.githubcopilot.com"
        );
        assert_eq!(
            get_github_copilot_base_url(None, Some("company.ghe.com")),
            "https://copilot-api.company.ghe.com"
        );
        assert_eq!(
            get_github_copilot_base_url(None, None),
            GITHUB_COPILOT_DEFAULT_BASE_URL
        );
    }

    #[test]
    fn builds_model_policy_url_like_pi() {
        assert_eq!(
            github_copilot_model_policy_url(
                "tid=1;proxy-ep=proxy.individual.githubcopilot.com;",
                "gpt-5.4",
                None,
            ),
            "https://api.individual.githubcopilot.com/models/gpt-5.4/policy"
        );
        assert_eq!(
            github_copilot_model_policy_url(
                "token-without-proxy",
                "gpt-5.4",
                Some("company.ghe.com")
            ),
            "https://copilot-api.company.ghe.com/models/gpt-5.4/policy"
        );
    }

    #[test]
    fn provider_modifies_only_github_copilot_models() {
        let provider = github_copilot_oauth_provider();
        let credentials = github_copilot_credentials(
            "refresh",
            "tid=1;proxy-ep=proxy.individual.githubcopilot.com;",
            1000,
            None,
        );
        let models = vec![
            Model {
                id: "gpt-5".to_string(),
                provider: GITHUB_COPILOT_PROVIDER_ID.to_string(),
                api: "openai-responses".to_string(),
                display_name: "GPT-5".to_string(),
                context_window: 128_000,
                ..Model::default()
            },
            Model {
                id: "claude".to_string(),
                provider: "anthropic".to_string(),
                api: "anthropic".to_string(),
                display_name: "Claude".to_string(),
                context_window: 128_000,
                base_url: Some("https://api.anthropic.com".to_string()),
                ..Model::default()
            },
        ];

        let models = provider.modify_models(models, &credentials);

        assert_eq!(
            models[0].base_url.as_deref(),
            Some("https://api.individual.githubcopilot.com")
        );
        assert_eq!(
            models[1].base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
    }

    #[test]
    fn parses_copilot_token_response_like_pi() {
        let credentials = parse_github_copilot_token_response(
            "refresh-token",
            Some("company.ghe.com"),
            r#"{"token":"access-token","expires_at":2000}"#,
        )
        .expect("credentials");

        assert_eq!(credentials.refresh, "refresh-token");
        assert_eq!(credentials.access, "access-token");
        assert_eq!(credentials.expires, 1_700_000);
        assert_eq!(
            credentials.extra.get(GITHUB_COPILOT_ENTERPRISE_URL_KEY),
            Some(&"company.ghe.com".to_string())
        );
    }

    #[test]
    fn rejects_invalid_copilot_token_response_fields() {
        let error = parse_github_copilot_token_response(
            "refresh-token",
            None,
            r#"{"token":"","expires_at":2000}"#,
        )
        .expect_err("invalid response");

        assert!(error
            .to_string()
            .contains("Invalid Copilot token response fields"));
    }

    #[test]
    fn parses_device_code_response_like_pi() {
        let device = parse_github_copilot_device_code_response(
            r#"{"device_code":"device","user_code":"user","verification_uri":"https://github.com/login/device","interval":2,"expires_in":900}"#,
        )
        .expect("device code");

        assert_eq!(
            device,
            GitHubCopilotDeviceCode {
                device_code: "device".to_string(),
                user_code: "user".to_string(),
                verification_uri: "https://github.com/login/device".to_string(),
                interval_seconds: Some(2),
                expires_in_seconds: 900,
            }
        );
    }

    #[test]
    fn rejects_invalid_device_code_response_fields() {
        let error = parse_github_copilot_device_code_response(
            r#"{"device_code":"device","verification_uri":"https://github.com/login/device","expires_in":900}"#,
        )
        .expect_err("invalid device code");

        assert!(error
            .to_string()
            .contains("Invalid device code response fields"));
    }

    #[test]
    fn parses_device_token_poll_states() {
        assert_eq!(
            parse_github_device_token_response(r#"{"access_token":"github-token"}"#)
                .expect("token"),
            OAuthDeviceCodePollResult::Complete {
                access_token: "github-token".to_string()
            }
        );
        assert_eq!(
            parse_github_device_token_response(r#"{"error":"authorization_pending"}"#)
                .expect("pending"),
            OAuthDeviceCodePollResult::Pending
        );
        assert_eq!(
            parse_github_device_token_response(r#"{"error":"slow_down"}"#).expect("slow down"),
            OAuthDeviceCodePollResult::SlowDown
        );
        assert_eq!(
            parse_github_device_token_response(
                r#"{"error":"access_denied","error_description":"User denied"}"#
            )
            .expect("failed"),
            OAuthDeviceCodePollResult::Failed {
                message: "Device flow failed: access_denied: User denied".to_string(),
            }
        );
    }
}
