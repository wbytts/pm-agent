pub mod anthropic;
pub mod device_code;
pub mod github_copilot;
pub mod oauth_page;
pub mod openai_codex;
pub mod pkce;
pub mod registry;
pub mod types;

pub use anthropic::{
    anthropic_callback_response, anthropic_oauth_provider, create_anthropic_authorization_url,
    exchange_anthropic_authorization_code, handle_anthropic_callback_stream, login_anthropic,
    parse_anthropic_authorization_input, parse_anthropic_token_response, refresh_anthropic_token,
    validate_anthropic_authorization_input, wait_for_anthropic_code, AnthropicAuthorizationFlow,
    AnthropicAuthorizationInput, AnthropicOAuthProvider, ANTHROPIC_OAUTH_AUTHORIZE_URL,
    ANTHROPIC_OAUTH_CALLBACK_HOST, ANTHROPIC_OAUTH_CALLBACK_PATH, ANTHROPIC_OAUTH_CALLBACK_PORT,
    ANTHROPIC_OAUTH_CLIENT_ID, ANTHROPIC_OAUTH_PROVIDER_ID, ANTHROPIC_OAUTH_REDIRECT_URI,
    ANTHROPIC_OAUTH_SCOPES, ANTHROPIC_OAUTH_TOKEN_URL,
};
pub use device_code::{
    poll_oauth_device_code_flow, OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult,
    CANCEL_MESSAGE, DEFAULT_POLL_INTERVAL_SECONDS, MINIMUM_INTERVAL_MS,
    SLOW_DOWN_INTERVAL_INCREMENT_MS, SLOW_DOWN_TIMEOUT_MESSAGE, TIMEOUT_MESSAGE,
};
pub use github_copilot::{
    enable_all_github_copilot_models, enable_github_copilot_model, get_base_url_from_token,
    get_github_copilot_base_url, get_github_copilot_urls, github_copilot_credentials,
    github_copilot_headers, github_copilot_model_policy_url, github_copilot_oauth_provider,
    login_github_copilot, normalize_domain, parse_github_copilot_device_code_response,
    parse_github_copilot_token_response, parse_github_device_token_response,
    poll_for_github_access_token, refresh_github_copilot_token, start_github_copilot_device_flow,
    GitHubCopilotDeviceCode, GitHubCopilotModelEnableResult, GitHubCopilotOAuthProvider,
    GitHubCopilotUrls, GITHUB_COPILOT_CLIENT_ID, GITHUB_COPILOT_DEFAULT_BASE_URL,
    GITHUB_COPILOT_EDITOR_PLUGIN_VERSION, GITHUB_COPILOT_EDITOR_VERSION,
    GITHUB_COPILOT_ENTERPRISE_URL_KEY, GITHUB_COPILOT_INTEGRATION_ID, GITHUB_COPILOT_PROVIDER_ID,
    GITHUB_COPILOT_USER_AGENT,
};
pub use oauth_page::{oauth_error_html, oauth_success_html};
pub use openai_codex::{
    create_openai_codex_authorization_url, create_openai_codex_state,
    exchange_openai_codex_authorization_code, get_openai_codex_account_id, login_openai_codex,
    openai_codex_callback_response, openai_codex_credentials, openai_codex_oauth_provider,
    parse_openai_codex_authorization_input, parse_openai_codex_token_response,
    refresh_openai_codex_access_token, refresh_openai_codex_token,
    validate_openai_codex_authorization_code, wait_for_openai_codex_code,
    OpenAiCodexAuthorizationFlow, OpenAiCodexAuthorizationInput, OpenAiCodexOAuthProvider,
    OpenAiCodexTokenResult, OPENAI_CODEX_ACCOUNT_ID_KEY, OPENAI_CODEX_AUTHORIZE_URL,
    OPENAI_CODEX_CALLBACK_HOST, OPENAI_CODEX_CALLBACK_PORT, OPENAI_CODEX_CLIENT_ID,
    OPENAI_CODEX_PROVIDER_ID, OPENAI_CODEX_REDIRECT_URI, OPENAI_CODEX_SCOPE,
    OPENAI_CODEX_TOKEN_URL,
};
pub use pkce::{
    base64_url_encode, generate_pkce, pkce_challenge, pkce_from_verifier_bytes, PkcePair,
};
pub use registry::{
    get_oauth_api_key, get_oauth_provider, get_oauth_provider_info_list, get_oauth_providers,
    refresh_oauth_token, register_built_in_oauth_provider, register_oauth_provider,
    reset_oauth_providers, unregister_oauth_provider, OAuthApiKeyResult, OAuthProviderRegistry,
    SharedOAuthProvider,
};
pub use types::{
    OAuthAuthInfo, OAuthCredentials, OAuthDeviceCodeInfo, OAuthLoginCallbacks, OAuthPrompt,
    OAuthProviderId, OAuthProviderInfo, OAuthProviderInterface, OAuthSelectOption,
    OAuthSelectPrompt,
};
