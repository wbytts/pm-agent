use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::{AiError, AiResult};

use super::oauth_page::{oauth_error_html, oauth_success_html};
use super::pkce::generate_pkce;
use super::types::{OAuthCredentials, OAuthLoginCallbacks, OAuthProviderInterface};

pub const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
pub const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OPENAI_CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const OPENAI_CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const OPENAI_CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const OPENAI_CODEX_CALLBACK_HOST: &str = "127.0.0.1";
pub const OPENAI_CODEX_CALLBACK_PORT: u16 = 1455;
pub const OPENAI_CODEX_SCOPE: &str = "openid profile email offline_access";
pub const OPENAI_CODEX_ACCOUNT_ID_KEY: &str = "accountId";
const JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCodexAuthorizationInput {
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCodexAuthorizationFlow {
    pub verifier: String,
    pub state: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAiCodexTokenResult {
    Success {
        access: String,
        refresh: String,
        expires: u128,
    },
    Failed {
        message: String,
        status: Option<u16>,
    },
}

pub struct OpenAiCodexOAuthProvider;

impl OAuthProviderInterface for OpenAiCodexOAuthProvider {
    fn id(&self) -> &str {
        OPENAI_CODEX_PROVIDER_ID
    }

    fn name(&self) -> &str {
        "ChatGPT Plus/Pro (Codex Subscription)"
    }

    fn uses_callback_server(&self) -> bool {
        true
    }

    fn login(&self, callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials> {
        login_openai_codex(callbacks, None)
    }

    fn refresh_token(&self, credentials: &OAuthCredentials) -> AiResult<OAuthCredentials> {
        refresh_openai_codex_token(credentials.refresh.as_str())
    }

    fn get_api_key(&self, credentials: &OAuthCredentials) -> String {
        credentials.access.clone()
    }
}

pub fn openai_codex_oauth_provider() -> OpenAiCodexOAuthProvider {
    OpenAiCodexOAuthProvider
}

pub fn login_openai_codex(
    callbacks: &mut dyn OAuthLoginCallbacks,
    originator: Option<&str>,
) -> AiResult<OAuthCredentials> {
    let pkce = generate_pkce()?;
    let state = create_openai_codex_state();
    let flow = create_openai_codex_authorization_url(
        pkce.verifier.as_str(),
        pkce.challenge.as_str(),
        state.as_str(),
        originator,
    );
    callbacks.on_auth(super::types::OAuthAuthInfo {
        url: flow.url.clone(),
        instructions: Some("A browser window should open. Complete login to finish.".to_string()),
    });

    let code = wait_for_openai_codex_code(flow.state.as_str())?.or_else(|| {
        callbacks
            .on_prompt(super::types::OAuthPrompt {
                message: "Paste the authorization code (or full redirect URL):".to_string(),
                placeholder: None,
                allow_empty: false,
            })
            .ok()
            .and_then(|input| {
                validate_openai_codex_authorization_code(input.as_str(), flow.state.as_str()).ok()
            })
            .flatten()
    });
    let code =
        code.ok_or_else(|| AiError::InvalidResponse("Missing authorization code".to_string()))?;
    let token_result =
        exchange_openai_codex_authorization_code(code.as_str(), flow.verifier.as_str(), None)?;
    let OpenAiCodexTokenResult::Success {
        access,
        refresh,
        expires,
    } = token_result
    else {
        let OpenAiCodexTokenResult::Failed { message, .. } = token_result else {
            unreachable!();
        };
        return Err(AiError::InvalidResponse(message));
    };
    openai_codex_credentials(access, refresh, expires)
}

pub fn wait_for_openai_codex_code(expected_state: &str) -> AiResult<Option<String>> {
    let Ok(listener) = TcpListener::bind((OPENAI_CODEX_CALLBACK_HOST, OPENAI_CODEX_CALLBACK_PORT))
    else {
        return Ok(None);
    };
    let (stream, _) = listener
        .accept()
        .map_err(|error| AiError::Http(format!("OpenAI Codex callback 接收失败：{error}")))?;
    handle_openai_codex_callback_stream(stream, expected_state)
}

pub fn handle_openai_codex_callback_stream(
    mut stream: TcpStream,
    expected_state: &str,
) -> AiResult<Option<String>> {
    let mut buffer = [0u8; 8192];
    let bytes_read = stream
        .read(&mut buffer)
        .map_err(|error| AiError::Http(format!("读取 OAuth callback 失败：{error}")))?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, html, code) = openai_codex_callback_response(target, expected_state);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| AiError::Http(format!("写入 OAuth callback 响应失败：{error}")))?;
    Ok(code)
}

pub fn openai_codex_callback_response(
    target: &str,
    expected_state: &str,
) -> (&'static str, String, Option<String>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/auth/callback" {
        return (
            "404 Not Found",
            oauth_error_html("Callback route not found.", None),
            None,
        );
    }
    let params = parse_form_params(query);
    if params.get("state").map(String::as_str) != Some(expected_state) {
        return (
            "400 Bad Request",
            oauth_error_html("State mismatch.", None),
            None,
        );
    }
    let Some(code) = params.get("code").filter(|code| !code.is_empty()) else {
        return (
            "400 Bad Request",
            oauth_error_html("Missing authorization code.", None),
            None,
        );
    };
    (
        "200 OK",
        oauth_success_html("OpenAI authentication completed. You can close this window."),
        Some(code.clone()),
    )
}

pub fn validate_openai_codex_authorization_code(
    input: &str,
    expected_state: &str,
) -> AiResult<Option<String>> {
    let parsed = parse_openai_codex_authorization_input(input);
    if parsed
        .state
        .as_deref()
        .is_some_and(|state| state != expected_state)
    {
        return Err(AiError::InvalidResponse("State mismatch".to_string()));
    }
    Ok(parsed.code)
}

pub fn create_openai_codex_state() -> String {
    let mut bytes = [0u8; 16];
    if File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_err()
    {
        let fallback = current_time_millis().to_be_bytes();
        bytes.copy_from_slice(&fallback);
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn parse_openai_codex_authorization_input(input: &str) -> OpenAiCodexAuthorizationInput {
    let value = input.trim();
    if value.is_empty() {
        return OpenAiCodexAuthorizationInput {
            code: None,
            state: None,
        };
    }

    if let Some(query) = extract_query_like(value) {
        let params = parse_form_params(query);
        return OpenAiCodexAuthorizationInput {
            code: params.get("code").cloned(),
            state: params.get("state").cloned(),
        };
    }

    if let Some((code, state)) = value.split_once('#') {
        return OpenAiCodexAuthorizationInput {
            code: Some(code.to_string()),
            state: Some(state.to_string()),
        };
    }

    OpenAiCodexAuthorizationInput {
        code: Some(value.to_string()),
        state: None,
    }
}

pub fn create_openai_codex_authorization_url(
    verifier: &str,
    challenge: &str,
    state: &str,
    originator: Option<&str>,
) -> OpenAiCodexAuthorizationFlow {
    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
        ("scope", OPENAI_CODEX_SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", originator.unwrap_or("pi")),
    ];
    OpenAiCodexAuthorizationFlow {
        verifier: verifier.to_string(),
        state: state.to_string(),
        url: format!(
            "{}?{}",
            OPENAI_CODEX_AUTHORIZE_URL,
            form_encode_params(&params)
        ),
    }
}

pub fn exchange_openai_codex_authorization_code(
    code: &str,
    verifier: &str,
    redirect_uri: Option<&str>,
) -> AiResult<OpenAiCodexTokenResult> {
    let body = form_encode_params(&[
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        (
            "redirect_uri",
            redirect_uri.unwrap_or(OPENAI_CODEX_REDIRECT_URI),
        ),
    ]);
    let response = reqwest::blocking::Client::new()
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    token_response_result("exchange", response)
}

pub fn refresh_openai_codex_access_token(refresh_token: &str) -> AiResult<OpenAiCodexTokenResult> {
    let body = form_encode_params(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
    ]);
    let response = reqwest::blocking::Client::new()
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    token_response_result("refresh", response)
}

pub fn refresh_openai_codex_token(refresh_token: &str) -> AiResult<OAuthCredentials> {
    let result = refresh_openai_codex_access_token(refresh_token)?;
    let OpenAiCodexTokenResult::Success {
        access,
        refresh,
        expires,
    } = result
    else {
        let OpenAiCodexTokenResult::Failed { message, .. } = result else {
            unreachable!();
        };
        return Err(AiError::InvalidResponse(message));
    };
    openai_codex_credentials(access, refresh, expires)
}

pub fn openai_codex_credentials(
    access: impl Into<String>,
    refresh: impl Into<String>,
    expires: u128,
) -> AiResult<OAuthCredentials> {
    let access = access.into();
    let account_id = get_openai_codex_account_id(access.as_str()).ok_or_else(|| {
        AiError::InvalidResponse("Failed to extract accountId from token".to_string())
    })?;
    Ok(OAuthCredentials {
        access,
        refresh: refresh.into(),
        expires,
        extra: BTreeMap::from([(OPENAI_CODEX_ACCOUNT_ID_KEY.to_string(), account_id)]),
    })
}

pub fn get_openai_codex_account_id(access_token: &str) -> Option<String> {
    let payload = decode_jwt_payload(access_token)?;
    let auth = payload.get(JWT_CLAIM_PATH)?.as_object()?;
    auth.get("chatgpt_account_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn parse_openai_codex_token_response(
    action: &str,
    status: u16,
    ok: bool,
    body: &str,
    now_millis: u128,
) -> OpenAiCodexTokenResult {
    if !ok {
        return OpenAiCodexTokenResult::Failed {
            status: Some(status),
            message: format!(
                "OpenAI Codex token {action} failed ({status}): {}",
                if body.is_empty() {
                    "HTTP request failed"
                } else {
                    body
                }
            ),
        };
    }
    let Ok(response) = serde_json::from_str::<OpenAiCodexTokenResponse>(body) else {
        return OpenAiCodexTokenResult::Failed {
            status: None,
            message: format!("OpenAI Codex token {action} response missing fields: {body}"),
        };
    };
    let Some(access) = response
        .access_token
        .filter(|value| !value.trim().is_empty())
    else {
        return missing_token_fields(action, body);
    };
    let Some(refresh) = response
        .refresh_token
        .filter(|value| !value.trim().is_empty())
    else {
        return missing_token_fields(action, body);
    };
    let Some(expires_in) = response.expires_in else {
        return missing_token_fields(action, body);
    };
    OpenAiCodexTokenResult::Success {
        access,
        refresh,
        expires: now_millis.saturating_add(expires_in.saturating_mul(1000)),
    }
}

fn token_response_result(
    action: &str,
    response: reqwest::blocking::Response,
) -> AiResult<OpenAiCodexTokenResult> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| AiError::Http(error.to_string()))?;
    Ok(parse_openai_codex_token_response(
        action,
        status.as_u16(),
        status.is_success(),
        body.as_str(),
        current_time_millis(),
    ))
}

fn missing_token_fields(action: &str, body: &str) -> OpenAiCodexTokenResult {
    OpenAiCodexTokenResult::Failed {
        status: None,
        message: format!("OpenAI Codex token {action} response missing fields: {body}"),
    }
}

fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
    let mut parts = token.split('.');
    parts.next()?;
    let payload = parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let bytes = base64_url_decode(payload)?;
    serde_json::from_slice(&bytes).ok()
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

fn base64_url_decode(value: &str) -> Option<Vec<u8>> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut output = Vec::new();
    for byte in value.bytes().filter(|byte| *byte != b'=') {
        let value = base64_value(byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'-' | b'+' => Some(62),
        b'_' | b'/' => Some(63),
        _ => None,
    }
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
struct OpenAiCodexTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_authorization_inputs_like_pi() {
        assert_eq!(
            parse_openai_codex_authorization_input("code-value#state-value"),
            OpenAiCodexAuthorizationInput {
                code: Some("code-value".to_string()),
                state: Some("state-value".to_string()),
            }
        );
        assert_eq!(
            parse_openai_codex_authorization_input(
                "http://localhost:1455/auth/callback?code=abc%201&state=s%2B1"
            ),
            OpenAiCodexAuthorizationInput {
                code: Some("abc 1".to_string()),
                state: Some("s+1".to_string()),
            }
        );
        assert_eq!(
            parse_openai_codex_authorization_input("plain-code")
                .code
                .as_deref(),
            Some("plain-code")
        );
    }

    #[test]
    fn builds_authorization_url_with_expected_params() {
        let flow = create_openai_codex_authorization_url("verifier", "challenge", "state", None);

        assert_eq!(flow.verifier, "verifier");
        assert_eq!(flow.state, "state");
        assert!(flow.url.starts_with(OPENAI_CODEX_AUTHORIZE_URL));
        assert!(flow.url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
        assert!(flow
            .url
            .contains("scope=openid+profile+email+offline_access"));
        assert!(flow.url.contains("code_challenge=challenge"));
        assert!(flow.url.contains("originator=pi"));
    }

    #[test]
    fn extracts_account_id_from_jwt_payload() {
        let token = "header.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF8xMjMifX0.signature";

        assert_eq!(
            get_openai_codex_account_id(token).as_deref(),
            Some("acct_123")
        );
    }

    #[test]
    fn parses_token_success_response() {
        assert_eq!(
            parse_openai_codex_token_response(
                "refresh",
                200,
                true,
                r#"{"access_token":"access","refresh_token":"refresh","expires_in":10}"#,
                1000,
            ),
            OpenAiCodexTokenResult::Success {
                access: "access".to_string(),
                refresh: "refresh".to_string(),
                expires: 11_000,
            }
        );
    }

    #[test]
    fn parses_token_error_response() {
        assert_eq!(
            parse_openai_codex_token_response("refresh", 401, false, "denied", 1000),
            OpenAiCodexTokenResult::Failed {
                message: "OpenAI Codex token refresh failed (401): denied".to_string(),
                status: Some(401),
            }
        );
    }

    #[test]
    fn credentials_include_account_id() {
        let access = "header.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF8xMjMifX0.signature";
        let credentials = openai_codex_credentials(access, "refresh", 1000).expect("credentials");

        assert_eq!(credentials.access, access);
        assert_eq!(
            credentials.extra.get(OPENAI_CODEX_ACCOUNT_ID_KEY),
            Some(&"acct_123".to_string())
        );
    }

    #[test]
    fn callback_response_validates_route_state_and_code() {
        let (status, _, code) = openai_codex_callback_response("/missing?code=abc&state=s1", "s1");
        assert_eq!(status, "404 Not Found");
        assert_eq!(code, None);

        let (status, _, code) =
            openai_codex_callback_response("/auth/callback?code=abc&state=wrong", "s1");
        assert_eq!(status, "400 Bad Request");
        assert_eq!(code, None);

        let (status, _, code) = openai_codex_callback_response("/auth/callback?state=s1", "s1");
        assert_eq!(status, "400 Bad Request");
        assert_eq!(code, None);

        let (status, html, code) =
            openai_codex_callback_response("/auth/callback?code=abc%201&state=s1", "s1");
        assert_eq!(status, "200 OK");
        assert!(html.contains("Authentication successful"));
        assert_eq!(code.as_deref(), Some("abc 1"));
    }

    #[test]
    fn validates_manual_authorization_code_state() {
        assert_eq!(
            validate_openai_codex_authorization_code("code-value#state-value", "state-value")
                .expect("valid")
                .as_deref(),
            Some("code-value")
        );
        let error = validate_openai_codex_authorization_code("code-value#wrong", "state-value")
            .expect_err("state mismatch");
        assert!(error.to_string().contains("State mismatch"));
    }

    #[test]
    fn creates_hex_state() {
        let state = create_openai_codex_state();

        assert_eq!(state.len(), 32);
        assert!(state.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
