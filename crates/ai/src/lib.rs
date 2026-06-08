pub mod conversation;
pub mod image_registry;
pub mod images;
pub mod providers;
pub mod proxy;
pub mod registry;
pub mod session_resources;
pub mod stream;
pub mod types;
pub mod utils;

mod env_api_keys;
mod event_stream;
mod image_model_catalog;
mod model_catalog;
mod model_options;

pub use conversation::{
    transform_messages, AssistantContentBlock, ImageContent, RichAssistantMessage, RichMessage,
    TextContent, ThinkingContent, ToolCall, ToolResultMessage, UserContentBlock, UserMessage,
    UserMessageContent, NON_VISION_TOOL_IMAGE_PLACEHOLDER, NON_VISION_USER_IMAGE_PLACEHOLDER,
};
pub use env_api_keys::{find_env_keys, get_env_api_key, provider_api_key_env_vars};
pub use event_stream::{
    create_assistant_message_event_stream, AssistantMessageEventStream, EventStream,
};
pub use image_registry::{
    ImagesApiProviderInfo, ImagesModelRegistry, ImagesProviderRegistry, RegisteredImagesProvider,
};
pub use images::{generate_images, generate_images_with_builtins};
pub use model_options::{
    calculate_cost, clamp_thinking_level, models_are_equal, supported_thinking_levels,
};
pub use providers::{
    adjust_max_tokens_for_thinking, bedrock_converse_stream_payload, bedrock_converse_stream_url,
    bedrock_endpoint_decision, bedrock_image_format, bedrock_model_match_candidates,
    bedrock_options_from_metadata, bedrock_stream_events_from_process_result,
    bedrock_supports_adaptive_thinking, bedrock_supports_native_xhigh_effort,
    bedrock_supports_prompt_caching, bedrock_supports_thinking_signature, build_base_options,
    build_bedrock_additional_model_request_fields, build_bedrock_system_prompt,
    build_copilot_dynamic_headers, build_google_vertex_adc_stream_url,
    build_google_vertex_http_options, build_google_vertex_params,
    build_google_vertex_simple_options, build_google_vertex_thinking_config,
    build_openai_completions_request, clamp_openai_prompt_cache_key, clamp_reasoning,
    complete_openai_responses_result, convert_bedrock_messages, convert_bedrock_tool_config,
    convert_google_messages, convert_google_tools, convert_openai_completions_messages,
    convert_openai_completions_messages_with_cache_retention, convert_openai_completions_tools,
    convert_openai_completions_tools_with_cache_retention, convert_openai_responses_messages,
    detect_openai_completions_compat, disabled_google_vertex_thinking_config,
    encode_openai_responses_text_signature_v1, faux_assistant_error, faux_assistant_message,
    faux_text, faux_thinking, faux_tool_call, gemini_major_version,
    google_vertex_base_url_includes_api_version, google_vertex_budget_for_level,
    google_vertex_gemini_3_thinking_level, has_copilot_vision_input, infer_copilot_initiator,
    is_anthropic_claude_bedrock_model, is_google_vertex_placeholder_api_key,
    is_gov_cloud_bedrock_target, is_thinking_part, is_valid_google_thought_signature,
    map_bedrock_stop_reason, map_bedrock_thinking_level_to_effort, map_google_stop_reason,
    map_google_tool_choice, map_openai_responses_stop_reason, normalize_bedrock_tool_call_id,
    openai_completions_stream_events_from_process_result,
    openai_responses_stream_events_from_process_result, parse_bedrock_converse_event_stream_body,
    parse_openai_completions_stream_chunks_from_value, parse_openai_responses_text_signature,
    process_anthropic_sse_events, process_anthropic_sse_events_with_options,
    process_bedrock_stream_events, process_openai_completions_stream_chunks,
    requires_google_tool_call_id, resolve_bedrock_cache_retention,
    resolve_google_thought_signature, resolve_google_vertex_access_token,
    resolve_google_vertex_api_key, resolve_google_vertex_custom_base_url,
    resolve_openai_responses_service_tier, retain_thought_signature,
    sanitize_google_schema_for_open_api, standard_bedrock_endpoint_region,
    supports_multimodal_function_response, usage_from_openai_responses_usage,
    AnthropicMessagesConfig, AnthropicMessagesProvider, AnthropicProcessedEvent,
    AnthropicRawSseEvent, AnthropicSseProcessOptions, AnthropicSseProcessResult, AwsCredentials,
    AzureOpenAiResponsesConfig, AzureOpenAiResponsesProvider, BedrockAdditionalModelRequestFields,
    BedrockCachePoint, BedrockCacheRetention, BedrockContentBlock, BedrockContentBlockDelta,
    BedrockContentBlockStart, BedrockConversationRole, BedrockConverseConfig,
    BedrockConverseProvider, BedrockEndpointDecision, BedrockImageBlock, BedrockImageFormat,
    BedrockImageSource, BedrockMessage, BedrockMessagesContext, BedrockOptions,
    BedrockProcessedEvent, BedrockReasoningContent, BedrockReasoningDelta, BedrockReasoningText,
    BedrockStreamEvent, BedrockStreamProcessResult, BedrockSystemContentBlock,
    BedrockThinkingDisplay, BedrockToolChoice, BedrockToolConfiguration, BedrockToolDefinition,
    BedrockToolResult, BedrockToolResultStatus, BedrockToolSpec, BedrockToolSpecBody,
    BedrockToolUse, BedrockToolUseDelta, BedrockToolUseStart, BedrockUsage, EchoProvider,
    FauxProvider, GoogleContent, GoogleFunctionCall, GoogleFunctionDeclaration,
    GoogleFunctionResponse, GoogleGenerativeAiConfig, GoogleGenerativeAiProvider, GoogleInlineData,
    GoogleMessagesContext, GooglePart, GooglePartThinkingState, GoogleThinkingLevel,
    GoogleToolChoiceMode, GoogleToolConfig, GoogleToolDefinition, GoogleVertexConfig,
    GoogleVertexContext, GoogleVertexCredentials, GoogleVertexGenerateContentConfig,
    GoogleVertexGenerateContentParams, GoogleVertexHttpOptions, GoogleVertexOptions,
    GoogleVertexProvider, GoogleVertexThinkingConfig, GoogleVertexThinkingOptions,
    MistralChatConfig, MistralChatProvider, OpenAiChatConfig, OpenAiChatMessage,
    OpenAiChatProvider, OpenAiCodexResponsesConfig, OpenAiCodexResponsesProvider,
    OpenAiCompatCacheControl, OpenAiCompletionsCompat, OpenAiCompletionsConfig,
    OpenAiCompletionsContentPart, OpenAiCompletionsContext, OpenAiCompletionsFunctionCall,
    OpenAiCompletionsFunctionDelta, OpenAiCompletionsMaxTokensField, OpenAiCompletionsMessage,
    OpenAiCompletionsMessageContent, OpenAiCompletionsOptions, OpenAiCompletionsProcessedEvent,
    OpenAiCompletionsPromptTokensDetails, OpenAiCompletionsProvider, OpenAiCompletionsRequest,
    OpenAiCompletionsStreamChoice, OpenAiCompletionsStreamChunk, OpenAiCompletionsStreamDelta,
    OpenAiCompletionsStreamProcessResult, OpenAiCompletionsThinkingFormat,
    OpenAiCompletionsToolCall, OpenAiCompletionsToolCallDelta, OpenAiCompletionsToolDefinition,
    OpenAiCompletionsToolFunction, OpenAiCompletionsUsage, OpenAiImageUrl,
    OpenAiResponsesCompleted, OpenAiResponsesCompletionResult, OpenAiResponsesConfig,
    OpenAiResponsesContent, OpenAiResponsesContext, OpenAiResponsesFunctionOutput,
    OpenAiResponsesInputItem, OpenAiResponsesInputTokensDetails, OpenAiResponsesProvider,
    OpenAiResponsesStatus, OpenAiResponsesStreamOptions, OpenAiResponsesTextPhase,
    OpenAiResponsesTextSignatureV1, OpenAiResponsesUsage, OpenRouterImagesConfig,
    OpenRouterImagesProvider, ParsedOpenAiResponsesTextSignature, SimpleStreamOptions,
    StreamOptions, ThinkingBudgets, ThinkingTokenBudget, GCP_VERTEX_CREDENTIALS_MARKER,
    GOOGLE_JSON_SCHEMA_META_DECLARATIONS, GOOGLE_VERTEX_API_VERSION,
    OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH,
};
pub use proxy::{
    create_proxy_message_event_stream, process_proxy_sse_text, process_proxy_sse_text_stream,
    stream_proxy, stream_proxy_event_stream, ProxyAssistantMessageEvent,
    ProxyAssistantMessageEventOutput, ProxyAssistantMessageEventStream, ProxyContext,
    ProxyMessageState, ProxySerializableStreamOptions, ProxyStreamOptions,
};
pub use registry::{ApiProviderInfo, ModelRegistry, ProviderRegistry, RegisteredProvider};
pub use session_resources::{
    cleanup_session_resources, register_session_resource_cleanup, SessionResourceCleanup,
    SessionResourceCleanupGuard,
};
pub use stream::{
    complete, complete_with_builtins, provider_events_to_stream, simple_request, stream,
    stream_with_builtins,
};
pub use types::{
    validate_images_model, validate_model, AiError, AiResult, AssistantImages, AssistantMessage,
    AssistantMessageEvent, AssistantStopReason, ContentBlock, ImagesContext, ImagesModel,
    ImagesProvider, ImagesStopReason, LanguageModelProvider, Message, MessageRole, Model,
    ModelCost, ModelInputKind, ModelReasoning, ModelThinkingLevel, StreamEvent, StreamRequest,
    ThinkingLevelMap, ToolDefinition, Usage, UsageCost,
};
pub use utils::{
    append_assistant_message_diagnostic, coerce_with_json_schema,
    create_assistant_message_diagnostic, create_error_diagnostic, create_message_diagnostic,
    diagnostic_error_from_message, extract_diagnostic_error, format_thrown_value,
    headers_to_record, is_context_overflow, parse_json_with_repair, parse_streaming_json,
    repair_json, sanitize_surrogates, short_hash, string_enum, validate_json_schema_value,
    validate_tool_arguments, validate_tool_call, AssistantMessageDiagnostic, AssistantMessageLike,
    DiagnosticErrorInfo, DiagnosticTarget, JsonSchemaObject, StringEnumOptions, ValidationError,
};

#[cfg(test)]
mod crate_root_export_tests {
    #[test]
    fn crate_root_exports_model_identity_helper_for_models_parity() {
        let _compare: fn(Option<&crate::Model>, Option<&crate::Model>) -> bool =
            crate::models_are_equal;
    }

    #[test]
    fn crate_root_exports_bedrock_runtime_helpers_for_provider_parity() {
        let _url: fn(&str, &str) -> String = crate::bedrock_converse_stream_url;
        let _parse: fn(&[u8]) -> Result<Vec<crate::BedrockStreamEvent>, String> =
            crate::parse_bedrock_converse_event_stream_body;
        let _adapter: fn(crate::BedrockStreamProcessResult) -> Vec<crate::StreamEvent> =
            crate::bedrock_stream_events_from_process_result;
        let _options: fn(
            &std::collections::BTreeMap<String, serde_json::Value>,
            Option<usize>,
        ) -> crate::BedrockOptions = crate::bedrock_options_from_metadata;
        let _credentials = crate::AwsCredentials {
            access_key_id: "key".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        };
    }

    #[test]
    fn crate_root_exports_google_vertex_adc_helpers_for_provider_parity() {
        let _url: fn(Option<&str>, &str, &str, &str) -> String =
            crate::build_google_vertex_adc_stream_url;
        let _token: fn(Option<&str>) -> Option<String> = crate::resolve_google_vertex_access_token;
    }
}
