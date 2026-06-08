pub mod diagnostics;
pub mod hash;
pub mod headers;
pub mod json_parse;
pub mod node_http_proxy;
pub mod oauth;
pub mod overflow;
pub mod sanitize_unicode;
pub mod typebox_helpers;
pub mod validation;

pub use diagnostics::{
    append_assistant_message_diagnostic, create_assistant_message_diagnostic,
    create_error_diagnostic, create_message_diagnostic, diagnostic_error_from_message,
    extract_diagnostic_error, format_thrown_value, AssistantMessageDiagnostic, DiagnosticErrorInfo,
    DiagnosticTarget,
};
pub use hash::short_hash;
pub use headers::headers_to_record;
pub use json_parse::{parse_json_with_repair, parse_streaming_json, repair_json};
pub use node_http_proxy::{resolve_http_proxy_url_for_target, UNSUPPORTED_PROXY_PROTOCOL_MESSAGE};
pub use oauth::{
    anthropic_callback_response, anthropic_oauth_provider, base64_url_encode,
    create_anthropic_authorization_url, create_openai_codex_authorization_url,
    create_openai_codex_state, enable_all_github_copilot_models, enable_github_copilot_model,
    exchange_anthropic_authorization_code, exchange_openai_codex_authorization_code, generate_pkce,
    get_base_url_from_token, get_github_copilot_base_url, get_github_copilot_urls,
    get_oauth_api_key, get_oauth_provider, get_oauth_provider_info_list, get_oauth_providers,
    get_openai_codex_account_id, github_copilot_credentials, github_copilot_headers,
    github_copilot_model_policy_url, github_copilot_oauth_provider,
    handle_anthropic_callback_stream, login_anthropic, login_github_copilot, login_openai_codex,
    normalize_domain, oauth_error_html, oauth_success_html, openai_codex_callback_response,
    openai_codex_credentials, openai_codex_oauth_provider, parse_anthropic_authorization_input,
    parse_anthropic_token_response, parse_github_copilot_device_code_response,
    parse_github_copilot_token_response, parse_github_device_token_response,
    parse_openai_codex_authorization_input, parse_openai_codex_token_response, pkce_challenge,
    pkce_from_verifier_bytes, poll_for_github_access_token, poll_oauth_device_code_flow,
    refresh_anthropic_token, refresh_github_copilot_token, refresh_oauth_token,
    refresh_openai_codex_access_token, refresh_openai_codex_token,
    register_built_in_oauth_provider, register_oauth_provider, reset_oauth_providers,
    start_github_copilot_device_flow, unregister_oauth_provider,
    validate_anthropic_authorization_input, validate_openai_codex_authorization_code,
    wait_for_anthropic_code, wait_for_openai_codex_code, AnthropicAuthorizationFlow,
    AnthropicAuthorizationInput, AnthropicOAuthProvider, GitHubCopilotDeviceCode,
    GitHubCopilotModelEnableResult, GitHubCopilotOAuthProvider, GitHubCopilotUrls,
    OAuthApiKeyResult, OAuthAuthInfo, OAuthCredentials, OAuthDeviceCodeInfo,
    OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, OAuthLoginCallbacks, OAuthPrompt,
    OAuthProviderId, OAuthProviderInfo, OAuthProviderInterface, OAuthProviderRegistry,
    OAuthSelectOption, OAuthSelectPrompt, OpenAiCodexAuthorizationFlow,
    OpenAiCodexAuthorizationInput, OpenAiCodexOAuthProvider, OpenAiCodexTokenResult, PkcePair,
    SharedOAuthProvider, ANTHROPIC_OAUTH_AUTHORIZE_URL, ANTHROPIC_OAUTH_CALLBACK_HOST,
    ANTHROPIC_OAUTH_CALLBACK_PATH, ANTHROPIC_OAUTH_CALLBACK_PORT, ANTHROPIC_OAUTH_CLIENT_ID,
    ANTHROPIC_OAUTH_PROVIDER_ID, ANTHROPIC_OAUTH_REDIRECT_URI, ANTHROPIC_OAUTH_SCOPES,
    ANTHROPIC_OAUTH_TOKEN_URL, GITHUB_COPILOT_CLIENT_ID, GITHUB_COPILOT_DEFAULT_BASE_URL,
    GITHUB_COPILOT_EDITOR_PLUGIN_VERSION, GITHUB_COPILOT_EDITOR_VERSION,
    GITHUB_COPILOT_ENTERPRISE_URL_KEY, GITHUB_COPILOT_INTEGRATION_ID, GITHUB_COPILOT_PROVIDER_ID,
    GITHUB_COPILOT_USER_AGENT, OPENAI_CODEX_ACCOUNT_ID_KEY, OPENAI_CODEX_AUTHORIZE_URL,
    OPENAI_CODEX_CALLBACK_HOST, OPENAI_CODEX_CALLBACK_PORT, OPENAI_CODEX_CLIENT_ID,
    OPENAI_CODEX_PROVIDER_ID, OPENAI_CODEX_REDIRECT_URI, OPENAI_CODEX_SCOPE,
    OPENAI_CODEX_TOKEN_URL,
};
pub use overflow::{get_overflow_patterns, is_context_overflow, AssistantMessageLike};
pub use sanitize_unicode::sanitize_surrogates;
pub use typebox_helpers::{string_enum, StringEnumOptions};
pub use validation::{
    coerce_with_json_schema, validate_json_schema_value, validate_tool_arguments,
    validate_tool_call, JsonSchemaObject, ValidationError,
};
