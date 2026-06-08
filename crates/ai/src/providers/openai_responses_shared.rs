use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::conversation::{
    transform_messages, AssistantContentBlock, RichAssistantMessage, RichMessage, TextContent,
    ThinkingContent, ToolCall, UserContentBlock, UserMessageContent,
};
use crate::types::{
    AssistantStopReason, Message, MessageRole, StreamEvent, StreamToolCall, Usage, UsageCost,
};
use crate::types::{Model, ModelInputKind};
use crate::utils::{sanitize_surrogates, short_hash};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiResponsesTextPhase {
    Commentary,
    FinalAnswer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiResponsesTextSignatureV1 {
    pub v: u8,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<OpenAiResponsesTextPhase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOpenAiResponsesTextSignature {
    pub id: String,
    pub phase: Option<OpenAiResponsesTextPhase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesContent {
    InputText {
        text: String,
    },
    InputImage {
        detail: String,
        image_url: String,
    },
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Vec<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OpenAiResponsesFunctionOutput {
    Text(String),
    Parts(Vec<OpenAiResponsesContent>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesInputItem {
    Message {
        role: String,
        content: Vec<OpenAiResponsesContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<OpenAiResponsesTextPhase>,
    },
    FunctionCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: OpenAiResponsesFunctionOutput,
    },
    Reasoning {
        #[serde(flatten)]
        item: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiResponsesContext {
    pub system_prompt: Option<String>,
    pub messages: Vec<RichMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertResponsesMessagesOptions {
    pub include_system_prompt: bool,
    pub allowed_tool_call_providers: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiResponsesStatus {
    Completed,
    Incomplete,
    Failed,
    Cancelled,
    InProgress,
    Queued,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponsesInputTokensDetails {
    #[serde(alias = "cached_tokens")]
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponsesUsage {
    #[serde(alias = "input_tokens")]
    pub input_tokens: Option<u64>,
    #[serde(alias = "output_tokens")]
    pub output_tokens: Option<u64>,
    #[serde(alias = "total_tokens")]
    pub total_tokens: Option<u64>,
    #[serde(alias = "input_tokens_details")]
    pub input_tokens_details: Option<OpenAiResponsesInputTokensDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponsesCompleted {
    pub status: Option<OpenAiResponsesStatus>,
    pub usage: Option<OpenAiResponsesUsage>,
    #[serde(alias = "service_tier")]
    pub service_tier: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiResponsesStreamOptions {
    pub service_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiResponsesCompletionResult {
    pub stop_reason: AssistantStopReason,
    pub usage: Usage,
    pub service_tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesOutputContent {
    OutputText { text: String },
    Refusal { refusal: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponsesReasoningPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesOutputItem {
    Reasoning {
        id: String,
        #[serde(default)]
        summary: Vec<OpenAiResponsesReasoningPart>,
        #[serde(default)]
        content: Vec<OpenAiResponsesReasoningPart>,
    },
    Message {
        id: String,
        #[serde(default)]
        content: Vec<OpenAiResponsesOutputContent>,
        #[serde(default)]
        phase: Option<OpenAiResponsesTextPhase>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        #[serde(default)]
        arguments: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesStreamContentPart {
    OutputText { text: String },
    Refusal { refusal: String },
    Other {},
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiResponsesStreamEvent {
    ResponseCreated {
        response_id: String,
    },
    OutputItemAdded {
        item: OpenAiResponsesOutputItem,
    },
    ReasoningSummaryPartAdded {
        part: OpenAiResponsesReasoningPart,
    },
    ReasoningSummaryTextDelta {
        delta: String,
    },
    ReasoningSummaryPartDone,
    ReasoningTextDelta {
        delta: String,
    },
    ContentPartAdded {
        part: OpenAiResponsesStreamContentPart,
    },
    OutputTextDelta {
        delta: String,
    },
    RefusalDelta {
        delta: String,
    },
    FunctionCallArgumentsDelta {
        delta: String,
    },
    FunctionCallArgumentsDone {
        arguments: String,
    },
    OutputItemDone {
        item: OpenAiResponsesOutputItem,
    },
    ResponseCompleted {
        response_id: Option<String>,
        completed: OpenAiResponsesCompleted,
    },
    Error {
        code: Option<String>,
        message: String,
    },
    ResponseFailed {
        error_code: Option<String>,
        error_message: Option<String>,
        incomplete_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenAiResponsesProcessedEvent {
    ResponseCreated {
        response_id: String,
    },
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        content: String,
    },
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
        content: String,
    },
    ToolCallStart {
        content_index: usize,
    },
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        tool_call: ToolCall,
    },
    Completed {
        result: OpenAiResponsesCompletionResult,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiResponsesStreamProcessResult {
    pub assistant: RichAssistantMessage,
    pub events: Vec<OpenAiResponsesProcessedEvent>,
}

pub fn openai_responses_stream_events_from_process_result(
    result: OpenAiResponsesStreamProcessResult,
) -> Result<Vec<StreamEvent>, String> {
    let mut events = Vec::new();
    let mut usage = result.assistant.usage.clone();

    for event in &result.events {
        match event {
            OpenAiResponsesProcessedEvent::ResponseCreated { .. }
            | OpenAiResponsesProcessedEvent::TextStart { .. } => {}
            OpenAiResponsesProcessedEvent::ThinkingStart { content_index } => {
                events.push(StreamEvent::ThinkingStart {
                    content_index: *content_index,
                });
            }
            OpenAiResponsesProcessedEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ThinkingDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            OpenAiResponsesProcessedEvent::ThinkingEnd {
                content_index,
                content,
            } => {
                let (thinking_signature, redacted) = result
                    .assistant
                    .content
                    .get(*content_index)
                    .and_then(|block| match block {
                        AssistantContentBlock::Thinking(thinking) => {
                            Some((thinking.thinking_signature.clone(), thinking.redacted))
                        }
                        _ => None,
                    })
                    .unwrap_or((None, false));
                events.push(StreamEvent::ThinkingEnd {
                    content_index: *content_index,
                    content: content.clone(),
                    thinking_signature,
                    redacted,
                });
            }
            OpenAiResponsesProcessedEvent::TextDelta { delta, .. } => {
                events.push(StreamEvent::TextDelta {
                    text: delta.clone(),
                });
            }
            OpenAiResponsesProcessedEvent::TextEnd { .. } => {}
            OpenAiResponsesProcessedEvent::ToolCallStart { content_index } => {
                events.push(StreamEvent::ToolCallStart {
                    content_index: *content_index,
                });
            }
            OpenAiResponsesProcessedEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ToolCallDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            OpenAiResponsesProcessedEvent::ToolCallEnd {
                content_index,
                tool_call,
            } => {
                events.push(StreamEvent::ToolCallEnd {
                    content_index: *content_index,
                    tool_call: StreamToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        thought_signature: tool_call.thought_signature.clone(),
                    },
                });
            }
            OpenAiResponsesProcessedEvent::Completed { result } => {
                usage = result.usage.clone();
            }
            OpenAiResponsesProcessedEvent::Error { message } => {
                events.push(StreamEvent::Error {
                    message: message.clone(),
                });
                return Ok(events);
            }
        }
    }

    if usage.total_tokens > 0 || usage.input > 0 || usage.output > 0 {
        events.push(StreamEvent::Usage {
            usage: usage.clone(),
        });
    }

    let content = openai_responses_assistant_text(&result.assistant);
    let has_tool_calls = result
        .assistant
        .content
        .iter()
        .any(|block| matches!(block, AssistantContentBlock::ToolCall(_)));
    if content.is_empty() && !has_tool_calls {
        return Err("Responses API 输出文本缺失".to_string());
    }
    events.push(StreamEvent::Finished {
        message: Message {
            role: MessageRole::Assistant,
            content,
        },
    });
    Ok(events)
}

fn openai_responses_assistant_text(assistant: &RichAssistantMessage) -> String {
    assistant
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn map_openai_responses_stop_reason(
    status: Option<&OpenAiResponsesStatus>,
) -> AssistantStopReason {
    match status {
        None
        | Some(OpenAiResponsesStatus::Completed)
        | Some(OpenAiResponsesStatus::InProgress)
        | Some(OpenAiResponsesStatus::Queued) => AssistantStopReason::Stop,
        Some(OpenAiResponsesStatus::Incomplete) => AssistantStopReason::Length,
        Some(OpenAiResponsesStatus::Failed) | Some(OpenAiResponsesStatus::Cancelled) => {
            AssistantStopReason::Error
        }
    }
}

pub fn process_openai_responses_stream_events<F>(
    events: &[OpenAiResponsesStreamEvent],
    mut assistant: RichAssistantMessage,
    options: &OpenAiResponsesStreamOptions,
    apply_service_tier_pricing: Option<F>,
) -> Result<OpenAiResponsesStreamProcessResult, String>
where
    F: FnMut(&mut Usage, Option<&str>),
{
    let mut processed_events = Vec::new();
    let mut current_item: Option<OpenAiResponsesOutputItem> = None;
    let mut current_block_index: Option<usize> = None;
    let mut current_tool_partial_json = String::new();
    let mut apply_service_tier_pricing = apply_service_tier_pricing;

    for event in events {
        match event {
            OpenAiResponsesStreamEvent::ResponseCreated { response_id } => {
                assistant.response_id = Some(response_id.clone());
                processed_events.push(OpenAiResponsesProcessedEvent::ResponseCreated {
                    response_id: response_id.clone(),
                });
            }
            OpenAiResponsesStreamEvent::OutputItemAdded { item } => {
                current_item = Some(item.clone());
                match item {
                    OpenAiResponsesOutputItem::Reasoning { .. } => {
                        assistant
                            .content
                            .push(AssistantContentBlock::Thinking(ThinkingContent {
                                thinking: String::new(),
                                thinking_signature: None,
                                redacted: false,
                            }));
                        let index = assistant.content.len() - 1;
                        current_block_index = Some(index);
                        processed_events.push(OpenAiResponsesProcessedEvent::ThinkingStart {
                            content_index: index,
                        });
                    }
                    OpenAiResponsesOutputItem::Message { .. } => {
                        assistant
                            .content
                            .push(AssistantContentBlock::Text(TextContent {
                                text: String::new(),
                                text_signature: None,
                            }));
                        let index = assistant.content.len() - 1;
                        current_block_index = Some(index);
                        processed_events.push(OpenAiResponsesProcessedEvent::TextStart {
                            content_index: index,
                        });
                    }
                    OpenAiResponsesOutputItem::FunctionCall {
                        id,
                        call_id,
                        name,
                        arguments,
                    } => {
                        current_tool_partial_json = arguments.clone();
                        assistant
                            .content
                            .push(AssistantContentBlock::ToolCall(ToolCall {
                                id: format!("{call_id}|{id}"),
                                name: name.clone(),
                                arguments: parse_tool_arguments(arguments),
                                thought_signature: None,
                            }));
                        let index = assistant.content.len() - 1;
                        current_block_index = Some(index);
                        processed_events.push(OpenAiResponsesProcessedEvent::ToolCallStart {
                            content_index: index,
                        });
                    }
                }
            }
            OpenAiResponsesStreamEvent::ReasoningSummaryPartAdded { part } => {
                if let Some(OpenAiResponsesOutputItem::Reasoning { summary, .. }) =
                    current_item.as_mut()
                {
                    summary.push(part.clone());
                }
            }
            OpenAiResponsesStreamEvent::ReasoningSummaryTextDelta { delta }
            | OpenAiResponsesStreamEvent::ReasoningTextDelta { delta } => {
                if let Some(index) = current_block_index {
                    if let Some(AssistantContentBlock::Thinking(thinking)) =
                        assistant.content.get_mut(index)
                    {
                        thinking.thinking.push_str(delta);
                        if let Some(OpenAiResponsesOutputItem::Reasoning { summary, .. }) =
                            current_item.as_mut()
                        {
                            if let Some(last_part) = summary.last_mut() {
                                last_part.text.push_str(delta);
                            }
                        }
                        processed_events.push(OpenAiResponsesProcessedEvent::ThinkingDelta {
                            content_index: index,
                            delta: delta.clone(),
                        });
                    }
                }
            }
            OpenAiResponsesStreamEvent::ReasoningSummaryPartDone => {
                if let Some(index) = current_block_index {
                    if let Some(AssistantContentBlock::Thinking(thinking)) =
                        assistant.content.get_mut(index)
                    {
                        thinking.thinking.push_str("\n\n");
                        if let Some(OpenAiResponsesOutputItem::Reasoning { summary, .. }) =
                            current_item.as_mut()
                        {
                            if let Some(last_part) = summary.last_mut() {
                                last_part.text.push_str("\n\n");
                            }
                        }
                        processed_events.push(OpenAiResponsesProcessedEvent::ThinkingDelta {
                            content_index: index,
                            delta: "\n\n".to_string(),
                        });
                    }
                }
            }
            OpenAiResponsesStreamEvent::ContentPartAdded { part } => {
                if let Some(OpenAiResponsesOutputItem::Message { content, .. }) =
                    current_item.as_mut()
                {
                    match part {
                        OpenAiResponsesStreamContentPart::OutputText { text } => {
                            content.push(OpenAiResponsesOutputContent::OutputText {
                                text: text.clone(),
                            });
                        }
                        OpenAiResponsesStreamContentPart::Refusal { refusal } => {
                            content.push(OpenAiResponsesOutputContent::Refusal {
                                refusal: refusal.clone(),
                            });
                        }
                        OpenAiResponsesStreamContentPart::Other {} => {}
                    }
                }
            }
            OpenAiResponsesStreamEvent::OutputTextDelta { delta }
            | OpenAiResponsesStreamEvent::RefusalDelta { delta } => {
                if let Some(index) = current_block_index {
                    if let Some(AssistantContentBlock::Text(text)) =
                        assistant.content.get_mut(index)
                    {
                        text.text.push_str(delta);
                        if let Some(OpenAiResponsesOutputItem::Message { content, .. }) =
                            current_item.as_mut()
                        {
                            match content.last_mut() {
                                Some(OpenAiResponsesOutputContent::OutputText { text }) => {
                                    text.push_str(delta)
                                }
                                Some(OpenAiResponsesOutputContent::Refusal { refusal }) => {
                                    refusal.push_str(delta)
                                }
                                None => {}
                            }
                        }
                        processed_events.push(OpenAiResponsesProcessedEvent::TextDelta {
                            content_index: index,
                            delta: delta.clone(),
                        });
                    }
                }
            }
            OpenAiResponsesStreamEvent::FunctionCallArgumentsDelta { delta } => {
                if let Some(index) = current_block_index {
                    if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                        assistant.content.get_mut(index)
                    {
                        current_tool_partial_json.push_str(delta);
                        tool_call.arguments = parse_tool_arguments(&current_tool_partial_json);
                        processed_events.push(OpenAiResponsesProcessedEvent::ToolCallDelta {
                            content_index: index,
                            delta: delta.clone(),
                        });
                    }
                }
            }
            OpenAiResponsesStreamEvent::FunctionCallArgumentsDone { arguments } => {
                if let Some(index) = current_block_index {
                    if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                        assistant.content.get_mut(index)
                    {
                        let previous = current_tool_partial_json.clone();
                        current_tool_partial_json = arguments.clone();
                        tool_call.arguments = parse_tool_arguments(&current_tool_partial_json);
                        if let Some(delta) = arguments.strip_prefix(&previous) {
                            if !delta.is_empty() {
                                processed_events.push(
                                    OpenAiResponsesProcessedEvent::ToolCallDelta {
                                        content_index: index,
                                        delta: delta.to_string(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
            OpenAiResponsesStreamEvent::OutputItemDone { item } => match item {
                OpenAiResponsesOutputItem::Reasoning {
                    summary, content, ..
                } => {
                    if let Some(index) = current_block_index {
                        if let Some(AssistantContentBlock::Thinking(thinking)) =
                            assistant.content.get_mut(index)
                        {
                            let summary_text = join_reasoning_parts(summary);
                            let content_text = join_reasoning_parts(content);
                            if !summary_text.is_empty() {
                                thinking.thinking = summary_text;
                            } else if !content_text.is_empty() {
                                thinking.thinking = content_text;
                            }
                            thinking.thinking_signature = serde_json::to_string(item)
                                .ok()
                                .filter(|value| !value.is_empty());
                            processed_events.push(OpenAiResponsesProcessedEvent::ThinkingEnd {
                                content_index: index,
                                content: thinking.thinking.clone(),
                            });
                        }
                    }
                    current_block_index = None;
                    current_item = None;
                }
                OpenAiResponsesOutputItem::Message { id, content, phase } => {
                    if let Some(index) = current_block_index {
                        if let Some(AssistantContentBlock::Text(text)) =
                            assistant.content.get_mut(index)
                        {
                            text.text = content
                                .iter()
                                .map(|part| match part {
                                    OpenAiResponsesOutputContent::OutputText { text } => {
                                        text.as_str()
                                    }
                                    OpenAiResponsesOutputContent::Refusal { refusal } => {
                                        refusal.as_str()
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            text.text_signature =
                                Some(encode_openai_responses_text_signature_v1(id, phase.clone()));
                            processed_events.push(OpenAiResponsesProcessedEvent::TextEnd {
                                content_index: index,
                                content: text.text.clone(),
                            });
                        }
                    }
                    current_block_index = None;
                    current_item = None;
                }
                OpenAiResponsesOutputItem::FunctionCall {
                    id,
                    call_id,
                    name,
                    arguments,
                } => {
                    let tool_call = if let Some(index) = current_block_index {
                        if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                            assistant.content.get_mut(index)
                        {
                            if !arguments.is_empty() {
                                tool_call.arguments = parse_tool_arguments(arguments);
                            }
                            tool_call.clone()
                        } else {
                            ToolCall {
                                id: format!("{call_id}|{id}"),
                                name: name.clone(),
                                arguments: parse_tool_arguments(arguments),
                                thought_signature: None,
                            }
                        }
                    } else {
                        ToolCall {
                            id: format!("{call_id}|{id}"),
                            name: name.clone(),
                            arguments: parse_tool_arguments(arguments),
                            thought_signature: None,
                        }
                    };
                    let index = current_block_index.unwrap_or(assistant.content.len());
                    if current_block_index.is_none() {
                        assistant
                            .content
                            .push(AssistantContentBlock::ToolCall(tool_call.clone()));
                    }
                    processed_events.push(OpenAiResponsesProcessedEvent::ToolCallEnd {
                        content_index: index,
                        tool_call,
                    });
                    current_block_index = None;
                    current_item = None;
                    current_tool_partial_json.clear();
                }
            },
            OpenAiResponsesStreamEvent::ResponseCompleted {
                response_id,
                completed,
            } => {
                if let Some(response_id) = response_id {
                    assistant.response_id = Some(response_id.clone());
                }
                let result = complete_openai_responses_result(
                    completed,
                    options,
                    apply_service_tier_pricing.as_mut(),
                );
                assistant.usage = result.usage.clone();
                assistant.stop_reason = result.stop_reason.clone();
                if assistant.stop_reason == AssistantStopReason::Stop
                    && assistant
                        .content
                        .iter()
                        .any(|block| matches!(block, AssistantContentBlock::ToolCall(_)))
                {
                    assistant.stop_reason = AssistantStopReason::ToolUse;
                }
                processed_events.push(OpenAiResponsesProcessedEvent::Completed { result });
            }
            OpenAiResponsesStreamEvent::Error { code, message } => {
                let message = code
                    .as_ref()
                    .map(|code| format!("Error Code {code}: {message}"))
                    .unwrap_or_else(|| message.clone());
                processed_events.push(OpenAiResponsesProcessedEvent::Error {
                    message: message.clone(),
                });
                return Err(message);
            }
            OpenAiResponsesStreamEvent::ResponseFailed {
                error_code,
                error_message,
                incomplete_reason,
            } => {
                let message = if error_code.is_some() || error_message.is_some() {
                    format!(
                        "{}: {}",
                        error_code.as_deref().unwrap_or("unknown"),
                        error_message.as_deref().unwrap_or("no message")
                    )
                } else if let Some(reason) = incomplete_reason {
                    format!("incomplete: {reason}")
                } else {
                    "Unknown error (no error details in response)".to_string()
                };
                processed_events.push(OpenAiResponsesProcessedEvent::Error {
                    message: message.clone(),
                });
                return Err(message);
            }
        }
    }

    Ok(OpenAiResponsesStreamProcessResult {
        assistant,
        events: processed_events,
    })
}

pub fn usage_from_openai_responses_usage(usage: Option<&OpenAiResponsesUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let cache_read = usage
        .input_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .unwrap_or_default();
    Usage {
        input: usage
            .input_tokens
            .unwrap_or_default()
            .saturating_sub(cache_read),
        output: usage.output_tokens.unwrap_or_default(),
        cache_read,
        cache_write: 0,
        total_tokens: usage.total_tokens.unwrap_or_default(),
        cost: UsageCost::default(),
    }
}

fn parse_tool_arguments(arguments: &str) -> std::collections::BTreeMap<String, Value> {
    serde_json::from_str::<std::collections::BTreeMap<String, Value>>(arguments).unwrap_or_default()
}

fn join_reasoning_parts(parts: &[OpenAiResponsesReasoningPart]) -> String {
    parts
        .iter()
        .map(|part| part.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn resolve_openai_responses_service_tier(
    response_service_tier: Option<&str>,
    request_service_tier: Option<&str>,
) -> Option<String> {
    response_service_tier
        .or(request_service_tier)
        .map(str::to_string)
}

pub fn complete_openai_responses_result<F>(
    response: &OpenAiResponsesCompleted,
    options: &OpenAiResponsesStreamOptions,
    mut apply_service_tier_pricing: Option<F>,
) -> OpenAiResponsesCompletionResult
where
    F: FnMut(&mut Usage, Option<&str>),
{
    let mut usage = usage_from_openai_responses_usage(response.usage.as_ref());
    let service_tier = resolve_openai_responses_service_tier(
        response.service_tier.as_deref(),
        options.service_tier.as_deref(),
    );
    if let Some(apply_service_tier_pricing) = apply_service_tier_pricing.as_mut() {
        apply_service_tier_pricing(&mut usage, service_tier.as_deref());
    }

    OpenAiResponsesCompletionResult {
        stop_reason: map_openai_responses_stop_reason(response.status.as_ref()),
        usage,
        service_tier,
    }
}

impl Default for ConvertResponsesMessagesOptions {
    fn default() -> Self {
        Self {
            include_system_prompt: true,
            allowed_tool_call_providers: BTreeSet::new(),
        }
    }
}

pub fn encode_openai_responses_text_signature_v1(
    id: &str,
    phase: Option<OpenAiResponsesTextPhase>,
) -> String {
    serde_json::to_string(&OpenAiResponsesTextSignatureV1 {
        v: 1,
        id: id.to_string(),
        phase,
    })
    .expect("text signature serialization should not fail")
}

pub fn parse_openai_responses_text_signature(
    signature: Option<&str>,
) -> Option<ParsedOpenAiResponsesTextSignature> {
    let signature = signature?;
    if signature.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<OpenAiResponsesTextSignatureV1>(signature) {
            if parsed.v == 1 {
                return Some(ParsedOpenAiResponsesTextSignature {
                    id: parsed.id,
                    phase: parsed.phase,
                });
            }
        }
    }
    Some(ParsedOpenAiResponsesTextSignature {
        id: signature.to_string(),
        phase: None,
    })
}

pub fn convert_openai_responses_messages(
    model: &Model,
    context: &OpenAiResponsesContext,
    options: ConvertResponsesMessagesOptions,
) -> Vec<OpenAiResponsesInputItem> {
    let mut messages = Vec::new();
    let allowed_tool_call_providers = options.allowed_tool_call_providers;
    let transformed_messages = transform_messages(
        &context.messages,
        model,
        Some(
            |id: &str, target_model: &Model, source: &RichAssistantMessage| {
                normalize_openai_responses_tool_call_id(
                    id,
                    target_model,
                    source,
                    &allowed_tool_call_providers,
                )
            },
        ),
    );

    if options.include_system_prompt {
        if let Some(system_prompt) = context
            .system_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
        {
            let role = if model
                .reasoning
                .as_ref()
                .is_some_and(|reasoning| reasoning.enabled)
            {
                "developer"
            } else {
                "system"
            };
            messages.push(OpenAiResponsesInputItem::Message {
                role: role.to_string(),
                content: vec![OpenAiResponsesContent::InputText {
                    text: sanitize_surrogates(system_prompt),
                }],
                status: None,
                id: None,
                phase: None,
            });
        }
    }

    for (index, message) in transformed_messages.into_iter().enumerate() {
        match message {
            RichMessage::User(user) => {
                if let Some(content) = convert_user_content(user.content) {
                    messages.push(OpenAiResponsesInputItem::Message {
                        role: "user".to_string(),
                        content,
                        status: None,
                        id: None,
                        phase: None,
                    });
                }
            }
            RichMessage::Assistant(assistant) => {
                let is_different_model = assistant.model != model.id
                    && assistant.provider == model.provider
                    && assistant.api == model.api;
                for block in assistant.content {
                    match block {
                        AssistantContentBlock::Thinking(thinking) => {
                            if let Some(signature) = thinking.thinking_signature {
                                let item = serde_json::from_str::<Value>(&signature)
                                    .unwrap_or_else(|_| json!({ "id": signature }));
                                messages.push(OpenAiResponsesInputItem::Reasoning { item });
                            }
                        }
                        AssistantContentBlock::Text(text) => {
                            let parsed = parse_openai_responses_text_signature(
                                text.text_signature.as_deref(),
                            );
                            let mut id = parsed
                                .as_ref()
                                .map(|signature| signature.id.clone())
                                .unwrap_or_else(|| format!("msg_{index}"));
                            if id.chars().count() > 64 {
                                id = format!("msg_{}", short_hash(&id));
                            }
                            messages.push(OpenAiResponsesInputItem::Message {
                                role: "assistant".to_string(),
                                content: vec![OpenAiResponsesContent::OutputText {
                                    text: sanitize_surrogates(&text.text),
                                    annotations: Vec::new(),
                                }],
                                status: Some("completed".to_string()),
                                id: Some(id),
                                phase: parsed.and_then(|signature| signature.phase),
                            });
                        }
                        AssistantContentBlock::ToolCall(tool_call) => {
                            let (call_id, item_id) =
                                split_openai_responses_tool_call_id(&tool_call.id);
                            let id = if is_different_model
                                && item_id.as_deref().is_some_and(|id| id.starts_with("fc_"))
                            {
                                None
                            } else {
                                item_id
                            };
                            messages.push(OpenAiResponsesInputItem::FunctionCall {
                                id,
                                call_id,
                                name: tool_call.name,
                                arguments: serde_json::to_string(&tool_call.arguments)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            });
                        }
                    }
                }
            }
            RichMessage::ToolResult(tool_result) => {
                let text_result = tool_result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        UserContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
                        UserContentBlock::Image(_) => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let has_text = !text_result.is_empty();
                let has_images = tool_result
                    .content
                    .iter()
                    .any(|block| matches!(block, UserContentBlock::Image(_)));
                let call_id = split_openai_responses_tool_call_id(&tool_result.tool_call_id).0;
                let output = if has_images && model_supports_images(model) {
                    let mut parts = Vec::new();
                    if has_text {
                        parts.push(OpenAiResponsesContent::InputText {
                            text: sanitize_surrogates(&text_result),
                        });
                    }
                    for block in tool_result.content {
                        if let UserContentBlock::Image(image) = block {
                            parts.push(OpenAiResponsesContent::InputImage {
                                detail: "auto".to_string(),
                                image_url: format!(
                                    "data:{};base64,{}",
                                    image.mime_type, image.data
                                ),
                            });
                        }
                    }
                    OpenAiResponsesFunctionOutput::Parts(parts)
                } else {
                    OpenAiResponsesFunctionOutput::Text(sanitize_surrogates(if has_text {
                        &text_result
                    } else {
                        "(see attached image)"
                    }))
                };
                messages.push(OpenAiResponsesInputItem::FunctionCallOutput { call_id, output });
            }
        }
    }

    messages
}

fn convert_user_content(content: UserMessageContent) -> Option<Vec<OpenAiResponsesContent>> {
    match content {
        UserMessageContent::Text(text) => Some(vec![OpenAiResponsesContent::InputText {
            text: sanitize_surrogates(&text),
        }]),
        UserMessageContent::Blocks(blocks) => {
            let content = blocks
                .into_iter()
                .map(|block| match block {
                    UserContentBlock::Text(text) => OpenAiResponsesContent::InputText {
                        text: sanitize_surrogates(&text.text),
                    },
                    UserContentBlock::Image(image) => OpenAiResponsesContent::InputImage {
                        detail: "auto".to_string(),
                        image_url: format!("data:{};base64,{}", image.mime_type, image.data),
                    },
                })
                .collect::<Vec<_>>();
            if content.is_empty() {
                None
            } else {
                Some(content)
            }
        }
    }
}

fn normalize_openai_responses_tool_call_id(
    id: &str,
    model: &Model,
    source: &RichAssistantMessage,
    allowed_tool_call_providers: &BTreeSet<String>,
) -> String {
    if !allowed_tool_call_providers.contains(&model.provider) {
        return normalize_openai_responses_id_part(id);
    }
    if !id.contains('|') {
        return normalize_openai_responses_id_part(id);
    }
    let (call_id, item_id) = split_openai_responses_tool_call_id(id);
    let normalized_call_id = normalize_openai_responses_id_part(&call_id);
    let is_foreign_tool_call = source.provider != model.provider || source.api != model.api;
    let mut normalized_item_id = if is_foreign_tool_call {
        item_id
            .as_deref()
            .map(build_foreign_openai_responses_item_id)
            .unwrap_or_else(|| "fc_missing".to_string())
    } else {
        normalize_openai_responses_id_part(item_id.as_deref().unwrap_or_default())
    };
    if !normalized_item_id.starts_with("fc_") {
        normalized_item_id =
            normalize_openai_responses_id_part(&format!("fc_{normalized_item_id}"));
    }
    format!("{normalized_call_id}|{normalized_item_id}")
}

fn normalize_openai_responses_id_part(part: &str) -> String {
    let sanitized = part
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let normalized = sanitized.chars().take(64).collect::<String>();
    normalized.trim_end_matches('_').to_string()
}

fn build_foreign_openai_responses_item_id(item_id: &str) -> String {
    let normalized = format!("fc_{}", short_hash(item_id));
    normalized.chars().take(64).collect()
}

fn split_openai_responses_tool_call_id(id: &str) -> (String, Option<String>) {
    let mut parts = id.splitn(2, '|');
    let call_id = parts.next().unwrap_or_default().to_string();
    let item_id = parts.next().map(str::to_string);
    (call_id, item_id)
}

fn model_supports_images(model: &Model) -> bool {
    model
        .input
        .iter()
        .any(|kind| matches!(kind, ModelInputKind::Image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::conversation::{
        ImageContent, RichAssistantMessage, TextContent, ThinkingContent, ToolCall,
        ToolResultMessage, UserMessage,
    };
    use crate::types::{ModelReasoning, Usage, UsageCost};

    #[test]
    fn encodes_and_parses_text_signature() {
        let signature = encode_openai_responses_text_signature_v1(
            "msg_1",
            Some(OpenAiResponsesTextPhase::FinalAnswer),
        );

        let parsed = parse_openai_responses_text_signature(Some(&signature)).expect("signature");

        assert_eq!(parsed.id, "msg_1");
        assert_eq!(parsed.phase, Some(OpenAiResponsesTextPhase::FinalAnswer));
        assert_eq!(
            parse_openai_responses_text_signature(Some("legacy"))
                .expect("legacy")
                .id,
            "legacy"
        );
    }

    #[test]
    fn converts_system_and_user_messages() {
        let context = OpenAiResponsesContext {
            system_prompt: Some("Be useful".to_string()),
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(TextContent {
                        text: "hello".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            })],
        };

        let result = convert_openai_responses_messages(
            &vision_model(),
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        assert_eq!(
            result[0],
            OpenAiResponsesInputItem::Message {
                role: "system".to_string(),
                content: vec![OpenAiResponsesContent::InputText {
                    text: "Be useful".to_string(),
                }],
                status: None,
                id: None,
                phase: None,
            }
        );
        assert_eq!(
            result[1],
            OpenAiResponsesInputItem::Message {
                role: "user".to_string(),
                content: vec![
                    OpenAiResponsesContent::InputText {
                        text: "hello".to_string(),
                    },
                    OpenAiResponsesContent::InputImage {
                        detail: "auto".to_string(),
                        image_url: "data:image/png;base64,abc".to_string(),
                    },
                ],
                status: None,
                id: None,
                phase: None,
            }
        );
    }

    #[test]
    fn uses_developer_role_when_model_has_reasoning() {
        let mut model = text_model();
        model.reasoning = Some(ModelReasoning { enabled: true });
        let context = OpenAiResponsesContext {
            system_prompt: Some("Be useful".to_string()),
            messages: Vec::new(),
        };

        let result = convert_openai_responses_messages(
            &model,
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        let OpenAiResponsesInputItem::Message { role, .. } = &result[0] else {
            panic!("message");
        };
        assert_eq!(role, "developer");
    }

    #[test]
    fn converts_assistant_text_with_signature() {
        let signature = encode_openai_responses_text_signature_v1(
            "message-from-api",
            Some(OpenAiResponsesTextPhase::Commentary),
        );
        let context = OpenAiResponsesContext {
            system_prompt: None,
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Text(TextContent {
                    text: "answer".to_string(),
                    text_signature: Some(signature),
                })],
                ..assistant_defaults()
            })],
        };

        let result = convert_openai_responses_messages(
            &text_model(),
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        assert_eq!(
            result[0],
            OpenAiResponsesInputItem::Message {
                role: "assistant".to_string(),
                content: vec![OpenAiResponsesContent::OutputText {
                    text: "answer".to_string(),
                    annotations: Vec::new(),
                }],
                status: Some("completed".to_string()),
                id: Some("message-from-api".to_string()),
                phase: Some(OpenAiResponsesTextPhase::Commentary),
            }
        );
    }

    #[test]
    fn hashes_long_assistant_message_ids() {
        let long_id = "x".repeat(80);
        let context = OpenAiResponsesContext {
            system_prompt: None,
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Text(TextContent {
                    text: "answer".to_string(),
                    text_signature: Some(long_id),
                })],
                ..assistant_defaults()
            })],
        };

        let result = convert_openai_responses_messages(
            &text_model(),
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        let OpenAiResponsesInputItem::Message { id, .. } = &result[0] else {
            panic!("message");
        };
        assert!(id.as_ref().expect("id").starts_with("msg_"));
        assert!(id.as_ref().expect("id").chars().count() <= 64);
    }

    #[test]
    fn converts_reasoning_signature_json() {
        let context = OpenAiResponsesContext {
            system_prompt: None,
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "summary".to_string(),
                    thinking_signature: Some(r#"{"id":"rs_1","type":"reasoning"}"#.to_string()),
                    redacted: false,
                })],
                ..assistant_defaults()
            })],
        };

        let result = convert_openai_responses_messages(
            &text_model(),
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        let OpenAiResponsesInputItem::Reasoning { item } = &result[0] else {
            panic!("reasoning");
        };
        assert_eq!(item["id"], "rs_1");
    }

    #[test]
    fn converts_tool_call_and_result_with_normalized_id() {
        let mut allowed = BTreeSet::new();
        allowed.insert("openai".to_string());
        let context = OpenAiResponsesContext {
            system_prompt: None,
            messages: vec![
                RichMessage::Assistant(RichAssistantMessage {
                    provider: "other".to_string(),
                    api: "other-api".to_string(),
                    content: vec![AssistantContentBlock::ToolCall(ToolCall {
                        id: "call id|foreign item id".to_string(),
                        name: "read_file".to_string(),
                        arguments: BTreeMap::from([("path".to_string(), json!("README.md"))]),
                        thought_signature: Some("sig".to_string()),
                    })],
                    ..assistant_defaults()
                }),
                RichMessage::ToolResult(ToolResultMessage {
                    tool_call_id: "call id|foreign item id".to_string(),
                    tool_name: "read_file".to_string(),
                    content: vec![UserContentBlock::Text(TextContent {
                        text: "ok".to_string(),
                        text_signature: None,
                    })],
                    details: None,
                    is_error: false,
                    timestamp_millis: 2,
                }),
            ],
        };

        let result = convert_openai_responses_messages(
            &text_model(),
            &context,
            ConvertResponsesMessagesOptions {
                allowed_tool_call_providers: allowed,
                ..ConvertResponsesMessagesOptions::default()
            },
        );

        let OpenAiResponsesInputItem::FunctionCall {
            id,
            call_id,
            arguments,
            ..
        } = &result[0]
        else {
            panic!("function call");
        };
        let OpenAiResponsesInputItem::FunctionCallOutput {
            call_id: result_call_id,
            ..
        } = &result[1]
        else {
            panic!("function output");
        };
        assert_eq!(call_id, "call_id");
        assert!(id.as_ref().expect("id").starts_with("fc_"));
        assert_eq!(result_call_id, "call_id");
        assert_eq!(arguments, r#"{"path":"README.md"}"#);
    }

    #[test]
    fn converts_tool_result_images_for_vision_models() {
        let context = OpenAiResponsesContext {
            system_prompt: None,
            messages: vec![RichMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call_1|fc_1".to_string(),
                tool_name: "screenshot".to_string(),
                content: vec![
                    UserContentBlock::Text(TextContent {
                        text: "see image".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ],
                details: None,
                is_error: false,
                timestamp_millis: 1,
            })],
        };

        let result = convert_openai_responses_messages(
            &vision_model(),
            &context,
            ConvertResponsesMessagesOptions::default(),
        );

        let OpenAiResponsesInputItem::FunctionCallOutput { output, .. } = &result[0] else {
            panic!("function output");
        };
        let OpenAiResponsesFunctionOutput::Parts(parts) = output else {
            panic!("parts");
        };
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn maps_response_status_to_stop_reason_like_pi() {
        assert_eq!(
            map_openai_responses_stop_reason(None),
            crate::types::AssistantStopReason::Stop
        );
        assert_eq!(
            map_openai_responses_stop_reason(Some(&OpenAiResponsesStatus::Completed)),
            crate::types::AssistantStopReason::Stop
        );
        assert_eq!(
            map_openai_responses_stop_reason(Some(&OpenAiResponsesStatus::Incomplete)),
            crate::types::AssistantStopReason::Length
        );
        assert_eq!(
            map_openai_responses_stop_reason(Some(&OpenAiResponsesStatus::Failed)),
            crate::types::AssistantStopReason::Error
        );
        assert_eq!(
            map_openai_responses_stop_reason(Some(&OpenAiResponsesStatus::Queued)),
            crate::types::AssistantStopReason::Stop
        );
    }

    #[test]
    fn converts_usage_with_cached_tokens_subtracted() {
        let usage = usage_from_openai_responses_usage(Some(&OpenAiResponsesUsage {
            input_tokens: Some(120),
            output_tokens: Some(30),
            total_tokens: Some(150),
            input_tokens_details: Some(OpenAiResponsesInputTokensDetails {
                cached_tokens: Some(20),
            }),
        }));

        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 30);
        assert_eq!(usage.cache_read, 20);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn resolves_service_tier_and_applies_pricing_hook() {
        let result = complete_openai_responses_result(
            &OpenAiResponsesCompleted {
                status: Some(OpenAiResponsesStatus::Completed),
                usage: Some(OpenAiResponsesUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    total_tokens: Some(15),
                    input_tokens_details: None,
                }),
                service_tier: Some("priority".to_string()),
            },
            &OpenAiResponsesStreamOptions {
                service_tier: Some("default".to_string()),
            },
            Some(|usage: &mut Usage, service_tier: Option<&str>| {
                if service_tier == Some("priority") {
                    usage.cost.total = 42.0;
                }
            }),
        );

        assert_eq!(result.stop_reason, crate::types::AssistantStopReason::Stop);
        assert_eq!(result.service_tier.as_deref(), Some("priority"));
        assert_eq!(result.usage.cost.total, 42.0);
    }

    #[test]
    fn falls_back_to_requested_service_tier() {
        assert_eq!(
            resolve_openai_responses_service_tier(None, Some("flex")).as_deref(),
            Some("flex")
        );
    }

    #[test]
    fn processes_text_stream_events() {
        let result = process_openai_responses_stream_events(
            &[
                OpenAiResponsesStreamEvent::ResponseCreated {
                    response_id: "resp_1".to_string(),
                },
                OpenAiResponsesStreamEvent::OutputItemAdded {
                    item: OpenAiResponsesOutputItem::Message {
                        id: "msg_1".to_string(),
                        content: Vec::new(),
                        phase: Some(OpenAiResponsesTextPhase::FinalAnswer),
                    },
                },
                OpenAiResponsesStreamEvent::ContentPartAdded {
                    part: OpenAiResponsesStreamContentPart::OutputText {
                        text: String::new(),
                    },
                },
                OpenAiResponsesStreamEvent::OutputTextDelta {
                    delta: "hel".to_string(),
                },
                OpenAiResponsesStreamEvent::OutputTextDelta {
                    delta: "lo".to_string(),
                },
                OpenAiResponsesStreamEvent::OutputItemDone {
                    item: OpenAiResponsesOutputItem::Message {
                        id: "msg_1".to_string(),
                        content: vec![OpenAiResponsesOutputContent::OutputText {
                            text: "hello".to_string(),
                        }],
                        phase: Some(OpenAiResponsesTextPhase::FinalAnswer),
                    },
                },
            ],
            assistant_defaults(),
            &OpenAiResponsesStreamOptions::default(),
            None::<fn(&mut Usage, Option<&str>)>,
        )
        .expect("stream");

        assert_eq!(result.assistant.response_id.as_deref(), Some("resp_1"));
        assert!(matches!(
            result.events[1],
            OpenAiResponsesProcessedEvent::TextStart { .. }
        ));
        let AssistantContentBlock::Text(text) = &result.assistant.content[0] else {
            panic!("text block");
        };
        assert_eq!(text.text, "hello");
        assert!(text
            .text_signature
            .as_ref()
            .expect("signature")
            .contains("msg_1"));
    }

    #[test]
    fn processes_reasoning_stream_events() {
        let result = process_openai_responses_stream_events(
            &[
                OpenAiResponsesStreamEvent::OutputItemAdded {
                    item: OpenAiResponsesOutputItem::Reasoning {
                        id: "rs_1".to_string(),
                        summary: Vec::new(),
                        content: Vec::new(),
                    },
                },
                OpenAiResponsesStreamEvent::ReasoningSummaryPartAdded {
                    part: OpenAiResponsesReasoningPart {
                        text: String::new(),
                    },
                },
                OpenAiResponsesStreamEvent::ReasoningSummaryTextDelta {
                    delta: "thinking".to_string(),
                },
                OpenAiResponsesStreamEvent::OutputItemDone {
                    item: OpenAiResponsesOutputItem::Reasoning {
                        id: "rs_1".to_string(),
                        summary: vec![OpenAiResponsesReasoningPart {
                            text: "thinking".to_string(),
                        }],
                        content: Vec::new(),
                    },
                },
            ],
            assistant_defaults(),
            &OpenAiResponsesStreamOptions::default(),
            None::<fn(&mut Usage, Option<&str>)>,
        )
        .expect("stream");

        let AssistantContentBlock::Thinking(thinking) = &result.assistant.content[0] else {
            panic!("thinking block");
        };
        assert_eq!(thinking.thinking, "thinking");
        assert!(thinking
            .thinking_signature
            .as_ref()
            .expect("signature")
            .contains("rs_1"));
    }

    #[test]
    fn converts_processed_reasoning_to_public_stream_events_like_pi() {
        let processed = OpenAiResponsesStreamProcessResult {
            assistant: RichAssistantMessage {
                content: vec![
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "thinking".to_string(),
                        thinking_signature: Some(r#"{"id":"rs_1","type":"reasoning"}"#.to_string()),
                        redacted: false,
                    }),
                    AssistantContentBlock::Text(TextContent {
                        text: "answer".to_string(),
                        text_signature: Some("msg_1".to_string()),
                    }),
                ],
                usage: Usage {
                    input: 3,
                    output: 2,
                    total_tokens: 5,
                    ..Usage::default()
                },
                stop_reason: crate::AssistantStopReason::Stop,
                ..assistant_defaults()
            },
            events: vec![
                OpenAiResponsesProcessedEvent::ThinkingStart { content_index: 0 },
                OpenAiResponsesProcessedEvent::ThinkingDelta {
                    content_index: 0,
                    delta: "thinking".to_string(),
                },
                OpenAiResponsesProcessedEvent::ThinkingEnd {
                    content_index: 0,
                    content: "thinking".to_string(),
                },
                OpenAiResponsesProcessedEvent::TextStart { content_index: 1 },
                OpenAiResponsesProcessedEvent::TextDelta {
                    content_index: 1,
                    delta: "answer".to_string(),
                },
                OpenAiResponsesProcessedEvent::TextEnd {
                    content_index: 1,
                    content: "answer".to_string(),
                },
                OpenAiResponsesProcessedEvent::Completed {
                    result: OpenAiResponsesCompletionResult {
                        usage: Usage {
                            input: 3,
                            output: 2,
                            total_tokens: 5,
                            ..Usage::default()
                        },
                        stop_reason: crate::AssistantStopReason::Stop,
                        service_tier: None,
                    },
                },
            ],
        };

        let events =
            openai_responses_stream_events_from_process_result(processed).expect("public events");

        assert!(matches!(
            &events[0],
            crate::StreamEvent::ThinkingStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            crate::StreamEvent::ThinkingDelta { content_index, delta }
                if *content_index == 0 && delta == "thinking"
        ));
        assert!(matches!(
            &events[2],
            crate::StreamEvent::ThinkingEnd {
                content_index,
                content,
                thinking_signature,
                redacted,
            } if *content_index == 0
                && content == "thinking"
                && thinking_signature.as_deref() == Some(r#"{"id":"rs_1","type":"reasoning"}"#)
                && !redacted
        ));
        assert!(matches!(
            &events[3],
            crate::StreamEvent::TextDelta { text } if text == "answer"
        ));
        assert!(matches!(
            &events[4],
            crate::StreamEvent::Usage { usage } if usage.input == 3 && usage.output == 2
        ));
        assert!(matches!(
            events.last().expect("finished"),
            crate::StreamEvent::Finished { message } if message.content == "answer"
        ));
    }

    #[test]
    fn converts_processed_tool_calls_to_public_stream_events_like_pi() {
        let mut arguments = std::collections::BTreeMap::new();
        arguments.insert("path".to_string(), json!("README.md"));
        let tool_call = ToolCall {
            id: "call_1|fc_1".to_string(),
            name: "read_file".to_string(),
            arguments,
            thought_signature: None,
        };
        let processed = OpenAiResponsesStreamProcessResult {
            assistant: RichAssistantMessage {
                content: vec![AssistantContentBlock::ToolCall(tool_call.clone())],
                usage: Usage {
                    input: 3,
                    output: 2,
                    total_tokens: 5,
                    ..Usage::default()
                },
                stop_reason: crate::AssistantStopReason::ToolUse,
                ..assistant_defaults()
            },
            events: vec![
                OpenAiResponsesProcessedEvent::ToolCallStart { content_index: 0 },
                OpenAiResponsesProcessedEvent::ToolCallDelta {
                    content_index: 0,
                    delta: "{\"path\":\"README.md\"}".to_string(),
                },
                OpenAiResponsesProcessedEvent::ToolCallEnd {
                    content_index: 0,
                    tool_call,
                },
                OpenAiResponsesProcessedEvent::Completed {
                    result: OpenAiResponsesCompletionResult {
                        usage: Usage {
                            input: 3,
                            output: 2,
                            total_tokens: 5,
                            ..Usage::default()
                        },
                        stop_reason: crate::AssistantStopReason::ToolUse,
                        service_tier: None,
                    },
                },
            ],
        };

        let events =
            openai_responses_stream_events_from_process_result(processed).expect("public events");

        assert!(matches!(
            &events[0],
            crate::StreamEvent::ToolCallStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            crate::StreamEvent::ToolCallDelta { content_index, delta }
                if *content_index == 0 && delta == "{\"path\":\"README.md\"}"
        ));
        assert!(matches!(
            &events[2],
            crate::StreamEvent::ToolCallEnd {
                content_index,
                tool_call
            } if *content_index == 0
                && tool_call.name == "read_file"
                && tool_call.arguments["path"] == json!("README.md")
        ));
        assert!(matches!(
            &events[3],
            crate::StreamEvent::Usage { usage } if usage.input == 3 && usage.output == 2
        ));
        assert!(matches!(
            events.last().expect("finished"),
            crate::StreamEvent::Finished { message } if message.content.is_empty()
        ));
    }

    #[test]
    fn processes_tool_call_stream_events_and_marks_tool_use_on_completed() {
        let result = process_openai_responses_stream_events(
            &[
                OpenAiResponsesStreamEvent::OutputItemAdded {
                    item: OpenAiResponsesOutputItem::FunctionCall {
                        id: "fc_1".to_string(),
                        call_id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        arguments: String::new(),
                    },
                },
                OpenAiResponsesStreamEvent::FunctionCallArgumentsDelta {
                    delta: r#"{"path":"#.to_string(),
                },
                OpenAiResponsesStreamEvent::FunctionCallArgumentsDone {
                    arguments: r#"{"path":"README.md"}"#.to_string(),
                },
                OpenAiResponsesStreamEvent::OutputItemDone {
                    item: OpenAiResponsesOutputItem::FunctionCall {
                        id: "fc_1".to_string(),
                        call_id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        arguments: r#"{"path":"README.md"}"#.to_string(),
                    },
                },
                OpenAiResponsesStreamEvent::ResponseCompleted {
                    response_id: Some("resp_done".to_string()),
                    completed: OpenAiResponsesCompleted {
                        status: Some(OpenAiResponsesStatus::Completed),
                        usage: Some(OpenAiResponsesUsage {
                            input_tokens: Some(10),
                            output_tokens: Some(5),
                            total_tokens: Some(15),
                            input_tokens_details: None,
                        }),
                        service_tier: None,
                    },
                },
            ],
            assistant_defaults(),
            &OpenAiResponsesStreamOptions::default(),
            None::<fn(&mut Usage, Option<&str>)>,
        )
        .expect("stream");

        assert_eq!(
            result.assistant.stop_reason,
            crate::types::AssistantStopReason::ToolUse
        );
        assert_eq!(result.assistant.response_id.as_deref(), Some("resp_done"));
        let AssistantContentBlock::ToolCall(tool_call) = &result.assistant.content[0] else {
            panic!("tool call");
        };
        assert_eq!(tool_call.id, "call_1|fc_1");
        assert_eq!(tool_call.arguments["path"], "README.md");
    }

    #[test]
    fn returns_error_for_failed_stream_events() {
        let error = process_openai_responses_stream_events(
            &[OpenAiResponsesStreamEvent::ResponseFailed {
                error_code: Some("bad_request".to_string()),
                error_message: Some("invalid".to_string()),
                incomplete_reason: None,
            }],
            assistant_defaults(),
            &OpenAiResponsesStreamOptions::default(),
            None::<fn(&mut Usage, Option<&str>)>,
        )
        .expect_err("failed event");

        assert_eq!(error, "bad_request: invalid");
    }

    fn assistant_defaults() -> RichAssistantMessage {
        RichAssistantMessage {
            content: Vec::new(),
            api: "openai-responses".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            response_model: None,
            response_id: None,
            usage: Usage {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total_tokens: 0,
                cost: UsageCost::default(),
            },
            stop_reason: crate::types::AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 1,
        }
    }

    fn text_model() -> Model {
        Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 1,
            input: vec![ModelInputKind::Text],
            ..Model::default()
        }
    }

    fn vision_model() -> Model {
        Model {
            input: vec![ModelInputKind::Text, ModelInputKind::Image],
            ..text_model()
        }
    }
}
