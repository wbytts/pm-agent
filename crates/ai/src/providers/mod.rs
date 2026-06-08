pub mod anthropic;
pub mod bedrock;
pub mod bedrock_messages;
pub mod bedrock_stream;
pub mod bedrock_types;
pub mod cloudflare;
pub mod github_copilot_headers;
pub mod google;
pub mod google_shared;
pub mod google_vertex;
pub mod local;
pub mod mistral;
pub mod openai;
pub mod openai_completions_messages;
pub mod openai_completions_stream;
pub mod openai_completions_types;
pub mod openai_prompt_cache;
pub mod openai_responses;
pub mod openai_responses_shared;
pub mod openrouter_images;
pub mod simple_options;

pub use anthropic::{
    process_anthropic_sse_events, process_anthropic_sse_events_with_options,
    AnthropicMessagesConfig, AnthropicMessagesProvider, AnthropicProcessedEvent,
    AnthropicRawSseEvent, AnthropicSseProcessOptions, AnthropicSseProcessResult,
};
pub use bedrock::{
    bedrock_converse_stream_payload, bedrock_converse_stream_url, bedrock_endpoint_decision,
    bedrock_model_match_candidates, bedrock_options_from_metadata,
    bedrock_supports_adaptive_thinking, bedrock_supports_native_xhigh_effort,
    bedrock_supports_prompt_caching, bedrock_supports_thinking_signature,
    build_bedrock_additional_model_request_fields, is_anthropic_claude_bedrock_model,
    is_gov_cloud_bedrock_target, map_bedrock_thinking_level_to_effort,
    resolve_bedrock_cache_retention, standard_bedrock_endpoint_region, AwsCredentials,
    BedrockConverseConfig, BedrockConverseProvider,
};
pub use bedrock_messages::{
    bedrock_image_format, build_bedrock_system_prompt, convert_bedrock_messages,
    convert_bedrock_tool_config, normalize_bedrock_tool_call_id,
};
pub use bedrock_stream::{
    bedrock_stream_events_from_process_result, map_bedrock_stop_reason,
    parse_bedrock_converse_event_stream_body, process_bedrock_stream_events,
};
pub use bedrock_types::{
    BedrockAdditionalModelRequestFields, BedrockCachePoint, BedrockCacheRetention,
    BedrockContentBlock, BedrockContentBlockDelta, BedrockContentBlockStart,
    BedrockConversationRole, BedrockEndpointDecision, BedrockImageBlock, BedrockImageFormat,
    BedrockImageSource, BedrockMessage, BedrockMessagesContext, BedrockOptions,
    BedrockProcessedEvent, BedrockReasoningContent, BedrockReasoningDelta, BedrockReasoningText,
    BedrockStreamEvent, BedrockStreamProcessResult, BedrockSystemContentBlock,
    BedrockThinkingDisplay, BedrockToolChoice, BedrockToolConfiguration, BedrockToolDefinition,
    BedrockToolResult, BedrockToolResultStatus, BedrockToolSpec, BedrockToolSpecBody,
    BedrockToolUse, BedrockToolUseDelta, BedrockToolUseStart, BedrockUsage,
};
pub use cloudflare::{
    is_cloudflare_provider, resolve_cloudflare_base_url, resolve_cloudflare_base_url_from_str,
    resolve_cloudflare_base_url_with_values, CLOUDFLARE_AI_GATEWAY_ANTHROPIC_BASE_URL,
    CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL, CLOUDFLARE_AI_GATEWAY_OPENAI_BASE_URL,
    CLOUDFLARE_WORKERS_AI_BASE_URL,
};
pub use github_copilot_headers::{
    build_copilot_dynamic_headers, has_copilot_vision_input, infer_copilot_initiator,
};
pub(crate) use google::google_sse_text_to_stream_events;
pub use google::{GoogleGenerativeAiConfig, GoogleGenerativeAiProvider};
pub use google_shared::{
    convert_google_messages, convert_google_tools, gemini_major_version, is_thinking_part,
    is_valid_google_thought_signature, map_google_stop_reason, map_google_tool_choice,
    requires_google_tool_call_id, resolve_google_thought_signature, retain_thought_signature,
    sanitize_google_schema_for_open_api, supports_multimodal_function_response, GoogleContent,
    GoogleFunctionCall, GoogleFunctionDeclaration, GoogleFunctionResponse, GoogleInlineData,
    GoogleMessagesContext, GooglePart, GooglePartThinkingState, GoogleThinkingLevel,
    GoogleToolChoiceMode, GoogleToolConfig, GoogleToolDefinition,
    GOOGLE_JSON_SCHEMA_META_DECLARATIONS,
};
pub use google_vertex::{
    build_google_vertex_adc_stream_url, build_google_vertex_http_options,
    build_google_vertex_params, build_google_vertex_simple_options, build_google_vertex_stream_url,
    build_google_vertex_thinking_config, disabled_google_vertex_thinking_config,
    google_vertex_base_url_includes_api_version, google_vertex_budget_for_level,
    google_vertex_gemini_3_thinking_level, google_vertex_rest_payload,
    is_google_vertex_placeholder_api_key, resolve_google_vertex_access_token,
    resolve_google_vertex_api_key, resolve_google_vertex_custom_base_url, GoogleVertexConfig,
    GoogleVertexContext, GoogleVertexCredentials, GoogleVertexGenerateContentConfig,
    GoogleVertexGenerateContentParams, GoogleVertexHttpOptions, GoogleVertexOptions,
    GoogleVertexProvider, GoogleVertexThinkingConfig, GoogleVertexThinkingOptions,
    GCP_VERTEX_CREDENTIALS_MARKER, GOOGLE_VERTEX_API_VERSION,
};
pub use local::{
    faux_assistant_error, faux_assistant_message, faux_text, faux_thinking, faux_tool_call,
    EchoProvider, FauxProvider,
};
pub use mistral::{MistralChatConfig, MistralChatProvider};
pub use openai::{
    build_openai_completions_request, convert_openai_completions_tools,
    convert_openai_completions_tools_with_cache_retention, detect_openai_completions_compat,
    OpenAiChatConfig, OpenAiChatMessage, OpenAiChatProvider, OpenAiCompletionsConfig,
    OpenAiCompletionsProvider,
};
pub use openai_completions_messages::{
    convert_openai_completions_messages, convert_openai_completions_messages_with_cache_retention,
};
pub use openai_completions_stream::{
    openai_completions_stream_events_from_process_result,
    parse_openai_completions_stream_chunks_from_value, process_openai_completions_stream_chunks,
};
pub use openai_completions_types::{
    resolve_openai_completions_cache_control, OpenAiCompatCacheControl, OpenAiCompletionsCompat,
    OpenAiCompletionsContentPart, OpenAiCompletionsContext, OpenAiCompletionsFunctionCall,
    OpenAiCompletionsFunctionDelta, OpenAiCompletionsMaxTokensField, OpenAiCompletionsMessage,
    OpenAiCompletionsMessageContent, OpenAiCompletionsOptions, OpenAiCompletionsProcessedEvent,
    OpenAiCompletionsPromptTokensDetails, OpenAiCompletionsRequest, OpenAiCompletionsStreamChoice,
    OpenAiCompletionsStreamChunk, OpenAiCompletionsStreamDelta,
    OpenAiCompletionsStreamProcessResult, OpenAiCompletionsThinkingFormat,
    OpenAiCompletionsToolCall, OpenAiCompletionsToolCallDelta, OpenAiCompletionsToolDefinition,
    OpenAiCompletionsToolFunction, OpenAiCompletionsUsage, OpenAiImageUrl,
};
pub use openai_prompt_cache::{clamp_openai_prompt_cache_key, OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH};
pub use openai_responses::{
    AzureOpenAiResponsesConfig, AzureOpenAiResponsesProvider, OpenAiCodexResponsesConfig,
    OpenAiCodexResponsesProvider, OpenAiResponsesConfig, OpenAiResponsesProvider,
};
pub use openai_responses_shared::{
    complete_openai_responses_result, convert_openai_responses_messages,
    encode_openai_responses_text_signature_v1, map_openai_responses_stop_reason,
    openai_responses_stream_events_from_process_result, parse_openai_responses_text_signature,
    resolve_openai_responses_service_tier, usage_from_openai_responses_usage,
    ConvertResponsesMessagesOptions, OpenAiResponsesCompleted, OpenAiResponsesCompletionResult,
    OpenAiResponsesContent, OpenAiResponsesContext, OpenAiResponsesFunctionOutput,
    OpenAiResponsesInputItem, OpenAiResponsesInputTokensDetails, OpenAiResponsesStatus,
    OpenAiResponsesStreamOptions, OpenAiResponsesTextPhase, OpenAiResponsesTextSignatureV1,
    OpenAiResponsesUsage, ParsedOpenAiResponsesTextSignature,
};
pub use openrouter_images::{OpenRouterImagesConfig, OpenRouterImagesProvider};
pub use simple_options::{
    adjust_max_tokens_for_thinking, build_base_options, clamp_reasoning, SimpleStreamOptions,
    StreamOptions, ThinkingBudgets, ThinkingTokenBudget,
};

use crate::types::MessageRole;

fn chat_role(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}
