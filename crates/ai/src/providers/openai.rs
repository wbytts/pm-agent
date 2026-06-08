use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

use crate::conversation::RichAssistantMessage;
use crate::providers::openai_completions_stream::{
    openai_completions_stream_events_from_process_result, process_openai_completions_stream_chunks,
};
use crate::providers::openai_completions_types::{
    resolve_openai_completions_cache_control, OpenAiCompletionsCompat, OpenAiCompletionsContext,
    OpenAiCompletionsMaxTokensField, OpenAiCompletionsMessage, OpenAiCompletionsMessageContent,
    OpenAiCompletionsOptions, OpenAiCompletionsRequest, OpenAiCompletionsStreamChunk,
    OpenAiCompletionsThinkingFormat, OpenAiCompletionsToolDefinition,
    OpenAiCompletionsToolFunction,
};
use crate::providers::{
    chat_role, clamp_openai_prompt_cache_key, convert_openai_completions_messages,
    is_cloudflare_provider, resolve_cloudflare_base_url_from_str,
};
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, LanguageModelProvider, StreamEvent,
    StreamRequest, ToolDefinition, Usage,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCompletionsConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct OpenAiCompletionsProvider {
    config: OpenAiCompletionsConfig,
}

impl OpenAiCompletionsProvider {
    pub fn new(config: OpenAiCompletionsConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(OpenAiCompletionsConfig {
            api_key: env::var("OPENAI_API_KEY").ok(),
            base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();
        if key.is_empty() {
            return Err(AiError::MissingApiKey("OPENAI_API_KEY".to_string()));
        }
        Ok(key.to_string())
    }
}

pub type OpenAiChatConfig = OpenAiCompletionsConfig;
pub type OpenAiChatProvider = OpenAiCompletionsProvider;

impl LanguageModelProvider for OpenAiCompletionsProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let raw_base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let base_url = openai_provider_base_url(&request.model.provider, raw_base_url)?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let payload = build_provider_openai_completions_request(&request, &base_url)?;

        let mut http_request = reqwest::blocking::Client::new()
            .post(url)
            .bearer_auth(api_key)
            .json(&payload);
        for (key, value) in &request.model.headers {
            http_request = http_request.header(key, value);
        }
        let response = http_request
            .send()
            .map_err(|error| AiError::Http(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(AiError::Http(format!("status={status}, body={body}")));
        }

        let body = response
            .text()
            .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
        openai_completions_sse_text_to_stream_events(&body, &request.model)
            .map_err(AiError::InvalidResponse)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiChatMessage {
    pub role: String,
    pub content: String,
}

impl From<OpenAiChatMessage> for OpenAiCompletionsMessage {
    fn from(message: OpenAiChatMessage) -> Self {
        Self {
            role: message.role,
            content: Some(OpenAiCompletionsMessageContent::Text(message.content)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            extra: serde_json::Map::new(),
        }
    }
}

fn build_provider_openai_completions_request(
    request: &StreamRequest,
    base_url: &str,
) -> AiResult<OpenAiCompletionsRequest> {
    let compat = detect_openai_completions_compat(&request.model.provider, base_url);
    let messages = if request.rich_messages.is_empty() {
        request
            .messages
            .iter()
            .map(|message| {
                OpenAiChatMessage {
                    role: chat_role(&message.role).to_string(),
                    content: message.content.clone(),
                }
                .into()
            })
            .collect::<Vec<OpenAiCompletionsMessage>>()
    } else {
        convert_openai_completions_messages(
            &request.model,
            &OpenAiCompletionsContext {
                system_prompt: openai_system_prompt_from_messages(&request.messages),
                messages: request.rich_messages.clone(),
            },
            &compat,
        )
    };
    let tools = (!request.tools.is_empty())
        .then(|| convert_openai_completions_tools(&request.tools, &compat));
    Ok(build_openai_completions_request(
        &request.model.id,
        messages,
        &OpenAiCompletionsOptions {
            max_tokens: request.model.max_tokens,
            tools,
            has_tools: !request.tools.is_empty(),
            ..OpenAiCompletionsOptions::default()
        },
        &compat,
    ))
}

fn openai_system_prompt_from_messages(messages: &[crate::types::Message]) -> Option<String> {
    let system_prompt = messages
        .iter()
        .filter(|message| message.role == crate::types::MessageRole::System)
        .map(|message| message.content.as_str())
        .filter(|content| !content.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!system_prompt.is_empty()).then_some(system_prompt)
}

fn openai_completions_sse_text_to_stream_events(
    input: &str,
    model: &crate::types::Model,
) -> Result<Vec<StreamEvent>, String> {
    let chunks = parse_openai_completions_sse_text(input)?;
    let processed = process_openai_completions_stream_chunks(
        &chunks,
        openai_completions_assistant_defaults(model),
        model,
        None::<fn(&mut Usage)>,
    )?;
    openai_completions_stream_events_from_process_result(processed)
}

fn parse_openai_completions_sse_text(
    input: &str,
) -> Result<Vec<OpenAiCompletionsStreamChunk>, String> {
    let mut chunks = Vec::new();
    let mut data_lines = Vec::<String>::new();

    for line in input.lines().chain(std::iter::once("")) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n").trim().to_string();
                data_lines.clear();
                if !data.is_empty() && data != "[DONE]" {
                    chunks.push(
                        serde_json::from_str::<OpenAiCompletionsStreamChunk>(&data).map_err(
                            |error| format!("OpenAI Completions SSE JSON 无效：{error}"),
                        )?,
                    );
                }
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }

    Ok(chunks)
}

fn openai_completions_assistant_defaults(model: &crate::types::Model) -> RichAssistantMessage {
    RichAssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        usage: Usage::default(),
        stop_reason: AssistantStopReason::Stop,
        error_message: None,
        diagnostics: Vec::new(),
        timestamp_millis: 0,
    }
}

fn openai_provider_base_url(provider: &str, raw_base_url: &str) -> AiResult<String> {
    if is_cloudflare_provider(provider) {
        resolve_cloudflare_base_url_from_str(provider, raw_base_url)
    } else {
        Ok(raw_base_url.to_string())
    }
}

pub fn detect_openai_completions_compat(provider: &str, base_url: &str) -> OpenAiCompletionsCompat {
    let is_zai = provider == "zai" || base_url.contains("api.z.ai");
    let is_together = provider == "together"
        || base_url.contains("api.together.ai")
        || base_url.contains("api.together.xyz");
    let is_moonshot = provider == "moonshotai"
        || provider == "moonshotai-cn"
        || base_url.contains("api.moonshot.");
    let is_cloudflare_ai_gateway =
        provider == "cloudflare-ai-gateway" || base_url.contains("gateway.ai.cloudflare.com");
    let is_cloudflare_workers_ai =
        provider == "cloudflare-workers-ai" || base_url.contains("api.cloudflare.com");
    let is_grok = provider == "xai" || base_url.contains("api.x.ai");
    let is_deepseek = provider == "deepseek" || base_url.contains("deepseek.com");
    let is_xiaomi = provider == "xiaomi"
        || provider.starts_with("xiaomi-token-plan-")
        || base_url.contains("xiaomimimo.com");
    let is_non_standard = provider == "cerebras"
        || base_url.contains("cerebras.ai")
        || is_grok
        || is_together
        || base_url.contains("chutes.ai")
        || is_deepseek
        || is_xiaomi
        || is_zai
        || is_moonshot
        || provider == "opencode"
        || base_url.contains("opencode.ai")
        || is_cloudflare_workers_ai
        || is_cloudflare_ai_gateway;
    let use_max_tokens =
        base_url.contains("chutes.ai") || is_moonshot || is_cloudflare_ai_gateway || is_together;

    OpenAiCompletionsCompat {
        supports_store: !is_non_standard,
        supports_developer_role: !is_non_standard,
        supports_reasoning_effort: !is_grok
            && !is_zai
            && !is_moonshot
            && !is_together
            && !is_cloudflare_ai_gateway,
        supports_usage_in_streaming: true,
        max_tokens_field: if use_max_tokens {
            OpenAiCompletionsMaxTokensField::MaxTokens
        } else {
            OpenAiCompletionsMaxTokensField::MaxCompletionTokens
        },
        supports_strict_mode: !is_moonshot && !is_together && !is_cloudflare_ai_gateway,
        supports_long_cache_retention: !(is_together
            || is_cloudflare_workers_ai
            || is_cloudflare_ai_gateway),
        requires_tool_result_name: false,
        requires_assistant_after_tool_result: false,
        zai_tool_stream: false,
        requires_reasoning_content_on_assistant_messages: is_deepseek || is_xiaomi,
        requires_thinking_as_text: false,
        cache_control_format: None,
        thinking_format: if is_deepseek || is_xiaomi {
            OpenAiCompletionsThinkingFormat::DeepSeek
        } else if is_zai {
            OpenAiCompletionsThinkingFormat::Zai
        } else if is_together {
            OpenAiCompletionsThinkingFormat::Together
        } else if provider == "openrouter" || base_url.contains("openrouter.ai") {
            OpenAiCompletionsThinkingFormat::OpenRouter
        } else {
            OpenAiCompletionsThinkingFormat::OpenAi
        },
    }
}

pub fn build_openai_completions_request(
    model_id: &str,
    messages: Vec<OpenAiCompletionsMessage>,
    options: &OpenAiCompletionsOptions,
    compat: &OpenAiCompletionsCompat,
) -> OpenAiCompletionsRequest {
    let tools = resolve_openai_completions_request_tools(&messages, options.tools.as_deref());
    let mut request = OpenAiCompletionsRequest {
        model: model_id.to_string(),
        messages,
        stream: Some(true),
        stream_options: compat
            .supports_usage_in_streaming
            .then(|| json!({ "include_usage": true })),
        store: compat.supports_store.then_some(false),
        max_tokens: None,
        max_completion_tokens: None,
        temperature: options.temperature,
        prompt_cache_key: resolve_openai_prompt_cache_key(options, compat),
        prompt_cache_retention: (options.cache_retention.as_deref() == Some("long")
            && compat.supports_long_cache_retention)
            .then(|| "24h".to_string()),
        reasoning_effort: None,
        reasoning: None,
        thinking: None,
        enable_thinking: None,
        chat_template_kwargs: None,
        tool_choice: options.tool_choice.clone(),
        tool_stream: (compat.zai_tool_stream && options.has_tools).then_some(true),
        tools,
    };
    if let Some(max_tokens) = options.max_tokens {
        match compat.max_tokens_field {
            OpenAiCompletionsMaxTokensField::MaxTokens => request.max_tokens = Some(max_tokens),
            OpenAiCompletionsMaxTokensField::MaxCompletionTokens => {
                request.max_completion_tokens = Some(max_tokens)
            }
        }
    }
    match compat.thinking_format {
        OpenAiCompletionsThinkingFormat::DeepSeek => {
            request.thinking = Some(json!({
                "type": if options.reasoning_effort.is_some() { "enabled" } else { "disabled" }
            }));
            if compat.supports_reasoning_effort {
                request.reasoning_effort = options.reasoning_effort.clone();
            }
        }
        OpenAiCompletionsThinkingFormat::OpenRouter => {
            if let Some(reasoning_effort) = &options.reasoning_effort {
                request.reasoning = Some(json!({ "effort": reasoning_effort }));
            }
        }
        OpenAiCompletionsThinkingFormat::Zai | OpenAiCompletionsThinkingFormat::Qwen => {
            request.enable_thinking = Some(options.reasoning_effort.is_some());
        }
        OpenAiCompletionsThinkingFormat::QwenChatTemplate => {
            request.chat_template_kwargs = Some(json!({
                "enable_thinking": options.reasoning_effort.is_some(),
                "preserve_thinking": true
            }));
        }
        _ => {
            if compat.supports_reasoning_effort {
                request.reasoning_effort = options.reasoning_effort.clone();
            }
        }
    }
    request
}

fn resolve_openai_completions_request_tools(
    messages: &[OpenAiCompletionsMessage],
    tools: Option<&[OpenAiCompletionsToolDefinition]>,
) -> Option<Vec<OpenAiCompletionsToolDefinition>> {
    if let Some(tools) = tools.filter(|tools| !tools.is_empty()) {
        return Some(tools.to_vec());
    }

    let has_tool_history = messages.iter().any(|message| {
        message.role == "tool"
            || message.tool_call_id.is_some()
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|tool_calls| !tool_calls.is_empty())
    });

    has_tool_history.then(Vec::new)
}

pub fn convert_openai_completions_tools(
    tools: &[ToolDefinition],
    compat: &OpenAiCompletionsCompat,
) -> Vec<OpenAiCompletionsToolDefinition> {
    convert_openai_completions_tools_with_cache_retention(tools, compat, None)
}

pub fn convert_openai_completions_tools_with_cache_retention(
    tools: &[ToolDefinition],
    compat: &OpenAiCompletionsCompat,
    cache_retention: Option<&str>,
) -> Vec<OpenAiCompletionsToolDefinition> {
    let mut converted = tools
        .iter()
        .map(|tool| OpenAiCompletionsToolDefinition {
            r#type: "function".to_string(),
            function: OpenAiCompletionsToolFunction {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
                strict: compat.supports_strict_mode.then_some(false),
            },
            cache_control: None,
        })
        .collect::<Vec<_>>();

    if let Some(cache_control) = resolve_openai_completions_cache_control(compat, cache_retention) {
        if let Some(tool) = converted.last_mut() {
            tool.cache_control = Some(cache_control);
        }
    }

    converted
}

fn resolve_openai_prompt_cache_key(
    options: &OpenAiCompletionsOptions,
    compat: &OpenAiCompletionsCompat,
) -> Option<String> {
    let cache_retention = options.cache_retention.as_deref().unwrap_or("short");
    if cache_retention == "none" {
        return None;
    }
    if cache_retention == "long" && !compat.supports_long_cache_retention {
        return None;
    }
    clamp_openai_prompt_cache_key(options.session_id.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichMessage, UserContentBlock, UserMessage, UserMessageContent,
    };
    use crate::types::{Message, MessageRole, ModelInputKind};

    #[test]
    fn keeps_normal_openai_base_url() {
        assert_eq!(
            openai_provider_base_url("openai", "https://api.openai.com/v1").expect("base url"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn accepts_resolved_cloudflare_base_url() {
        assert_eq!(
            openai_provider_base_url(
                "cloudflare-ai-gateway",
                "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"
            )
            .expect("base url"),
            "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"
        );
    }

    #[test]
    fn detects_openai_completions_compat_like_pi() {
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        assert!(compat.supports_store);
        assert_eq!(
            compat.max_tokens_field,
            OpenAiCompletionsMaxTokensField::MaxCompletionTokens
        );

        let compat = detect_openai_completions_compat("moonshotai", "https://api.moonshot.ai/v1");
        assert!(!compat.supports_store);
        assert_eq!(
            compat.max_tokens_field,
            OpenAiCompletionsMaxTokensField::MaxTokens
        );

        let compat = detect_openai_completions_compat("xiaomi", "https://api.xiaomimimo.com/v1");
        assert!(compat.requires_reasoning_content_on_assistant_messages);
        assert_eq!(
            compat.thinking_format,
            OpenAiCompletionsThinkingFormat::DeepSeek
        );
    }

    #[test]
    fn builds_openai_completions_request_options() {
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        let request = build_openai_completions_request(
            "gpt-4o",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                max_tokens: Some(128),
                temperature: Some(0.2),
                session_id: Some("session-cache".to_string()),
                cache_retention: Some("long".to_string()),
                reasoning_effort: Some("medium".to_string()),
                tool_choice: Some(json!("auto")),
                tools: None,
                has_tools: false,
            },
            &compat,
        );

        assert_eq!(request.stream, Some(true));
        assert_eq!(request.store, Some(false));
        assert_eq!(request.max_completion_tokens, Some(128));
        assert_eq!(request.prompt_cache_key.as_deref(), Some("session-cache"));
        assert_eq!(request.prompt_cache_retention.as_deref(), Some("24h"));
        assert_eq!(request.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(request.tool_choice, Some(json!("auto")));
    }

    #[test]
    fn builds_provider_chat_completions_stream_request_like_pi() {
        let request = StreamRequest {
            model: crate::types::Model {
                id: "gpt-4o-mini".to_string(),
                provider: "openai".to_string(),
                api: "openai-completions".to_string(),
                display_name: "GPT-4o mini".to_string(),
                context_window: 128_000,
                max_tokens: Some(128),
                ..crate::types::Model::default()
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        };

        let payload =
            build_provider_openai_completions_request(&request, "https://api.openai.com/v1")
                .expect("payload");

        assert_eq!(payload.stream, Some(true));
        assert_eq!(payload.store, Some(false));
        assert_eq!(
            payload.stream_options,
            Some(json!({ "include_usage": true }))
        );
        assert_eq!(payload.max_completion_tokens, Some(128));
    }

    #[test]
    fn builds_provider_chat_completions_request_prefers_rich_messages_like_pi() {
        let request = StreamRequest {
            model: crate::types::Model {
                id: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                api: "openai-completions".to_string(),
                display_name: "GPT-4o".to_string(),
                context_window: 128_000,
                input: vec![ModelInputKind::Image],
                ..crate::types::Model::default()
            },
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: "system".to_string(),
                },
                Message {
                    role: MessageRole::User,
                    content: "fallback simple".to_string(),
                },
            ],
            rich_messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(crate::conversation::TextContent {
                        text: "rich hello".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            })],
            tools: Vec::new(),
            metadata: Default::default(),
        };

        let payload =
            build_provider_openai_completions_request(&request, "https://api.openai.com/v1")
                .expect("payload");
        let value = serde_json::to_value(payload).expect("payload json");

        assert!(!value.to_string().contains("fallback simple"));
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][0]["content"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["messages"][1]["content"][0]["text"], "rich hello");
        assert_eq!(
            value["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn parses_chat_completions_sse_to_public_stream_events_like_pi() {
        let model = crate::types::Model {
            id: "gpt-4o-mini".to_string(),
            provider: "openai".to_string(),
            api: "openai-completions".to_string(),
            display_name: "GPT-4o mini".to_string(),
            context_window: 128_000,
            ..crate::types::Model::default()
        };
        let sse = "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"},\"finish_reason\":null}]}\n\n\
                   data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n\
                   data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":1}}}\n\n\
                   data: [DONE]\n\n";

        let events =
            openai_completions_sse_text_to_stream_events(sse, &model).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hel"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::TextDelta { text } if text == "lo"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::Usage { usage }
                if usage.input == 2
                    && usage.cache_read == 1
                    && usage.output == 2
                    && usage.total_tokens == 5
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finished { message }) if message.content == "hello"
        ));
    }

    #[test]
    fn builds_zai_tool_stream_request_option_like_pi() {
        let mut compat = detect_openai_completions_compat("zai", "https://api.z.ai/api/paas/v4");
        compat.zai_tool_stream = true;

        let request = build_openai_completions_request(
            "glm-5.1",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                has_tools: true,
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value.get("tool_stream"), Some(&json!(true)));
    }

    #[test]
    fn omits_zai_tool_stream_without_tools_like_pi() {
        let mut compat = detect_openai_completions_compat("zai", "https://api.z.ai/api/paas/v4");
        compat.zai_tool_stream = true;

        let request = build_openai_completions_request(
            "glm-5.1",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions::default(),
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert!(value.get("tool_stream").is_none());
    }

    #[test]
    fn builds_openrouter_reasoning_object_instead_of_reasoning_effort_like_pi() {
        let compat = detect_openai_completions_compat("openrouter", "https://openrouter.ai/api/v1");
        let request = build_openai_completions_request(
            "deepseek/deepseek-r1",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                reasoning_effort: Some("high".to_string()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value.get("reasoning"), Some(&json!({ "effort": "high" })));
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn builds_deepseek_thinking_object_and_reasoning_effort_like_pi() {
        let compat = detect_openai_completions_compat("xiaomi", "https://api.xiaomimimo.com/v1");
        let request = build_openai_completions_request(
            "mimo-v2.5-pro",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                reasoning_effort: Some("high".to_string()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value.get("thinking"), Some(&json!({ "type": "enabled" })));
        assert_eq!(value.get("reasoning_effort"), Some(&json!("high")));
    }

    #[test]
    fn builds_zai_enable_thinking_instead_of_reasoning_effort_like_pi() {
        let compat = detect_openai_completions_compat("zai", "https://api.z.ai/api/paas/v4");
        let request = build_openai_completions_request(
            "glm-5.1",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                reasoning_effort: Some("high".to_string()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value.get("enable_thinking"), Some(&json!(true)));
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn builds_qwen_chat_template_kwargs_like_pi() {
        let mut compat = detect_openai_completions_compat(
            "qwen",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        );
        compat.thinking_format = OpenAiCompletionsThinkingFormat::QwenChatTemplate;
        let request = build_openai_completions_request(
            "qwen3-coder-plus",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                reasoning_effort: Some("high".to_string()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(
            value.get("chat_template_kwargs"),
            Some(&json!({ "enable_thinking": true, "preserve_thinking": true }))
        );
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn converts_openai_completions_tools_with_strict_and_cache_control_like_pi() {
        let mut compat = detect_openai_completions_compat("openrouter", "https://example.com/v1");
        compat.cache_control_format = Some("anthropic".to_string());
        let tools = convert_openai_completions_tools(
            &[ToolDefinition {
                name: "read".to_string(),
                description: "Read a file".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            }],
            &compat,
        );

        let value = serde_json::to_value(tools).expect("tools serialize");
        assert_eq!(value[0]["type"], json!("function"));
        assert_eq!(value[0]["function"]["name"], json!("read"));
        assert_eq!(value[0]["function"]["strict"], json!(false));
        assert_eq!(value[0]["cache_control"], json!({ "type": "ephemeral" }));
    }

    #[test]
    fn converts_openai_completions_tools_with_cache_retention_like_pi() {
        let mut compat = detect_openai_completions_compat("openrouter", "https://example.com/v1");
        compat.cache_control_format = Some("anthropic".to_string());
        let tools = [ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({ "type": "object" }),
        }];

        let none_tools =
            convert_openai_completions_tools_with_cache_retention(&tools, &compat, Some("none"));
        let none_value = serde_json::to_value(none_tools).expect("tools serialize");
        assert!(none_value[0].get("cache_control").is_none());

        let long_tools =
            convert_openai_completions_tools_with_cache_retention(&tools, &compat, Some("long"));
        let long_value = serde_json::to_value(long_tools).expect("tools serialize");
        assert_eq!(
            long_value[0]["cache_control"],
            json!({ "type": "ephemeral", "ttl": "1h" })
        );
    }

    #[test]
    fn builds_openai_completions_request_omits_empty_tools_without_tool_history_like_pi() {
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        let request = build_openai_completions_request(
            "gpt-4o-mini",
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }
            .into()],
            &OpenAiCompletionsOptions {
                tools: Some(Vec::new()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert!(value.get("tools").is_none());
    }

    #[test]
    fn builds_openai_completions_request_emits_empty_tools_for_tool_history_like_pi() {
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        let request = build_openai_completions_request(
            "gpt-4o-mini",
            vec![OpenAiCompletionsMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![
                    crate::providers::openai_completions_types::OpenAiCompletionsToolCall {
                    id: "call_1".to_string(),
                    r#type: "function".to_string(),
                    function: crate::providers::openai_completions_types::OpenAiCompletionsFunctionCall {
                        name: "noop".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
                name: None,
                extra: serde_json::Map::new(),
            }],
            &OpenAiCompletionsOptions {
                tools: Some(Vec::new()),
                ..OpenAiCompletionsOptions::default()
            },
            &compat,
        );

        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value.get("tools"), Some(&json!([])));
    }
}
