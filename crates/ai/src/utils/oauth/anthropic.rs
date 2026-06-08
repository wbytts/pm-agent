use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::{AiError, AiResult};

use super::oauth_page::{oauth_error_html, oauth_success_html};
use super::pkce::generate_pkce;
use super::types::{
    OAuthAuthInfo, OAuthCredentials, OAuthLoginCallbacks, OAuthPrompt, OAuthProviderInterface,
};

pub const ANTHROPIC_OAUTH_PROVIDER_ID: &str = "anthropic";
pub const ANTHROPIC_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const ANTHROPIC_OAUTH_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const ANTHROPIC_OAUTH_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const ANTHROPIC_OAUTH_CALLBACK_HOST: &str = "127.0.0.1";
pub const ANTHROPIC_OAUTH_CALLBACK_PORT: u16 = 53692;
pub const ANTHROPIC_OAUTH_CALLBACK_PATH: &str = "/callback";
pub const ANTHROPIC_OAUTH_REDIRECT_URI: &str = "http://localhost:53692/callback";
pub const ANTHROPIC_OAUTH_SCOPES: &str =
    "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicAuthorizationInput {
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicAuthorizationFlow {
    pub verifier: String,
    pub challenge: String,
    pub url: String,
    pub redirect_uri: String,
}

pub struct AnthropicOAuthProvider;

impl OAuthProviderInterface for AnthropicOAuthProvider {
    fn id(&self) -> &str {
        ANTHROPIC_OAUTH_PROVIDER_ID
    }

    fn name(&self) -> &str {
        "Anthropic (Claude Pro/Max)"
    }

    fn uses_callback_server(&self) -> bool {
        true
    }

    fn login(&self, callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
        login_anthropic(callbacks)
    }

    fn refresh_token(&self, credentials: &OAuthCredentials) -> AiResult<OAuthCredentials> {
        refresh_anthropic_token(credentials.refresh.as_str())
    }

    fn get_api_key(&self, credentials: &OAuthCredentials) -> String {
        credentials.access.clone()
    }
}

pub fn anthropic_oauth_provider() -> AnthropicOAuthProvider {
    AnthropicOAuthProvider
}

pub fn login_anthropic(callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
    let pkce = generate_pkce()?;
    let flow = create_anthropic_authorization_url(
        pkce.verifier.as_str(),
        pkce.challenge.as_str(),
        ANTHROPIC_OAUTH_REDIRECT_URI,
    );
    callbacks.on_auth(OAuthAuthInfo {
        url: flow.url.clone(),
        instructions: Some(
            "Complete login in your browser. If the browser is on another machine, paste the final redirect URL here."
                .to_string(),
        ),
    });

    let callback = wait_for_anthropic_code(flow.verifier.as_str())?;
    let (code, state, redirect_uri) = if let Some((code, state)) = callback {
        (code, state, ANTHROPIC_OAUTH_REDIRECT_URI.to_string())
    } else {
        let input = callbacks.on_prompt(OAuthPrompt {
            message: "Paste the authorization code or full redirect URL:".to_string(),
            placeholder: Some(ANTHROPIC_OAUTH_REDIRECT_URI.to_string()),
            allow_empty: false,
        })?;
        let parsed =
            validate_anthropic_authorization_input(input.as_str(), flow.verifier.as_str())?;
        let code = parsed
            .code
            .ok_or_else(|| AiError::InvalidResponse("Missing authorization code".to_string()))?;
        let state = parsed.state.unwrap_or_else(|| flow.verifier.clone());
        (code, state, ANTHROPIC_OAUTH_REDIRECT_URI.to_string())
    };

    callbacks.on_progress("Exchanging authorization code for tokens...");
    exchange_anthropic_authorization_code(
        code.as_str(),
        state.as_str(),
        flow.verifier.as_str(),
        redirect_uri.as_str(),
    )
}

pub fn create_anthropic_authorization_url(
    verifier: &str,
    challenge: &str,
    redirect_uri: &str,
) -> AnthropicAuthorizationFlow {
    let params = [
        ("code", "true"),
        ("client_id", ANTHROPIC_OAUTH_CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", ANTHROPIC_OAUTH_SCOPES),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", verifier),
    ];
    AnthropicAuthorizationFlow {
        verifier: verifier.to_string(),
        challenge: challenge.to_string(),
        url: format!(
            "{}?{}",
            ANTHROPIC_OAUTH_AUTHORIZE_URL,
            form_encode_params(&params)
        ),
        redirect_uri: redirect_uri.to_string(),
    }
}

pub fn wait_for_anthropic_code(expected_state: &str) -> AiResult<Option<(String, String)>> {
    let Ok(listener) =
        TcpListener::bind((ANTHROPIC_OAUTH_CALLBACK_HOST, ANTHROPIC_OAUTH_CALLBACK_PORT))
    else {
        return Ok(None);
    };
    let (stream, _) = listener
        .accept()
        .map_err(|error| AiError::Http(format!("Anthropic OAuth callback 接收失败：{error}")))?;
    handle_anthropic_callback_stream(stream, expected_state)
}

pub fn handle_anthropic_callback_stream(
    mut stream: TcpStream,
    expected_state: &str,
) -> AiResult<Option<(String, String)>> {
    let mut buffer = [0u8; 8192];
    let bytes_read = stream
        .read(&mut buffer)
        .map_err(|error| AiError::Http(format!("读取 Anthropic OAuth callback 失败：{error}")))?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, html, result) = anthropic_callback_response(target, expected_state);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    stream.write_all(response.as_bytes()).map_err(|error| {
        AiError::Http(format!("写入 Anthropic OAuth callback 响应失败：{error}"))
    })?;
    Ok(result)
}

pub fn anthropic_callback_response(
    target: &str,
    expected_state: &str,
) -> (&'static str, String, Option<(String, String)>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != ANTHROPIC_OAUTH_CALLBACK_PATH {
        return (
            "404 Not Found",
            oauth_error_html("Callback route not found.", None),
            None,
        );
    }
    let params = parse_form_params(query);
    if let Some(error) = params.get("error") {
        return (
            "400 Bad Request",
            oauth_error_html(
                "Anthropic authentication did not complete.",
                Some(format!("Error: {error}").as_str()),
            ),
            None,
        );
    }
    let Some(code) = params.get("code").filter(|code| !code.is_empty()) else {
        return (
            "400 Bad Request",
            oauth_error_html("Missing code or state parameter.", None),
            None,
        );
    };
    let Some(state) = params.get("state").filter(|state| !state.is_empty()) else {
        return (
            "400 Bad Request",
            oauth_error_html("Missing code or state parameter.", None),
            None,
        );
    };
    if state != expected_state {
        return (
            "400 Bad Request",
            oauth_error_html("State mismatch.", None),
            None,
        );
    }
    (
        "200 OK",
        oauth_success_html("Anthropic authentication completed. You can close this window."),
        Some((code.clone(), state.clone())),
    )
}

pub fn validate_anthropic_authorization_input(
    input: &str,
    expected_state: &str,
) -> AiResult<AnthropicAuthorizationInput> {
    let parsed = parse_anthropic_authorization_input(input);
    if parsed
        .state
        .as_deref()
        .is_some_and(|state| state != expected_state)
    {
        return Err(AiError::InvalidResponse("OAuth state mismatch".to_string()));
    }
    Ok(parsed)
}

pub fn parse_anthropic_authorization_input(input: &str) -> AnthropicAuthorizationInput {
    let value = input.trim();
    if value.is_empty() {
        return AnthropicAuthorizationInput {
            code: None,
            state: None,
        };
    }
    if let Some(query) = extract_query_like(value) {
        let params = parse_form_params(query);
        return AnthropicAuthorizationInput {
            code: params.get("code").cloned(),
            state: params.get("state").cloned(),
        };
    }
    if let Some((code, state)) = value.split_once('#') {
        return AnthropicAuthorizationInput {
            code: Some(code.to_string()),
            state: Some(state.to_string()),
        };
    }
    AnthropicAuthorizationInput {
        code: Some(value.to_string()),
        state: None,
    }
}

pub fn exchange_anthropic_authorization_code(
    code: &str,
    state: &str,
    verifier: &str,
    redirect_uri: &str,
) -> AiResult<OAuthCredentials> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": ANTHROPIC_OAUTH_CLIENT_ID,
        "code": code,
        "state": state,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
    });
    post_anthropic_token_json(body)
        .and_then(|body| parse_anthropic_token_response(&body, current_time_millis()))
}

pub fn refresh_anthropic_token(refresh_token: &str) -> AiResult<OAuthCredentials> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": ANTHROPIC_OAUTH_CLIENT_ID,
        "refresh_token": refresh_token,
    });
    post_anthropic_token_json(body)
        .and_then(|body| parse_anthropic_token_response(&body, current_time_millis()))
}

pub fn parse_anthropic_token_response(body: &str, now_millis: u128) -> AiResult<OAuthCredentials> {
    let response = serde_json::from_str::<AnthropicTokenResponse>(body)
        .map_err(|error| AiError::InvalidResponse(format!("Anthropic token JSON 无效：{error}")))?;
    let access = response
        .access_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AiError::InvalidResponse("Anthropic token response fields 缺失".to_string())
        })?;
    let refresh = response
        .refresh_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AiError::InvalidResponse("Anthropic token response fields 缺失".to_string())
        })?;
    let expires_in = response.expires_in.ok_or_else(|| {
        AiError::InvalidResponse("Anthropic token response fields 缺失".to_string())
    })?;
    Ok(OAuthCredentials {
        refresh,
        access,
        expires: now_millis
            .saturating_add(expires_in.saturating_mul(1000))
            .saturating_sub(5 * 60 * 1000),
        extra: BTreeMap::new(),
    })
}

fn post_anthropic_token_json(body: serde_json::Value) -> AiResult<String> {
    let response = reqwest::blocking::Client::new()
        .post(ANTHROPIC_OAUTH_TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| AiError::Http(error.to_string()))?;
    if !status.is_success() {
        return Err(AiError::Http(format!(
            "HTTP request failed. status={status}; url={ANTHROPIC_OAUTH_TOKEN_URL}; body={body}"
        )));
    }
    Ok(body)
}

fn extract_query_like(value: &str) -> Option<&str> {
    if let Some((_, query_and_fragment)) = value.split_once('?') {
        return Some(query_and_fragment.split('#').next().unwrap_or_default());
    }
    if value.contains("code=") {
        return Some(value);
    }
    None
}

fn parse_form_params(value: &str) -> BTreeMap<String, String> {
    value
        .split('&')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn form_encode_params(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", form_encode(key), form_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn form_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            b' ' => output.push('+'),
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    output.push((high << 4) | low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct AnthropicTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_authorization_input_like_pi() {
        assert_eq!(
            parse_anthropic_authorization_input("code#state"),
            AnthropicAuthorizationInput {
                code: Some("code".to_string()),
                state: Some("state".to_string()),
            }
        );
        assert_eq!(
            parse_anthropic_authorization_input(
                "http://localhost:53692/callback?code=a%201&state=s%2B1"
            ),
            AnthropicAuthorizationInput {
                code: Some("a 1".to_string()),
                state: Some("s+1".to_string()),
            }
        );
    }

    #[test]
    fn builds_authorization_url() {
        let flow = create_anthropic_authorization_url(
            "verifier",
            "challenge",
            ANTHROPIC_OAUTH_REDIRECT_URI,
        );

        assert_eq!(flow.verifier, "verifier");
        assert!(flow.url.starts_with(ANTHROPIC_OAUTH_AUTHORIZE_URL));
        assert!(flow
            .url
            .contains("client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"));
        assert!(flow.url.contains("code_challenge=challenge"));
        assert!(flow.url.contains("state=verifier"));
    }

    #[test]
    fn callback_response_validates_error_state_and_code() {
        let (status, _, result) = anthropic_callback_response("/missing?code=abc&state=s", "s");
        assert_eq!(status, "404 Not Found");
        assert_eq!(result, None);

        let (status, html, result) = anthropic_callback_response("/callback?error=denied", "s");
        assert_eq!(status, "400 Bad Request");
        assert!(html.contains("denied"));
        assert_eq!(result, None);

        let (status, _, result) =
            anthropic_callback_response("/callback?code=abc&state=wrong", "s");
        assert_eq!(status, "400 Bad Request");
        assert_eq!(result, None);

        let (status, html, result) =
            anthropic_callback_response("/callback?code=abc%201&state=s", "s");
        assert_eq!(status, "200 OK");
        assert!(html.contains("Authentication successful"));
        assert_eq!(result, Some(("abc 1".to_string(), "s".to_string())));
    }

    #[test]
    fn validates_manual_state() {
        assert_eq!(
            validate_anthropic_authorization_input("code#state", "state")
                .expect("valid")
                .code
                .as_deref(),
            Some("code")
        );
        let error = validate_anthropic_authorization_input("code#wrong", "state")
            .expect_err("state mismatch");
        assert!(error.to_string().contains("OAuth state mismatch"));
    }

    #[test]
    fn parses_token_response_with_early_expiry() {
        let credentials = parse_anthropic_token_response(
            r#"{"access_token":"access","refresh_token":"refresh","expires_in":600}"#,
            1000,
        )
        .expect("credentials");

        assert_eq!(credentials.access, "access");
        assert_eq!(credentials.refresh, "refresh");
        assert_eq!(credentials.expires, 301_000);
    }
}
