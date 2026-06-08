use serde::{Deserialize, Serialize};
use std::env;

use crate::conversation::{
    AssistantContentBlock, RichAssistantMessage, RichMessage, TextContent, UserMessage,
    UserMessageContent,
};
use crate::providers::google_shared::{
    convert_google_messages, is_thinking_part, map_google_stop_reason, retain_thought_signature,
    GoogleContent, GoogleMessagesContext, GooglePart, GooglePartThinkingState,
};
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, LanguageModelProvider, Message,
    MessageRole, StreamEvent, StreamRequest, StreamToolCall, Usage,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerativeAiConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct GoogleGenerativeAiProvider {
    config: GoogleGenerativeAiConfig,
}

impl GoogleGenerativeAiProvider {
    pub fn new(config: GoogleGenerativeAiConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(GoogleGenerativeAiConfig {
            api_key: env::var("GOOGLE_API_KEY")
                .or_else(|_| env::var("GEMINI_API_KEY"))
                .ok(),
            base_url: env::var("GOOGLE_GENERATIVE_AI_BASE_URL")
                .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();
        if key.is_empty() {
            return Err(AiError::MissingApiKey(
                "GOOGLE_API_KEY 或 GEMINI_API_KEY".to_string(),
            ));
        }
        Ok(key.to_string())
    }
}

impl LanguageModelProvider for GoogleGenerativeAiProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let url = google_stream_generate_content_url(base_url, &request.model.id, &api_key);
        let rich_messages = request.rich_messages;
        let (system_prompt, messages) = split_google_system_messages(request.messages);
        let context_messages = if rich_messages.is_empty() {
            messages
                .into_iter()
                .map(simple_message_to_google_rich_message)
                .collect()
        } else {
            rich_messages
        };
        let contents = convert_google_messages(
            &request.model,
            &GoogleMessagesContext {
                messages: context_messages,
            },
        );

        let payload = GoogleGenerateContentRequest {
            system_instruction: system_prompt.map(|prompt| GoogleSystemInstruction {
                parts: vec![GooglePart {
                    text: Some(prompt),
                    inline_data: None,
                    thought: None,
                    thought_signature: None,
                    function_call: None,
                    function_response: None,
                }],
            }),
            contents,
        };

        let response = reqwest::blocking::Client::new()
            .post(url)
            .json(&payload)
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
        google_sse_text_to_stream_events(&body).map_err(AiError::InvalidResponse)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGenerateContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GoogleSystemInstruction>,
    contents: Vec<GoogleContent>,
}

#[derive(Debug, Serialize)]
struct GoogleSystemInstruction {
    parts: Vec<GooglePart>,
}

#[derive(Debug, Deserialize)]
struct GoogleGenerateContentResponse {
    #[serde(default, rename = "responseId")]
    response_id: Option<String>,
    candidates: Vec<GoogleCandidate>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GoogleUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GoogleCandidate {
    content: GoogleContent,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleUsageMetadata {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    cached_content_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
    #[serde(default)]
    thoughts_token_count: u64,
    #[serde(default)]
    total_token_count: u64,
}

fn google_stream_generate_content_url(base_url: &str, model_id: &str, api_key: &str) -> String {
    format!(
        "{}/models/{}:streamGenerateContent?alt=sse&key={}",
        base_url.trim_end_matches('/'),
        model_id,
        api_key
    )
}

fn split_google_system_messages(messages: Vec<Message>) -> (Option<String>, Vec<Message>) {
    let mut system = Vec::new();
    let mut rest = Vec::new();
    for message in messages {
        if message.role == MessageRole::System {
            system.push(message.content);
        } else {
            rest.push(message);
        }
    }
    let system_prompt = if system.is_empty() {
        None
    } else {
        Some(system.join("\n\n"))
    };
    (system_prompt, rest)
}

fn simple_message_to_google_rich_message(message: Message) -> RichMessage {
    match message.role {
        MessageRole::Assistant => RichMessage::Assistant(RichAssistantMessage {
            content: vec![AssistantContentBlock::Text(TextContent {
                text: message.content,
                text_signature: None,
            })],
            api: "google-generative-ai".to_string(),
            provider: "google".to_string(),
            model: String::new(),
            response_model: None,
            response_id: None,
            usage: Usage::default(),
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        }),
        MessageRole::System | MessageRole::User | MessageRole::Tool => {
            RichMessage::User(UserMessage {
                content: UserMessageContent::Text(message.content),
                timestamp_millis: 0,
            })
        }
    }
}

pub(crate) fn google_sse_text_to_stream_events(input: &str) -> Result<Vec<StreamEvent>, String> {
    let chunks = parse_google_sse_text(input)?;
    let mut events = Vec::new();
    let mut content = String::new();
    let mut current_thinking: Option<GoogleThinkingBlock> = None;
    let mut next_content_index = 0usize;
    let mut current_text_open = false;
    let mut generated_tool_call_counter = 0usize;
    let mut response_id = None;
    let mut stop_reason = AssistantStopReason::Stop;

    for chunk in chunks {
        if response_id.is_none() {
            response_id = chunk.response_id.filter(|value| !value.is_empty());
        }
        for candidate in chunk.candidates {
            if let Some(reason) = candidate.finish_reason.as_deref() {
                stop_reason = map_google_stop_reason(reason);
            }
            for part in candidate.content.parts {
                if let Some(text) = part.text.clone().filter(|text| !text.is_empty()) {
                    let thinking_state = GooglePartThinkingState {
                        thought: part.thought.unwrap_or(false),
                        thought_signature: part.thought_signature.clone(),
                    };
                    if is_thinking_part(&thinking_state) {
                        if current_text_open {
                            current_text_open = false;
                        }
                        let thinking = current_thinking.get_or_insert_with(|| {
                            let content_index = next_content_index;
                            next_content_index += 1;
                            events.push(StreamEvent::ThinkingStart { content_index });
                            GoogleThinkingBlock {
                                content_index,
                                content: String::new(),
                                thinking_signature: None,
                            }
                        });
                        thinking.content.push_str(&text);
                        thinking.thinking_signature = retain_thought_signature(
                            thinking.thinking_signature.as_deref(),
                            thinking_state.thought_signature.as_deref(),
                        );
                        events.push(StreamEvent::ThinkingDelta {
                            content_index: thinking.content_index,
                            delta: text,
                        });
                    } else {
                        finish_google_thinking_block(&mut events, &mut current_thinking);
                        if !current_text_open {
                            current_text_open = true;
                            next_content_index += 1;
                        }
                        content.push_str(&text);
                        events.push(StreamEvent::TextDelta { text });
                    }
                }
                if let Some(function_call) = part.function_call {
                    finish_google_thinking_block(&mut events, &mut current_thinking);
                    current_text_open = false;
                    generated_tool_call_counter += 1;
                    let content_index = next_content_index;
                    next_content_index += 1;
                    let delta = serde_json::to_string(&function_call.args)
                        .map_err(|error| format!("Google functionCall 参数 JSON 无效：{error}"))?;
                    let tool_call = StreamToolCall {
                        id: function_call.id.unwrap_or_else(|| {
                            format!("{}_{}", function_call.name, generated_tool_call_counter)
                        }),
                        name: function_call.name,
                        arguments: function_call.args,
                        thought_signature: part.thought_signature,
                    };
                    events.push(StreamEvent::ToolCallStart { content_index });
                    events.push(StreamEvent::ToolCallDelta {
                        content_index,
                        delta,
                    });
                    events.push(StreamEvent::ToolCallEnd {
                        content_index,
                        tool_call,
                    });
                    stop_reason = AssistantStopReason::ToolUse;
                }
            }
        }
        if let Some(usage_metadata) = chunk.usage_metadata {
            events.push(StreamEvent::Usage {
                usage: Usage {
                    input: usage_metadata
                        .prompt_token_count
                        .saturating_sub(usage_metadata.cached_content_token_count),
                    output: usage_metadata.candidates_token_count
                        + usage_metadata.thoughts_token_count,
                    cache_read: usage_metadata.cached_content_token_count,
                    cache_write: 0,
                    total_tokens: usage_metadata.total_token_count,
                    ..Usage::default()
                },
            });
        }
    }
    finish_google_thinking_block(&mut events, &mut current_thinking);

    if content.is_empty() {
        return Err("Google Generative AI 输出文本缺失".to_string());
    }
    events.push(StreamEvent::RichFinished {
        message: RichAssistantMessage {
            content: vec![AssistantContentBlock::Text(TextContent {
                text: content,
                text_signature: None,
            })],
            api: "google-generative-ai".to_string(),
            provider: "google".to_string(),
            model: String::new(),
            response_model: None,
            response_id,
            usage: Usage::default(),
            stop_reason,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        },
    });
    Ok(events)
}

struct GoogleThinkingBlock {
    content_index: usize,
    content: String,
    thinking_signature: Option<String>,
}

fn finish_google_thinking_block(
    events: &mut Vec<StreamEvent>,
    current_thinking: &mut Option<GoogleThinkingBlock>,
) {
    let Some(thinking) = current_thinking.take() else {
        return;
    };
    events.push(StreamEvent::ThinkingEnd {
        content_index: thinking.content_index,
        content: thinking.content,
        thinking_signature: thinking.thinking_signature,
        redacted: false,
    });
}

fn parse_google_sse_text(input: &str) -> Result<Vec<GoogleGenerateContentResponse>, String> {
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
                        serde_json::from_str::<GoogleGenerateContentResponse>(&data)
                            .map_err(|error| format!("Google SSE JSON 无效：{error}"))?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichMessage, TextContent, UserContentBlock, UserMessage, UserMessageContent,
    };
    use crate::types::{Model, ModelInputKind};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn builds_google_stream_generate_content_url_like_pi() {
        assert_eq!(
            google_stream_generate_content_url(
                "https://generativelanguage.googleapis.com/v1beta",
                "gemini-2.5-pro",
                "key_123",
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse&key=key_123"
        );
    }

    #[test]
    fn google_runtime_prefers_stream_request_rich_messages_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text.starts_with(
                "POST /models/gemini-2.5-pro:streamGenerateContent?alt=sse&key=google-key HTTP/1.1"
            ));
            assert!(!request_text.contains("fallback simple"));
            assert!(request_text.contains("\"text\":\"rich hello\""));
            assert!(request_text.contains("\"inlineData\""));
            assert!(request_text.contains("\"mimeType\":\"image/png\""));
            assert!(request_text.contains("\"data\":\"abc\""));

            let body =
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let provider = GoogleGenerativeAiProvider::new(GoogleGenerativeAiConfig {
            api_key: Some("google-key".to_string()),
            base_url: format!("http://{address}"),
        });
        let events = provider
            .stream(StreamRequest {
                model: Model {
                    id: "gemini-2.5-pro".to_string(),
                    provider: "google".to_string(),
                    api: "google-generative-ai".to_string(),
                    display_name: "Gemini 2.5 Pro".to_string(),
                    context_window: 1_000_000,
                    input: vec![ModelInputKind::Image],
                    ..Model::default()
                },
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "fallback simple".to_string(),
                }],
                rich_messages: vec![RichMessage::User(UserMessage {
                    content: UserMessageContent::Blocks(vec![
                        UserContentBlock::Text(TextContent {
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
            })
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
    }

    #[test]
    fn parses_google_sse_text_to_public_stream_events_like_pi() {
        let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hel\"}]}}]}\n\n\
                   data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo\"}]}}]}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hel"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::TextDelta { text } if text == "lo"
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::RichFinished { message }) if crate::stream::rich_assistant_text(message) == "hello"
        ));
    }

    #[test]
    fn google_stream_preserves_response_id_like_pi() {
        let sse = "data: {\"responseId\":\"resp_google_1\",\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");
        let stream = crate::provider_events_to_stream(events).expect("stream");

        assert_eq!(
            stream
                .result()
                .and_then(|message| message.response_id.as_deref()),
            Some("resp_google_1")
        );
    }

    #[test]
    fn google_stream_maps_finish_reason_like_pi() {
        let sse = "data: {\"candidates\":[{\"finishReason\":\"MAX_TOKENS\",\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");
        let stream = crate::provider_events_to_stream(events).expect("stream");

        assert_eq!(
            stream.result().map(|message| message.stop_reason.clone()),
            Some(AssistantStopReason::Length)
        );
    }

    #[test]
    fn parses_google_usage_metadata_to_public_usage_event_like_pi() {
        let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ok\"}]}}],\
                   \"usageMetadata\":{\"promptTokenCount\":12,\"cachedContentTokenCount\":5,\
                   \"candidatesTokenCount\":7,\"thoughtsTokenCount\":3,\"totalTokenCount\":22}}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[1],
            StreamEvent::Usage { usage }
                if usage.input == 7
                    && usage.output == 10
                    && usage.cache_read == 5
                    && usage.cache_write == 0
                    && usage.total_tokens == 22
        ));
    }

    #[test]
    fn parses_google_thought_parts_to_public_thinking_events_like_pi() {
        let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[\
                   {\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"c2ln\"},\
                   {\"text\":\"answer\"}]}}]}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::ThinkingStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ThinkingDelta { content_index, delta }
                if *content_index == 0 && delta == "plan"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ThinkingEnd {
                content_index,
                content,
                thinking_signature,
                redacted,
            } if *content_index == 0
                && content == "plan"
                && thinking_signature.as_deref() == Some("c2ln")
                && !redacted
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::TextDelta { text } if text == "answer"
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::RichFinished { message }) if crate::stream::rich_assistant_text(message) == "answer"
        ));
    }

    #[test]
    fn parses_google_function_calls_to_public_tool_events_like_pi() {
        let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[\
                   {\"text\":\"checking\"},\
                   {\"functionCall\":{\"id\":\"call_1\",\"name\":\"read\",\"args\":{\"path\":\"README.md\"}},\
                    \"thoughtSignature\":\"dG9vbA==\"}]}}]}\n\n";

        let events = google_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "checking"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ToolCallStart { content_index } if *content_index == 1
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ToolCallDelta {
                content_index,
                delta,
            } if *content_index == 1 && delta == "{\"path\":\"README.md\"}"
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::ToolCallEnd {
                content_index,
                tool_call,
            } if *content_index == 1
                && tool_call.id == "call_1"
                && tool_call.name == "read"
                && tool_call.arguments["path"] == serde_json::json!("README.md")
                && tool_call.thought_signature.as_deref() == Some("dG9vbA==")
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::RichFinished { message }) if crate::stream::rich_assistant_text(message) == "checking"
        ));
    }
}
