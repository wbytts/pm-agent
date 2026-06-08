use ai::{
    validate_tool_arguments, AssistantContentBlock, AssistantStopReason, LanguageModelProvider,
    Message as AiMessage, MessageRole, Model, ModelThinkingLevel, RichAssistantMessage,
    RichMessage, StreamEvent, StreamRequest, TextContent, ThinkingContent, ToolCall,
    ToolDefinition, ToolResultMessage, UserContentBlock, UserMessage, UserMessageContent,
};
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc;

use crate::error::{AgentError, AgentResult};
use crate::state::AgentMessage;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolResult {
    pub content: Vec<UserContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
    pub terminate: bool,
}

impl AgentToolResult {
    pub fn text(
        content: impl Into<String>,
        details: Option<Value>,
        is_error: bool,
        terminate: bool,
    ) -> Self {
        Self {
            content: text_tool_result_content(content),
            details,
            is_error,
            terminate,
        }
    }
}

pub fn text_tool_result_content(content: impl Into<String>) -> Vec<UserContentBlock> {
    vec![UserContentBlock::Text(TextContent {
        text: content.into(),
        text_signature: None,
    })]
}

fn tool_result_content_text(content: &[UserContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            UserContentBlock::Text(text) => Some(text.text.as_str()),
            UserContentBlock::Image(_) => None,
        })
        .collect::<String>()
}

pub type AgentToolUpdateCallback<'a> = &'a mut dyn FnMut(AgentToolResult);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

pub trait AgentTool: std::fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    fn parameters_schema(&self) -> Option<Value> {
        None
    }
    fn prepare_arguments(&self, arguments: &Value) -> Value {
        arguments.clone()
    }
    fn execute(&self, call: &AgentToolCall) -> AgentToolResult;
    fn execution_mode(&self) -> Option<ToolExecutionMode> {
        None
    }
    fn execute_with_update(
        &self,
        call: &AgentToolCall,
        _on_update: AgentToolUpdateCallback<'_>,
    ) -> AgentToolResult {
        self.execute(call)
    }
}

#[derive(Debug, Clone)]
pub struct BeforeToolCallContext<'a> {
    pub assistant_message: &'a AgentMessage,
    pub tool_call: &'a AgentToolCall,
    pub args: &'a Value,
    pub messages: &'a [AgentMessage],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeforeToolCallResult {
    pub block: bool,
    pub reason: Option<String>,
}

pub type BeforeToolCallHook =
    Box<dyn for<'a> Fn(BeforeToolCallContext<'a>) -> BeforeToolCallResult>;

#[derive(Debug, Clone)]
pub struct AfterToolCallContext<'a> {
    pub assistant_message: &'a AgentMessage,
    pub tool_call: &'a AgentToolCall,
    pub args: &'a Value,
    pub result: &'a AgentToolResult,
    pub messages: &'a [AgentMessage],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AfterToolCallResult {
    pub content: Option<Vec<UserContentBlock>>,
    pub details: Option<Value>,
    pub is_error: Option<bool>,
    pub terminate: Option<bool>,
}

pub type AfterToolCallHook = Box<dyn for<'a> Fn(AfterToolCallContext<'a>) -> AfterToolCallResult>;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionUpdate {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub partial_result: AgentToolResult,
}

#[derive(Debug, Clone)]
pub struct AfterTurnContext<'a> {
    pub message: &'a AgentMessage,
    pub tool_results: &'a [AgentMessage],
    pub messages: &'a [AgentMessage],
    pub new_messages: &'a [AgentMessage],
}

#[derive(Debug, Clone)]
pub struct PrepareNextTurnContext<'a> {
    pub message: &'a AgentMessage,
    pub tool_results: &'a [AgentMessage],
    pub messages: &'a [AgentMessage],
    pub new_messages: &'a [AgentMessage],
}

#[derive(Debug, Clone)]
pub struct PrepareNextTurnResult {
    pub messages: Option<Vec<AgentMessage>>,
    pub model: Option<Model>,
    pub thinking_level: Option<ModelThinkingLevel>,
}

pub type ShouldStopAfterTurnHook = Box<dyn for<'a> Fn(AfterTurnContext<'a>) -> bool>;
pub type GetFollowUpMessagesHook = Box<dyn Fn() -> Vec<AgentMessage>>;
pub type GetSteeringMessagesHook = Box<dyn Fn() -> Vec<AgentMessage>>;
pub type TransformContextHook = Box<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage>>;
pub type ConvertToLlmHook = Box<dyn Fn(&[AgentMessage]) -> Vec<AiMessage>>;
pub type ConvertToRichLlmHook = Box<dyn Fn(&[AgentMessage]) -> Vec<RichMessage>>;
pub type PrepareNextTurnHook =
    Box<dyn for<'a> Fn(PrepareNextTurnContext<'a>) -> PrepareNextTurnResult>;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentLoopEvent {
    AgentStart,
    AgentEnd {
        messages: Vec<AgentMessage>,
    },
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<AgentMessage>,
    },
    MessageStart {
        message: AgentMessage,
    },
    MessageEnd {
        message: AgentMessage,
    },
    MessageUpdate {
        assistant_message_event: StreamEvent,
        message: AgentMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        partial_result: AgentToolResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
    },
}

impl AgentLoopEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AgentStart => "agent_start",
            Self::AgentEnd { .. } => "agent_end",
            Self::TurnStart => "turn_start",
            Self::TurnEnd { .. } => "turn_end",
            Self::MessageStart { .. } => "message_start",
            Self::MessageEnd { .. } => "message_end",
            Self::MessageUpdate { .. } => "message_update",
            Self::ToolExecutionStart { .. } => "tool_execution_start",
            Self::ToolExecutionUpdate { .. } => "tool_execution_update",
            Self::ToolExecutionEnd { .. } => "tool_execution_end",
        }
    }
}

#[derive(Default)]
pub struct AgentLoopConfig {
    pub tools: Vec<Box<dyn AgentTool>>,
    pub tool_execution: ToolExecutionMode,
    pub reasoning: Option<ModelThinkingLevel>,
    pub before_tool_call: Option<BeforeToolCallHook>,
    pub after_tool_call: Option<AfterToolCallHook>,
    pub should_stop_after_turn: Option<ShouldStopAfterTurnHook>,
    pub get_follow_up_messages: Option<GetFollowUpMessagesHook>,
    pub get_steering_messages: Option<GetSteeringMessagesHook>,
    pub transform_context: Option<TransformContextHook>,
    pub convert_to_llm: Option<ConvertToLlmHook>,
    pub convert_to_rich_llm: Option<ConvertToRichLlmHook>,
    pub prepare_next_turn: Option<PrepareNextTurnHook>,
}

impl std::fmt::Debug for AgentLoopConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentLoopConfig")
            .field("tools", &self.tools)
            .field("tool_execution", &self.tool_execution)
            .field("reasoning", &self.reasoning)
            .field("before_tool_call", &self.before_tool_call.is_some())
            .field("after_tool_call", &self.after_tool_call.is_some())
            .field(
                "should_stop_after_turn",
                &self.should_stop_after_turn.is_some(),
            )
            .field(
                "get_follow_up_messages",
                &self.get_follow_up_messages.is_some(),
            )
            .field(
                "get_steering_messages",
                &self.get_steering_messages.is_some(),
            )
            .field("transform_context", &self.transform_context.is_some())
            .field("convert_to_llm", &self.convert_to_llm.is_some())
            .field("convert_to_rich_llm", &self.convert_to_rich_llm.is_some())
            .field("prepare_next_turn", &self.prepare_next_turn.is_some())
            .finish()
    }
}

#[derive(Debug)]
struct ExecutedToolCallBatch {
    results: Vec<ExecutedToolCallResult>,
    terminate: bool,
}

impl ExecutedToolCallBatch {
    fn new(mut results: Vec<ExecutedToolCallResult>) -> Self {
        results.sort_by_key(|entry| entry.source_index);
        let terminate = !results.is_empty() && results.iter().all(|entry| entry.result.terminate);
        Self { results, terminate }
    }
}

#[derive(Debug)]
struct ExecutedToolCallResult {
    source_index: usize,
    tool_call: AgentToolCall,
    result: AgentToolResult,
}

#[derive(Debug)]
struct PreparedToolCall {
    source_index: usize,
    tool_index: usize,
    tool_call: AgentToolCall,
}

#[derive(Debug)]
struct RawExecutedToolCallResult {
    source_index: usize,
    tool_call: AgentToolCall,
    result: AgentToolResult,
    updates: Vec<ToolExecutionUpdate>,
}

#[derive(Debug)]
enum PreparedToolCallOutcome {
    Immediate(ExecutedToolCallResult),
    Prepared(PreparedToolCall),
}

pub struct AgentLoop<P: LanguageModelProvider> {
    model: Model,
    system_prompt: String,
    provider: P,
    config: AgentLoopConfig,
    reasoning: Option<ModelThinkingLevel>,
    messages: Vec<AgentMessage>,
    tool_updates: Vec<ToolExecutionUpdate>,
    events: Vec<AgentLoopEvent>,
}

impl<P: LanguageModelProvider> AgentLoop<P> {
    pub fn new(
        model: Model,
        system_prompt: impl Into<String>,
        provider: P,
        config: AgentLoopConfig,
    ) -> Self {
        Self {
            model,
            system_prompt: system_prompt.into(),
            provider,
            reasoning: config.reasoning,
            config,
            messages: Vec::new(),
            tool_updates: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn tool_updates(&self) -> &[ToolExecutionUpdate] {
        &self.tool_updates
    }

    pub fn events(&self) -> &[AgentLoopEvent] {
        &self.events
    }

    pub fn run(&mut self, prompts: Vec<AgentMessage>) -> AgentResult<Vec<AgentMessage>> {
        let mut new_messages = prompts;
        self.messages.extend(new_messages.clone());
        self.events.clear();
        self.events.push(AgentLoopEvent::AgentStart);
        self.events.push(AgentLoopEvent::TurnStart);
        for message in &new_messages {
            self.events.push(AgentLoopEvent::MessageStart {
                message: message.clone(),
            });
            self.events.push(AgentLoopEvent::MessageEnd {
                message: message.clone(),
            });
        }
        self.enqueue_steering_messages(&mut new_messages);
        self.run_loop(new_messages)
    }

    pub fn continue_from_context(
        &mut self,
        context_messages: Vec<AgentMessage>,
    ) -> AgentResult<Vec<AgentMessage>> {
        if context_messages.is_empty() {
            return Err(AgentError::EmptyContinueContext);
        }
        if context_messages
            .last()
            .map(|message| message.role == MessageRole::Assistant)
            .unwrap_or(false)
        {
            return Err(AgentError::ContinueFromAssistant);
        }
        self.messages = context_messages;
        self.events.clear();
        self.events.push(AgentLoopEvent::AgentStart);
        self.events.push(AgentLoopEvent::TurnStart);
        self.run_loop(Vec::new())
    }

    fn run_loop(&mut self, mut new_messages: Vec<AgentMessage>) -> AgentResult<Vec<AgentMessage>> {
        let mut first_turn = true;
        loop {
            if first_turn {
                first_turn = false;
            } else {
                self.events.push(AgentLoopEvent::TurnStart);
            }
            let assistant = self.stream_assistant_response()?;
            self.messages.push(assistant.clone());
            new_messages.push(assistant.clone());
            self.events.push(AgentLoopEvent::MessageStart {
                message: assistant.clone(),
            });
            self.events.push(AgentLoopEvent::MessageEnd {
                message: assistant.clone(),
            });
            let mut tool_results = Vec::new();

            if matches!(
                assistant.stop_reason,
                Some(ai::AssistantStopReason::Error | ai::AssistantStopReason::Aborted)
            ) {
                self.events.push(AgentLoopEvent::TurnEnd {
                    message: assistant,
                    tool_results,
                });
                break;
            }

            let tool_calls = tool_calls_from_message(&assistant);
            if tool_calls.is_empty() {
                self.events.push(AgentLoopEvent::TurnEnd {
                    message: assistant.clone(),
                    tool_results: tool_results.clone(),
                });
                self.prepare_next_turn(&assistant, &tool_results, &new_messages);
                if self.should_stop_after_turn(&assistant, &tool_results, &new_messages) {
                    break;
                }
                if self.enqueue_steering_messages(&mut new_messages) {
                    continue;
                }
                if self.enqueue_follow_up_messages(&mut new_messages) {
                    continue;
                }
                break;
            }

            let executed_tool_batch = self.execute_tool_calls(&assistant, tool_calls);
            let terminate = executed_tool_batch.terminate;
            for executed in executed_tool_batch.results {
                let result = executed.result;
                let content = tool_result_content_text(&result.content);
                let tool_result = AgentMessage {
                    role: MessageRole::Tool,
                    content,
                    content_blocks: Vec::new(),
                    user_content_blocks: result.content,
                    tool_call_id: Some(executed.tool_call.id),
                    tool_name: Some(executed.tool_call.name),
                    details: result.details,
                    is_error: result.is_error,
                    usage: None,
                    stop_reason: None,
                };
                self.messages.push(tool_result.clone());
                new_messages.push(tool_result.clone());
                tool_results.push(tool_result);
                let tool_result = tool_results.last().expect("tool result").clone();
                self.events.push(AgentLoopEvent::MessageStart {
                    message: tool_result.clone(),
                });
                self.events.push(AgentLoopEvent::MessageEnd {
                    message: tool_result,
                });
            }

            self.events.push(AgentLoopEvent::TurnEnd {
                message: assistant.clone(),
                tool_results: tool_results.clone(),
            });
            self.prepare_next_turn(&assistant, &tool_results, &new_messages);

            if self.should_stop_after_turn(&assistant, &tool_results, &new_messages) {
                break;
            }

            if self.enqueue_steering_messages(&mut new_messages) {
                continue;
            }

            if terminate {
                if self.enqueue_follow_up_messages(&mut new_messages) {
                    continue;
                }
                break;
            }
        }

        self.events.push(AgentLoopEvent::AgentEnd {
            messages: new_messages.clone(),
        });
        Ok(new_messages)
    }

    fn should_stop_after_turn(
        &self,
        message: &AgentMessage,
        tool_results: &[AgentMessage],
        new_messages: &[AgentMessage],
    ) -> bool {
        self.config
            .should_stop_after_turn
            .as_ref()
            .map(|hook| {
                hook(AfterTurnContext {
                    message,
                    tool_results,
                    messages: &self.messages,
                    new_messages,
                })
            })
            .unwrap_or(false)
    }

    fn prepare_next_turn(
        &mut self,
        message: &AgentMessage,
        tool_results: &[AgentMessage],
        new_messages: &[AgentMessage],
    ) {
        if let Some(prepare_next_turn) = &self.config.prepare_next_turn {
            let result = prepare_next_turn(PrepareNextTurnContext {
                message,
                tool_results,
                messages: &self.messages,
                new_messages,
            });
            if let Some(messages) = result.messages {
                self.messages = messages;
            }
            if let Some(model) = result.model {
                self.model = model;
            }
            if let Some(thinking_level) = result.thinking_level {
                self.reasoning = match thinking_level {
                    ModelThinkingLevel::Off => None,
                    level => Some(level),
                };
            }
        }
    }

    fn enqueue_follow_up_messages(&mut self, new_messages: &mut Vec<AgentMessage>) -> bool {
        let Some(get_follow_up_messages) = &self.config.get_follow_up_messages else {
            return false;
        };
        let follow_up_messages = get_follow_up_messages();
        if follow_up_messages.is_empty() {
            return false;
        }
        self.enqueue_pending_messages(follow_up_messages, new_messages);
        true
    }

    fn enqueue_steering_messages(&mut self, new_messages: &mut Vec<AgentMessage>) -> bool {
        let Some(get_steering_messages) = &self.config.get_steering_messages else {
            return false;
        };
        let steering_messages = get_steering_messages();
        if steering_messages.is_empty() {
            return false;
        }
        self.enqueue_pending_messages(steering_messages, new_messages);
        true
    }

    fn enqueue_pending_messages(
        &mut self,
        messages: Vec<AgentMessage>,
        new_messages: &mut Vec<AgentMessage>,
    ) {
        for message in messages {
            self.events.push(AgentLoopEvent::MessageStart {
                message: message.clone(),
            });
            self.events.push(AgentLoopEvent::MessageEnd {
                message: message.clone(),
            });
            self.messages.push(message.clone());
            new_messages.push(message);
        }
    }

    fn stream_assistant_response(&mut self) -> AgentResult<AgentMessage> {
        let mut request_messages = Vec::new();
        if !self.system_prompt.trim().is_empty() {
            request_messages.push(AiMessage {
                role: MessageRole::System,
                content: self.system_prompt.clone(),
            });
        }
        let context_messages = self
            .config
            .transform_context
            .as_ref()
            .map(|hook| hook(&self.messages))
            .unwrap_or_else(|| self.messages.clone());
        let rich_messages = if let Some(convert_to_rich_llm) = &self.config.convert_to_rich_llm {
            convert_to_rich_llm(&context_messages)
        } else {
            request_messages.extend(self.convert_messages_to_llm(&context_messages));
            if self.config.convert_to_llm.is_none() && agent_messages_need_rich(&context_messages) {
                agent_messages_to_rich_messages(&context_messages, &self.model)
            } else {
                Vec::new()
            }
        };

        let mut metadata = BTreeMap::new();
        if let Some(reasoning) = self.reasoning {
            metadata.insert(
                "reasoning".to_string(),
                json!(model_thinking_level_as_str(reasoning)),
            );
        }

        let response = self
            .provider
            .stream(StreamRequest {
                model: self.model.clone(),
                messages: request_messages,
                rich_messages,
                tools: self.tool_definitions(),
                metadata,
            })
            .map_err(|error| AgentError::Ai(error.to_string()))?;

        let mut content_blocks = Vec::new();
        let mut tool_call_argument_deltas = BTreeMap::<usize, String>::new();
        let mut usage = None;
        let mut saw_tool_call = false;
        for event in response {
            match event {
                StreamEvent::TextDelta { text } => {
                    let assistant_message_event = StreamEvent::TextDelta { text: text.clone() };
                    append_text_block(&mut content_blocks, 0, &text);
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ThinkingStart { content_index } => {
                    let assistant_message_event = StreamEvent::ThinkingStart { content_index };
                    set_content_block(
                        &mut content_blocks,
                        content_index,
                        AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: String::new(),
                            thinking_signature: None,
                            redacted: false,
                        }),
                    );
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ThinkingDelta {
                    content_index,
                    delta,
                } => {
                    let assistant_message_event = StreamEvent::ThinkingDelta {
                        content_index,
                        delta: delta.clone(),
                    };
                    append_thinking_block(&mut content_blocks, content_index, &delta);
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ThinkingEnd {
                    content_index,
                    content,
                    thinking_signature,
                    redacted,
                } => {
                    let assistant_message_event = StreamEvent::ThinkingEnd {
                        content_index,
                        content: content.clone(),
                        thinking_signature: thinking_signature.clone(),
                        redacted,
                    };
                    set_content_block(
                        &mut content_blocks,
                        content_index,
                        AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: content,
                            thinking_signature,
                            redacted,
                        }),
                    );
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ToolCallStart { content_index } => {
                    let assistant_message_event = StreamEvent::ToolCallStart { content_index };
                    saw_tool_call = true;
                    tool_call_argument_deltas.insert(content_index, String::new());
                    set_content_block(
                        &mut content_blocks,
                        content_index,
                        AssistantContentBlock::ToolCall(ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: Default::default(),
                            thought_signature: None,
                        }),
                    );
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ToolCallDelta {
                    content_index,
                    delta,
                } => {
                    let assistant_message_event = StreamEvent::ToolCallDelta {
                        content_index,
                        delta: delta.clone(),
                    };
                    append_tool_call_arguments_delta(
                        &mut content_blocks,
                        &mut tool_call_argument_deltas,
                        content_index,
                        &delta,
                    );
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::ToolCallEnd {
                    content_index,
                    tool_call,
                } => {
                    saw_tool_call = true;
                    let assistant_message_event = StreamEvent::ToolCallEnd {
                        content_index,
                        tool_call: tool_call.clone(),
                    };
                    tool_call_argument_deltas.remove(&content_index);
                    set_content_block(
                        &mut content_blocks,
                        content_index,
                        AssistantContentBlock::ToolCall(ToolCall {
                            id: tool_call.id,
                            name: tool_call.name,
                            arguments: tool_call.arguments,
                            thought_signature: tool_call.thought_signature,
                        }),
                    );
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::Usage { usage: next_usage } => {
                    let assistant_message_event = StreamEvent::Usage {
                        usage: next_usage.clone(),
                    };
                    usage = Some(next_usage);
                    self.events.push(AgentLoopEvent::MessageUpdate {
                        assistant_message_event,
                        message: partial_assistant_message(&content_blocks, usage.clone()),
                    });
                }
                StreamEvent::Finished { message } => {
                    let stop_reason = if saw_tool_call {
                        ai::AssistantStopReason::ToolUse
                    } else {
                        ai::AssistantStopReason::Stop
                    };
                    if content_blocks.is_empty() && !message.content.is_empty() {
                        content_blocks.push(AssistantContentBlock::Text(TextContent {
                            text: message.content.clone(),
                            text_signature: None,
                        }));
                    }
                    return Ok(AgentMessage {
                        role: message.role,
                        content: message.content,
                        content_blocks,
                        user_content_blocks: Vec::new(),
                        tool_call_id: None,
                        tool_name: None,
                        details: None,
                        is_error: false,
                        usage,
                        stop_reason: Some(stop_reason),
                    });
                }
                StreamEvent::RichFinished { message } => {
                    let stop_reason = if saw_tool_call {
                        ai::AssistantStopReason::ToolUse
                    } else {
                        message.stop_reason.clone()
                    };
                    if content_blocks.is_empty() {
                        content_blocks = message.content.clone();
                    }
                    return Ok(AgentMessage {
                        role: MessageRole::Assistant,
                        content: rich_assistant_text(&message),
                        content_blocks,
                        user_content_blocks: Vec::new(),
                        tool_call_id: None,
                        tool_name: None,
                        details: None,
                        is_error: false,
                        usage: usage.or_else(|| {
                            (message.usage != Default::default()).then_some(message.usage)
                        }),
                        stop_reason: Some(stop_reason),
                    });
                }
                StreamEvent::Error { message } => {
                    return Ok(AgentMessage {
                        role: MessageRole::Assistant,
                        content: String::new(),
                        content_blocks: Vec::new(),
                        user_content_blocks: Vec::new(),
                        tool_call_id: None,
                        tool_name: None,
                        details: Some(json!({ "errorMessage": message })),
                        is_error: true,
                        usage,
                        stop_reason: Some(ai::AssistantStopReason::Error),
                    });
                }
            }
        }

        Err(AgentError::Ai(
            "Provider stream ended without a final message".to_string(),
        ))
    }

    fn execute_tool_calls(
        &mut self,
        assistant_message: &AgentMessage,
        tool_calls: Vec<AgentToolCall>,
    ) -> ExecutedToolCallBatch {
        if self.should_execute_tool_calls_sequential(&tool_calls) {
            return self.execute_tool_calls_sequential(assistant_message, tool_calls);
        }
        self.execute_tool_calls_parallel(assistant_message, tool_calls)
    }

    fn should_execute_tool_calls_sequential(&self, tool_calls: &[AgentToolCall]) -> bool {
        self.config.tool_execution == ToolExecutionMode::Sequential
            || tool_calls.iter().any(|tool_call| {
                self.config
                    .tools
                    .iter()
                    .find(|tool| tool.name() == tool_call.name)
                    .and_then(|tool| tool.execution_mode())
                    == Some(ToolExecutionMode::Sequential)
            })
    }

    fn execute_tool_calls_sequential(
        &mut self,
        assistant_message: &AgentMessage,
        tool_calls: Vec<AgentToolCall>,
    ) -> ExecutedToolCallBatch {
        let mut results = Vec::new();
        for (source_index, tool_call) in tool_calls.into_iter().enumerate() {
            match self.prepare_tool_call(assistant_message, source_index, &tool_call) {
                PreparedToolCallOutcome::Immediate(result) => results.push(result),
                PreparedToolCallOutcome::Prepared(prepared) => {
                    let raw = execute_prepared_tool_call(
                        self.config.tools[prepared.tool_index].as_ref(),
                        prepared,
                    );
                    results.push(self.finalize_executed_tool_call(assistant_message, raw));
                }
            }
        }
        ExecutedToolCallBatch::new(results)
    }

    fn execute_tool_calls_parallel(
        &mut self,
        assistant_message: &AgentMessage,
        tool_calls: Vec<AgentToolCall>,
    ) -> ExecutedToolCallBatch {
        let mut results = Vec::new();
        let mut prepared_calls = Vec::new();
        for (source_index, tool_call) in tool_calls.into_iter().enumerate() {
            match self.prepare_tool_call(assistant_message, source_index, &tool_call) {
                PreparedToolCallOutcome::Immediate(result) => results.push(result),
                PreparedToolCallOutcome::Prepared(prepared) => prepared_calls.push(prepared),
            }
        }

        let completed_calls = std::thread::scope(|scope| {
            let (sender, receiver) = mpsc::channel();
            let spawned_count = prepared_calls.len();
            for prepared in prepared_calls {
                let sender = sender.clone();
                let tool = self.config.tools[prepared.tool_index].as_ref();
                scope.spawn(move || {
                    let raw = execute_prepared_tool_call(tool, prepared);
                    sender.send(raw).expect("tool execution result channel");
                });
            }
            drop(sender);

            let mut completed_calls = Vec::with_capacity(spawned_count);
            for _ in 0..spawned_count {
                completed_calls.push(
                    receiver
                        .recv()
                        .expect("tool execution worker should send a result"),
                );
            }
            completed_calls
        });

        for raw in completed_calls {
            results.push(self.finalize_executed_tool_call(assistant_message, raw));
        }

        ExecutedToolCallBatch::new(results)
    }

    fn prepare_tool_call(
        &mut self,
        assistant_message: &AgentMessage,
        source_index: usize,
        tool_call: &AgentToolCall,
    ) -> PreparedToolCallOutcome {
        let Some(tool_index) = self
            .config
            .tools
            .iter()
            .position(|tool| tool.name() == tool_call.name)
        else {
            let result = AgentToolResult::text(
                format!("Tool {} not found", tool_call.name),
                None,
                true,
                false,
            );
            self.events.push(AgentLoopEvent::ToolExecutionEnd {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                result: result.clone(),
                is_error: result.is_error,
            });
            return PreparedToolCallOutcome::Immediate(ExecutedToolCallResult {
                source_index,
                tool_call: tool_call.clone(),
                result,
            });
        };
        let prepared_arguments =
            self.config.tools[tool_index].prepare_arguments(&tool_call.arguments);
        let prepared_tool_call = if prepared_arguments == tool_call.arguments {
            tool_call.clone()
        } else {
            AgentToolCall {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: prepared_arguments,
            }
        };
        let prepared_tool_call = match validate_agent_tool_call(
            self.config.tools[tool_index].as_ref(),
            &prepared_tool_call,
        ) {
            Ok(tool_call) => tool_call,
            Err(message) => {
                let result = AgentToolResult::text(message, None, true, false);
                self.events.push(AgentLoopEvent::ToolExecutionEnd {
                    tool_call_id: prepared_tool_call.id.clone(),
                    tool_name: prepared_tool_call.name.clone(),
                    result: result.clone(),
                    is_error: result.is_error,
                });
                return PreparedToolCallOutcome::Immediate(ExecutedToolCallResult {
                    source_index,
                    tool_call: prepared_tool_call,
                    result,
                });
            }
        };
        self.events.push(AgentLoopEvent::ToolExecutionStart {
            tool_call_id: prepared_tool_call.id.clone(),
            tool_name: prepared_tool_call.name.clone(),
            args: prepared_tool_call.arguments.clone(),
        });
        if let Some(before_tool_call) = &self.config.before_tool_call {
            let decision = before_tool_call(BeforeToolCallContext {
                assistant_message,
                tool_call: &prepared_tool_call,
                args: &prepared_tool_call.arguments,
                messages: &self.messages,
            });
            if decision.block {
                let result = AgentToolResult::text(
                    decision
                        .reason
                        .unwrap_or_else(|| "Tool execution was blocked".to_string()),
                    None,
                    true,
                    false,
                );
                self.events.push(AgentLoopEvent::ToolExecutionEnd {
                    tool_call_id: prepared_tool_call.id.clone(),
                    tool_name: prepared_tool_call.name.clone(),
                    result: result.clone(),
                    is_error: result.is_error,
                });
                return PreparedToolCallOutcome::Immediate(ExecutedToolCallResult {
                    source_index,
                    tool_call: prepared_tool_call,
                    result,
                });
            }
        }
        PreparedToolCallOutcome::Prepared(PreparedToolCall {
            source_index,
            tool_index,
            tool_call: prepared_tool_call,
        })
    }

    fn finalize_executed_tool_call(
        &mut self,
        assistant_message: &AgentMessage,
        raw: RawExecutedToolCallResult,
    ) -> ExecutedToolCallResult {
        for update in raw.updates {
            self.events.push(AgentLoopEvent::ToolExecutionUpdate {
                tool_call_id: update.tool_call_id.clone(),
                tool_name: update.tool_name.clone(),
                args: update.args.clone(),
                partial_result: update.partial_result.clone(),
            });
            self.tool_updates.push(update);
        }
        let mut result = raw.result;
        if let Some(after_tool_call) = &self.config.after_tool_call {
            let override_result = after_tool_call(AfterToolCallContext {
                assistant_message,
                tool_call: &raw.tool_call,
                args: &raw.tool_call.arguments,
                result: &result,
                messages: &self.messages,
            });
            if let Some(content) = override_result.content {
                result.content = content;
            }
            if override_result.details.is_some() {
                result.details = override_result.details;
            }
            if let Some(is_error) = override_result.is_error {
                result.is_error = is_error;
            }
            if let Some(terminate) = override_result.terminate {
                result.terminate = terminate;
            }
        }
        self.events.push(AgentLoopEvent::ToolExecutionEnd {
            tool_call_id: raw.tool_call.id.clone(),
            tool_name: raw.tool_call.name.clone(),
            result: result.clone(),
            is_error: result.is_error,
        });
        ExecutedToolCallResult {
            source_index: raw.source_index,
            tool_call: raw.tool_call,
            result,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.config
            .tools
            .iter()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema().unwrap_or_else(|| json!({})),
            })
            .collect()
    }

    fn convert_messages_to_llm(&self, messages: &[AgentMessage]) -> Vec<AiMessage> {
        if let Some(convert_to_llm) = &self.config.convert_to_llm {
            return convert_to_llm(messages);
        }

        messages
            .iter()
            .map(|message| AiMessage {
                role: message.role.clone(),
                content: message.content.clone(),
            })
            .collect()
    }
}

fn execute_prepared_tool_call(
    tool: &dyn AgentTool,
    prepared_tool_call: PreparedToolCall,
) -> RawExecutedToolCallResult {
    let mut updates = Vec::new();
    let mut on_update = |partial_result: AgentToolResult| {
        updates.push(ToolExecutionUpdate {
            tool_call_id: prepared_tool_call.tool_call.id.clone(),
            tool_name: prepared_tool_call.tool_call.name.clone(),
            args: prepared_tool_call.tool_call.arguments.clone(),
            partial_result,
        });
    };
    let result = tool.execute_with_update(&prepared_tool_call.tool_call, &mut on_update);
    RawExecutedToolCallResult {
        source_index: prepared_tool_call.source_index,
        tool_call: prepared_tool_call.tool_call,
        result,
        updates,
    }
}

fn validate_agent_tool_call(
    tool: &dyn AgentTool,
    tool_call: &AgentToolCall,
) -> Result<AgentToolCall, String> {
    let Some(parameters) = tool.parameters_schema() else {
        return Ok(tool_call.clone());
    };
    let ai_tool = ToolDefinition {
        name: tool.name().to_string(),
        description: String::new(),
        parameters,
    };
    let arguments = match &tool_call.arguments {
        Value::Object(arguments) => arguments
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
        _ => BTreeMap::new(),
    };
    let ai_tool_call = ToolCall {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments,
        thought_signature: None,
    };
    validate_tool_arguments(&ai_tool, &ai_tool_call).map(|arguments| AgentToolCall {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments,
    })
}

fn tool_calls_from_message(message: &AgentMessage) -> Vec<AgentToolCall> {
    message
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::ToolCall(tool_call) => Some(AgentToolCall {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: Value::Object(tool_call.arguments.clone().into_iter().collect()),
            }),
            _ => None,
        })
        .collect()
}

fn agent_messages_need_rich(messages: &[AgentMessage]) -> bool {
    messages.iter().any(|message| {
        !message.user_content_blocks.is_empty() || !message.content_blocks.is_empty()
    })
}

fn agent_messages_to_rich_messages(messages: &[AgentMessage], model: &Model) -> Vec<RichMessage> {
    messages
        .iter()
        .filter_map(|message| match message.role {
            MessageRole::System => None,
            MessageRole::User => Some(RichMessage::User(UserMessage {
                content: if message.user_content_blocks.is_empty() {
                    UserMessageContent::Text(message.content.clone())
                } else {
                    UserMessageContent::Blocks(message.user_content_blocks.clone())
                },
                timestamp_millis: 0,
            })),
            MessageRole::Assistant => Some(RichMessage::Assistant(RichAssistantMessage {
                content: rich_assistant_content(message),
                api: model.api.clone(),
                provider: model.provider.clone(),
                model: model.id.clone(),
                response_model: None,
                response_id: None,
                usage: message.usage.clone().unwrap_or_default(),
                stop_reason: message
                    .stop_reason
                    .clone()
                    .unwrap_or(AssistantStopReason::Stop),
                error_message: message.details.as_ref().and_then(|details| {
                    details
                        .get("errorMessage")
                        .and_then(|message| message.as_str())
                        .map(ToString::to_string)
                }),
                diagnostics: Vec::new(),
                timestamp_millis: 0,
            })),
            MessageRole::Tool => {
                let tool_call_id = message.tool_call_id.clone()?;
                let tool_name = message.tool_name.clone().unwrap_or_default();
                Some(RichMessage::ToolResult(ToolResultMessage {
                    tool_call_id,
                    tool_name,
                    content: if message.user_content_blocks.is_empty() {
                        text_tool_result_content(message.content.clone())
                    } else {
                        message.user_content_blocks.clone()
                    },
                    details: message.details.clone(),
                    is_error: message.is_error,
                    timestamp_millis: 0,
                }))
            }
        })
        .collect()
}

pub fn block_rich_message_images(messages: Vec<RichMessage>) -> Vec<RichMessage> {
    messages
        .into_iter()
        .map(|message| match message {
            RichMessage::User(mut user) => {
                if let UserMessageContent::Blocks(blocks) = user.content {
                    user.content = UserMessageContent::Blocks(block_user_content_images(blocks));
                }
                RichMessage::User(user)
            }
            RichMessage::ToolResult(mut tool_result) => {
                tool_result.content = block_user_content_images(tool_result.content);
                RichMessage::ToolResult(tool_result)
            }
            RichMessage::Assistant(assistant) => RichMessage::Assistant(assistant),
        })
        .collect()
}

fn block_user_content_images(blocks: Vec<UserContentBlock>) -> Vec<UserContentBlock> {
    const IMAGE_BLOCKED_TEXT: &str = "Image reading is disabled.";

    let mut filtered = Vec::with_capacity(blocks.len());
    for block in blocks {
        let replacement = match block {
            UserContentBlock::Image(_) => UserContentBlock::Text(TextContent {
                text: IMAGE_BLOCKED_TEXT.to_string(),
                text_signature: None,
            }),
            UserContentBlock::Text(text) => UserContentBlock::Text(text),
        };

        let duplicate_placeholder = matches!(
            (&replacement, filtered.last()),
            (
                UserContentBlock::Text(current),
                Some(UserContentBlock::Text(previous))
            ) if current.text == IMAGE_BLOCKED_TEXT && previous.text == IMAGE_BLOCKED_TEXT
        );
        if !duplicate_placeholder {
            filtered.push(replacement);
        }
    }
    filtered
}

fn rich_assistant_content(message: &AgentMessage) -> Vec<AssistantContentBlock> {
    if !message.content_blocks.is_empty() {
        return message.content_blocks.clone();
    }
    if message.content.is_empty() {
        return Vec::new();
    }
    vec![AssistantContentBlock::Text(TextContent {
        text: message.content.clone(),
        text_signature: None,
    })]
}

fn rich_assistant_text(message: &RichAssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn set_content_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    block: AssistantContentBlock,
) {
    if content_blocks.len() <= content_index {
        content_blocks.resize_with(content_index + 1, || {
            AssistantContentBlock::Text(TextContent {
                text: String::new(),
                text_signature: None,
            })
        });
    }
    content_blocks[content_index] = block;
}

fn append_text_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    delta: &str,
) {
    if content_blocks.len() <= content_index {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Text(TextContent {
                text: String::new(),
                text_signature: None,
            }),
        );
    }
    match &mut content_blocks[content_index] {
        AssistantContentBlock::Text(text) => text.text.push_str(delta),
        _ => set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Text(TextContent {
                text: delta.to_string(),
                text_signature: None,
            }),
        ),
    }
}

fn append_thinking_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    delta: &str,
) {
    if content_blocks.len() <= content_index {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: None,
                redacted: false,
            }),
        );
    }
    match &mut content_blocks[content_index] {
        AssistantContentBlock::Thinking(thinking) => thinking.thinking.push_str(delta),
        _ => set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: delta.to_string(),
                thinking_signature: None,
                redacted: false,
            }),
        ),
    }
}

fn append_tool_call_arguments_delta(
    content_blocks: &mut Vec<AssistantContentBlock>,
    argument_deltas: &mut BTreeMap<usize, String>,
    content_index: usize,
    delta: &str,
) {
    if content_blocks.len() <= content_index {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::ToolCall(ToolCall {
                id: String::new(),
                name: String::new(),
                arguments: BTreeMap::new(),
                thought_signature: None,
            }),
        );
    }

    if !matches!(
        content_blocks.get(content_index),
        Some(AssistantContentBlock::ToolCall(_))
    ) {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::ToolCall(ToolCall {
                id: String::new(),
                name: String::new(),
                arguments: BTreeMap::new(),
                thought_signature: None,
            }),
        );
    }

    let accumulated = argument_deltas.entry(content_index).or_default();
    accumulated.push_str(delta);
    let Ok(arguments) = serde_json::from_str::<BTreeMap<String, Value>>(accumulated) else {
        return;
    };
    if let Some(AssistantContentBlock::ToolCall(tool_call)) = content_blocks.get_mut(content_index)
    {
        tool_call.arguments = arguments;
    }
}

fn model_thinking_level_as_str(level: ModelThinkingLevel) -> &'static str {
    match level {
        ModelThinkingLevel::Off => "off",
        ModelThinkingLevel::Minimal => "minimal",
        ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High => "high",
        ModelThinkingLevel::XHigh => "xhigh",
    }
}

fn partial_assistant_message(
    content_blocks: &[AssistantContentBlock],
    usage: Option<ai::Usage>,
) -> AgentMessage {
    let content = content_blocks
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    AgentMessage {
        role: MessageRole::Assistant,
        content,
        content_blocks: content_blocks.to_vec(),
        user_content_blocks: Vec::new(),
        tool_call_id: None,
        tool_name: None,
        details: None,
        is_error: false,
        usage,
        stop_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{
        types::StreamToolCall, AiResult, ImageContent, LanguageModelProvider, Message, MessageRole,
        Model, ModelThinkingLevel, StreamEvent, StreamRequest, Usage, UserContentBlock,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct ScriptedToolLoopProvider {
        requests: Arc<Mutex<Vec<StreamRequest>>>,
    }

    impl ScriptedToolLoopProvider {
        fn new() -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl LanguageModelProvider for ScriptedToolLoopProvider {
        fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            let mut requests = self.requests.lock().expect("requests lock");
            requests.push(request);
            let call_count = requests.len();
            drop(requests);

            if call_count == 1 {
                return Ok(vec![
                    StreamEvent::TextDelta {
                        text: "checking".to_string(),
                    },
                    StreamEvent::ToolCallStart { content_index: 1 },
                    StreamEvent::ToolCallEnd {
                        content_index: 1,
                        tool_call: StreamToolCall {
                            id: "call_1".to_string(),
                            name: "read".to_string(),
                            arguments: BTreeMap::from([("path".to_string(), json!("README.md"))]),
                            thought_signature: None,
                        },
                    },
                    StreamEvent::Usage {
                        usage: Usage {
                            input: 1,
                            output: 2,
                            total_tokens: 3,
                            ..Usage::default()
                        },
                    },
                    StreamEvent::Finished {
                        message: Message {
                            role: MessageRole::Assistant,
                            content: "checking".to_string(),
                        },
                    },
                ]);
            }

            Ok(vec![StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: "done".to_string(),
                },
            }])
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedTwoToolLoopProvider {
        requests: Arc<Mutex<Vec<StreamRequest>>>,
    }

    impl ScriptedTwoToolLoopProvider {
        fn new() -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl LanguageModelProvider for ScriptedTwoToolLoopProvider {
        fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            let mut requests = self.requests.lock().expect("requests lock");
            requests.push(request);
            let call_count = requests.len();
            drop(requests);

            if call_count == 1 {
                return Ok(vec![
                    StreamEvent::ToolCallStart { content_index: 0 },
                    StreamEvent::ToolCallEnd {
                        content_index: 0,
                        tool_call: StreamToolCall {
                            id: "call_slow".to_string(),
                            name: "echo".to_string(),
                            arguments: BTreeMap::from([("value".to_string(), json!("slow"))]),
                            thought_signature: None,
                        },
                    },
                    StreamEvent::ToolCallStart { content_index: 1 },
                    StreamEvent::ToolCallEnd {
                        content_index: 1,
                        tool_call: StreamToolCall {
                            id: "call_fast".to_string(),
                            name: "echo".to_string(),
                            arguments: BTreeMap::from([("value".to_string(), json!("fast"))]),
                            thought_signature: None,
                        },
                    },
                    StreamEvent::Finished {
                        message: Message {
                            role: MessageRole::Assistant,
                            content: String::new(),
                        },
                    },
                ]);
            }

            Ok(vec![StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: "done".to_string(),
                },
            }])
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedTextProvider {
        requests: Arc<Mutex<Vec<StreamRequest>>>,
    }

    impl ScriptedTextProvider {
        fn new() -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl LanguageModelProvider for ScriptedTextProvider {
        fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            let mut requests = self.requests.lock().expect("requests lock");
            requests.push(request);
            let call_count = requests.len();
            Ok(vec![StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: format!("reply {call_count}"),
                },
            }])
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedToolDeltaProvider;

    impl LanguageModelProvider for ScriptedToolDeltaProvider {
        fn stream(&self, _request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            Ok(vec![
                StreamEvent::ToolCallStart { content_index: 0 },
                StreamEvent::ToolCallDelta {
                    content_index: 0,
                    delta: "{\"path\"".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    content_index: 0,
                    delta: ":\"README.md\"}".to_string(),
                },
                StreamEvent::ToolCallEnd {
                    content_index: 0,
                    tool_call: StreamToolCall {
                        id: "call_1".to_string(),
                        name: "read".to_string(),
                        arguments: BTreeMap::from([("path".to_string(), json!("README.md"))]),
                        thought_signature: None,
                    },
                },
                StreamEvent::Finished {
                    message: Message {
                        role: MessageRole::Assistant,
                        content: String::new(),
                    },
                },
            ])
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedErrorProvider;

    impl LanguageModelProvider for ScriptedErrorProvider {
        fn stream(&self, _request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            Ok(vec![StreamEvent::Error {
                message: "provider failed".to_string(),
            }])
        }
    }

    #[derive(Debug, Clone)]
    struct ReadTool;

    impl AgentTool for ReadTool {
        fn name(&self) -> &str {
            "read"
        }

        fn execute(&self, call: &AgentToolCall) -> AgentToolResult {
            assert_eq!(call.id, "call_1");
            assert_eq!(call.arguments["path"], json!("README.md"));
            AgentToolResult::text("file contents", Some(json!({"bytes": 13})), false, false)
        }
    }

    #[derive(Debug, Clone)]
    struct TerminatingReadTool;

    impl AgentTool for TerminatingReadTool {
        fn name(&self) -> &str {
            "read"
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            AgentToolResult::text("file contents", None, false, true)
        }
    }

    #[derive(Debug, Clone)]
    struct CountingTool {
        calls: Arc<AtomicUsize>,
    }

    impl AgentTool for CountingTool {
        fn name(&self) -> &str {
            "read"
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            AgentToolResult::text("should not run", None, false, false)
        }
    }

    #[derive(Debug, Clone)]
    struct UpdatingTool;

    impl AgentTool for UpdatingTool {
        fn name(&self) -> &str {
            "read"
        }

        fn execute_with_update(
            &self,
            call: &AgentToolCall,
            on_update: AgentToolUpdateCallback<'_>,
        ) -> AgentToolResult {
            assert_eq!(call.id, "call_1");
            on_update(AgentToolResult::text(
                "partial",
                Some(json!({"phase": "partial"})),
                false,
                false,
            ));
            AgentToolResult::text("final", Some(json!({"phase": "final"})), false, false)
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            panic!("execute_with_update should be used")
        }
    }

    #[derive(Debug, Clone)]
    struct TimedEchoTool;

    fn execute_timed_echo(call: &AgentToolCall) -> AgentToolResult {
        let value = call
            .arguments
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if value == "slow" {
            std::thread::sleep(Duration::from_millis(40));
        }
        AgentToolResult::text(
            format!("echo: {value}"),
            Some(json!({ "value": value })),
            false,
            false,
        )
    }

    impl AgentTool for TimedEchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn execute(&self, call: &AgentToolCall) -> AgentToolResult {
            execute_timed_echo(call)
        }
    }

    #[derive(Debug, Clone)]
    struct SequentialTimedEchoTool;

    impl AgentTool for SequentialTimedEchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn execution_mode(&self) -> Option<ToolExecutionMode> {
            Some(ToolExecutionMode::Sequential)
        }

        fn execute(&self, call: &AgentToolCall) -> AgentToolResult {
            execute_timed_echo(call)
        }
    }

    #[derive(Debug, Clone)]
    struct PreparingTool;

    impl AgentTool for PreparingTool {
        fn name(&self) -> &str {
            "read"
        }

        fn prepare_arguments(&self, arguments: &Value) -> Value {
            assert_eq!(arguments["path"], json!("README.md"));
            json!({"path": "prepared.md", "mode": "safe"})
        }

        fn execute(&self, call: &AgentToolCall) -> AgentToolResult {
            assert_eq!(call.id, "call_1");
            assert_eq!(call.arguments["path"], json!("prepared.md"));
            assert_eq!(call.arguments["mode"], json!("safe"));
            AgentToolResult::text(
                "prepared contents",
                Some(json!({"prepared": true})),
                false,
                true,
            )
        }
    }

    #[derive(Debug, Clone)]
    struct ValidatingTool;

    impl AgentTool for ValidatingTool {
        fn name(&self) -> &str {
            "read"
        }

        fn parameters_schema(&self) -> Option<Value> {
            Some(json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            }))
        }

        fn prepare_arguments(&self, _arguments: &Value) -> Value {
            json!({})
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            panic!("invalid tool arguments should block execution")
        }
    }

    #[derive(Debug, Clone)]
    struct DescribedTool;

    impl AgentTool for DescribedTool {
        fn name(&self) -> &str {
            "search"
        }

        fn description(&self) -> &str {
            "Search project files"
        }

        fn parameters_schema(&self) -> Option<Value> {
            Some(json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {"type": "string"}
                }
            }))
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            AgentToolResult::text("results", None, false, true)
        }
    }

    #[derive(Debug, Clone)]
    struct ImageResultTool;

    impl AgentTool for ImageResultTool {
        fn name(&self) -> &str {
            "read"
        }

        fn execute(&self, _call: &AgentToolCall) -> AgentToolResult {
            AgentToolResult {
                content: vec![
                    UserContentBlock::Text(TextContent {
                        text: "see image".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "aW1hZ2U=".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ],
                details: Some(json!({"kind": "screenshot"})),
                is_error: false,
                terminate: false,
            }
        }
    }

    #[test]
    fn agent_loop_executes_tool_result_and_continues_until_no_tool_calls() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(new_messages.len(), 4);
        assert_eq!(new_messages[0].role, MessageRole::User);
        assert_eq!(new_messages[1].role, MessageRole::Assistant);
        assert_eq!(new_messages[2].role, MessageRole::Tool);
        assert_eq!(new_messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(new_messages[2].tool_name.as_deref(), Some("read"));
        assert_eq!(new_messages[2].content, "file contents");
        assert_eq!(new_messages[3].content, "done");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1]
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool
                    && message.content == "file contents")
        );
    }

    #[test]
    fn agent_loop_preserves_tool_result_text_and_image_blocks_like_pi() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ImageResultTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert_eq!(tool_result.content, "see image");
        assert_eq!(
            tool_result.user_content_blocks,
            vec![
                UserContentBlock::Text(TextContent {
                    text: "see image".to_string(),
                    text_signature: None,
                }),
                UserContentBlock::Image(ImageContent {
                    data: "aW1hZ2U=".to_string(),
                    mime_type: "image/png".to_string(),
                }),
            ]
        );

        let execution_result = loop_runtime
            .events()
            .iter()
            .find_map(|event| match event {
                AgentLoopEvent::ToolExecutionEnd { result, .. } => Some(result),
                _ => None,
            })
            .expect("tool execution end result");
        assert_eq!(execution_result.content, tool_result.user_content_blocks);
    }

    #[test]
    fn agent_loop_default_provider_request_replays_tool_result_blocks_as_rich_messages_like_pi() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ImageResultTool)],
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        let tool_result = requests[1]
            .rich_messages
            .iter()
            .find_map(|message| match message {
                RichMessage::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .expect("second provider request should replay rich tool result");
        assert_eq!(tool_result.tool_call_id, "call_1");
        assert_eq!(tool_result.tool_name, "read");
        assert_eq!(
            tool_result.content,
            vec![
                UserContentBlock::Text(TextContent {
                    text: "see image".to_string(),
                    text_signature: None,
                }),
                UserContentBlock::Image(ImageContent {
                    data: "aW1hZ2U=".to_string(),
                    mime_type: "image/png".to_string(),
                }),
            ]
        );
    }

    #[test]
    fn agent_loop_passes_configured_tool_definitions_to_provider_like_pi_context_tools() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(DescribedTool)],
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].tools.len(), 1);
        assert_eq!(requests[0].tools[0].name, "search");
        assert_eq!(requests[0].tools[0].description, "Search project files");
        assert_eq!(
            requests[0].tools[0].parameters,
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {"type": "string"}
                }
            })
        );
    }

    #[test]
    fn agent_loop_marks_assistant_stop_reason_tool_use_when_stream_contains_tool_call_like_pi() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(new_messages[1].role, MessageRole::Assistant);
        assert_eq!(
            new_messages[1].stop_reason,
            Some(ai::AssistantStopReason::ToolUse)
        );
        assert_eq!(
            new_messages[3].stop_reason,
            Some(ai::AssistantStopReason::Stop)
        );
    }

    #[test]
    fn agent_loop_before_tool_call_can_block_execution_with_error_result() {
        let provider = ScriptedToolLoopProvider::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(CountingTool {
                    calls: calls.clone(),
                })],
                before_tool_call: Some(Box::new(|context| {
                    assert_eq!(context.tool_call.name, "read");
                    assert_eq!(context.args["path"], json!("README.md"));
                    assert_eq!(context.tool_call.arguments["path"], json!("README.md"));
                    BeforeToolCallResult {
                        block: true,
                        reason: Some("blocked by policy".to_string()),
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert_eq!(tool_result.content, "blocked by policy");
        assert!(tool_result.is_error);
        assert_eq!(new_messages.last().expect("final").content, "done");
    }

    #[test]
    fn agent_loop_prepares_tool_arguments_before_execution_like_pi_agent_loop() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(PreparingTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert_eq!(tool_result.content, "prepared contents");
        assert_eq!(tool_result.details, Some(json!({"prepared": true})));

        let tool_start = loop_runtime
            .events()
            .iter()
            .find_map(|event| match event {
                AgentLoopEvent::ToolExecutionStart { args, .. } => Some(args),
                _ => None,
            })
            .expect("tool start event");
        assert_eq!(tool_start["path"], json!("prepared.md"));
    }

    #[test]
    fn agent_loop_validates_prepared_tool_arguments_before_execution_like_pi_agent_loop() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ValidatingTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert!(tool_result.is_error);
        assert!(tool_result.content.contains("Validation failed for tool"));
        assert!(tool_result.content.contains("path"));
        assert!(tool_result.content.contains("is required"));
    }

    #[test]
    fn agent_loop_after_tool_call_can_override_result_fields() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                after_tool_call: Some(Box::new(|context| {
                    assert_eq!(context.tool_call.id, "call_1");
                    assert_eq!(context.args["path"], json!("README.md"));
                    assert_eq!(
                        tool_result_content_text(&context.result.content),
                        "file contents"
                    );
                    AfterToolCallResult {
                        content: Some(text_tool_result_content("overridden")),
                        details: Some(json!({"source": "after"})),
                        is_error: Some(true),
                        terminate: Some(true),
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert_eq!(tool_result.content, "overridden");
        assert_eq!(tool_result.details, Some(json!({"source": "after"})));
        assert!(tool_result.is_error);
        assert_eq!(
            new_messages
                .last()
                .expect("last should be tool result")
                .role,
            MessageRole::Tool
        );
    }

    #[test]
    fn agent_loop_collects_tool_execution_updates_from_tool_callback() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(UpdatingTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let updates = loop_runtime.tool_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].tool_call_id, "call_1");
        assert_eq!(updates[0].tool_name, "read");
        assert_eq!(
            tool_result_content_text(&updates[0].partial_result.content),
            "partial"
        );
        assert_eq!(
            updates[0].partial_result.details,
            Some(json!({"phase": "partial"}))
        );
        let tool_result = new_messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("tool result should exist");
        assert_eq!(tool_result.content, "final");
        assert_eq!(tool_result.details, Some(json!({"phase": "final"})));
    }

    #[test]
    fn agent_loop_executes_tool_calls_in_parallel_by_default_like_pi() {
        let provider = ScriptedTwoToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(TimedEchoTool)],
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "echo both")])
            .expect("loop should run");

        let tool_execution_end_ids = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::ToolExecutionEnd { tool_call_id, .. } => {
                    Some(tool_call_id.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let event_tool_result_ids = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::MessageEnd { message } if message.role == MessageRole::Tool => {
                    message.tool_call_id.as_deref()
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let returned_tool_result_ids = new_messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>();

        assert_eq!(tool_execution_end_ids, vec!["call_fast", "call_slow"]);
        assert_eq!(event_tool_result_ids, vec!["call_slow", "call_fast"]);
        assert_eq!(returned_tool_result_ids, vec!["call_slow", "call_fast"]);
    }

    #[test]
    fn agent_loop_can_execute_tool_calls_sequentially_from_config_like_pi() {
        let provider = ScriptedTwoToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(TimedEchoTool)],
                tool_execution: ToolExecutionMode::Sequential,
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "echo both")])
            .expect("loop should run");

        let tool_execution_end_ids = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::ToolExecutionEnd { tool_call_id, .. } => {
                    Some(tool_call_id.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_execution_end_ids, vec!["call_slow", "call_fast"]);
    }

    #[test]
    fn agent_loop_uses_sequential_batch_when_any_tool_requires_it_like_pi() {
        let provider = ScriptedTwoToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(SequentialTimedEchoTool)],
                tool_execution: ToolExecutionMode::Parallel,
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "echo both")])
            .expect("loop should run");

        let tool_execution_end_ids = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::ToolExecutionEnd { tool_call_id, .. } => {
                    Some(tool_call_id.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_execution_end_ids, vec!["call_slow", "call_fast"]);
    }

    #[test]
    fn agent_loop_should_stop_after_turn_can_end_without_follow_up_provider_call() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let seen_message = Arc::new(Mutex::new(None::<String>));
        let seen_message_for_hook = seen_message.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                should_stop_after_turn: Some(Box::new(move |context| {
                    *seen_message_for_hook.lock().expect("seen message lock") =
                        Some(context.message.content.clone());
                    assert_eq!(context.tool_results, []);
                    assert_eq!(context.new_messages.len(), 2);
                    true
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(requests.lock().expect("requests lock").len(), 1);
        assert_eq!(
            *seen_message.lock().expect("seen message lock"),
            Some("reply 1".to_string())
        );
        assert_eq!(new_messages.len(), 2);
        assert_eq!(new_messages[1].content, "reply 1");
    }

    #[test]
    fn agent_loop_get_follow_up_messages_injects_messages_before_next_turn() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let follow_up_calls = Arc::new(AtomicUsize::new(0));
        let follow_up_calls_for_hook = follow_up_calls.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                get_follow_up_messages: Some(Box::new(move || {
                    if follow_up_calls_for_hook.fetch_add(1, Ordering::SeqCst) == 0 {
                        vec![AgentMessage::new(MessageRole::User, "follow up")]
                    } else {
                        Vec::new()
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(follow_up_calls.load(Ordering::SeqCst), 2);
        assert_eq!(new_messages.len(), 4);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "reply 1");
        assert_eq!(new_messages[2].content, "follow up");
        assert_eq!(new_messages[3].content, "reply 2");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .messages
            .iter()
            .any(|message| message.role == MessageRole::User && message.content == "follow up"));

        let turn_start_count = loop_runtime
            .events()
            .iter()
            .filter(|event| matches!(event, AgentLoopEvent::TurnStart))
            .count();
        assert_eq!(turn_start_count, 2);
    }

    #[test]
    fn agent_loop_get_steering_messages_injects_messages_before_first_assistant_call() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let steering_calls = Arc::new(AtomicUsize::new(0));
        let steering_calls_for_hook = steering_calls.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                get_steering_messages: Some(Box::new(move || {
                    if steering_calls_for_hook.fetch_add(1, Ordering::SeqCst) == 0 {
                        vec![AgentMessage::new(MessageRole::User, "steer")]
                    } else {
                        Vec::new()
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(steering_calls.load(Ordering::SeqCst), 2);
        assert_eq!(new_messages.len(), 3);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "steer");
        assert_eq!(new_messages[2].content, "reply 1");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages[0].content, "inspect");
        assert_eq!(requests[0].messages[1].content, "steer");

        let steering_event_types = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::MessageStart { message }
                | AgentLoopEvent::MessageEnd { message }
                    if message.content == "steer" =>
                {
                    Some(event.event_type())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(steering_event_types, vec!["message_start", "message_end"]);
    }

    #[test]
    fn agent_loop_get_steering_messages_polls_after_turn_and_continues() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let steering_calls = Arc::new(AtomicUsize::new(0));
        let steering_calls_for_hook = steering_calls.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                get_steering_messages: Some(Box::new(move || {
                    if steering_calls_for_hook.fetch_add(1, Ordering::SeqCst) == 1 {
                        vec![AgentMessage::new(MessageRole::User, "late steer")]
                    } else {
                        Vec::new()
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(steering_calls.load(Ordering::SeqCst), 3);
        assert_eq!(new_messages.len(), 4);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "reply 1");
        assert_eq!(new_messages[2].content, "late steer");
        assert_eq!(new_messages[3].content, "reply 2");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .messages
            .iter()
            .any(|message| message.role == MessageRole::User && message.content == "late steer"));
    }

    #[test]
    fn agent_loop_transform_context_rewrites_provider_request_without_replacing_loop_context() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                transform_context: Some(Box::new(|messages| {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].content, "inspect");
                    vec![AgentMessage::new(MessageRole::User, "transformed")]
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(new_messages.len(), 2);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "reply 1");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.len(), 1);
        assert_eq!(requests[0].messages[0].content, "transformed");
    }

    #[test]
    fn agent_loop_convert_to_llm_rewrites_provider_request_after_transform_context_like_pi() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                transform_context: Some(Box::new(|messages| {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].content, "inspect");
                    vec![
                        AgentMessage::new(MessageRole::Assistant, "previous assistant"),
                        AgentMessage::new(MessageRole::User, "transformed user"),
                    ]
                })),
                convert_to_llm: Some(Box::new(|messages| {
                    assert_eq!(messages.len(), 2);
                    assert_eq!(messages[0].content, "previous assistant");
                    assert_eq!(messages[1].content, "transformed user");
                    vec![Message {
                        role: MessageRole::User,
                        content: "converted for provider".to_string(),
                    }]
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(new_messages.len(), 2);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "reply 1");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].messages,
            vec![Message {
                role: MessageRole::User,
                content: "converted for provider".to_string(),
            }]
        );
    }

    #[test]
    fn agent_loop_convert_to_rich_llm_populates_rich_provider_request_after_transform_context_like_pi(
    ) {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                transform_context: Some(Box::new(|messages| {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].content, "inspect");
                    vec![AgentMessage::new(MessageRole::Assistant, "rich source")]
                })),
                convert_to_rich_llm: Some(Box::new(|messages| {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].content, "rich source");
                    vec![ai::RichMessage::Assistant(ai::RichAssistantMessage {
                        content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: "reasoning".to_string(),
                            thinking_signature: Some("sig".to_string()),
                            redacted: false,
                        })],
                        provider: "local".to_string(),
                        api: "faux".to_string(),
                        model: "model".to_string(),
                        response_model: None,
                        response_id: None,
                        usage: Usage::default(),
                        stop_reason: ai::AssistantStopReason::Stop,
                        error_message: None,
                        diagnostics: Vec::new(),
                        timestamp_millis: 0,
                    })]
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(new_messages.len(), 2);
        assert_eq!(new_messages[0].content, "inspect");
        assert_eq!(new_messages[1].content, "reply 1");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].messages.is_empty());
        assert_eq!(requests[0].rich_messages.len(), 1);
        let ai::RichMessage::Assistant(assistant) = &requests[0].rich_messages[0] else {
            panic!("expected assistant rich message");
        };
        assert_eq!(
            assistant.content,
            vec![AssistantContentBlock::Thinking(ThinkingContent {
                thinking: "reasoning".to_string(),
                thinking_signature: Some("sig".to_string()),
                redacted: false,
            })]
        );
    }

    #[test]
    fn block_rich_message_images_replaces_user_and_tool_images_like_pi_sdk() {
        let messages = vec![
            ai::RichMessage::User(ai::UserMessage {
                content: ai::UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(TextContent {
                        text: "before".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "image-1".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "image-2".to_string(),
                        mime_type: "image/jpeg".to_string(),
                    }),
                    UserContentBlock::Text(TextContent {
                        text: "after".to_string(),
                        text_signature: None,
                    }),
                ]),
                timestamp_millis: 1,
            }),
            ai::RichMessage::ToolResult(ai::ToolResultMessage {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                content: vec![
                    UserContentBlock::Image(ImageContent {
                        data: "tool-image".to_string(),
                        mime_type: "image/webp".to_string(),
                    }),
                    UserContentBlock::Text(TextContent {
                        text: "tool text".to_string(),
                        text_signature: None,
                    }),
                ],
                details: None,
                is_error: false,
                timestamp_millis: 2,
            }),
        ];

        let filtered = block_rich_message_images(messages);

        let ai::RichMessage::User(user) = &filtered[0] else {
            panic!("expected user message");
        };
        assert_eq!(
            user.content,
            ai::UserMessageContent::Blocks(vec![
                UserContentBlock::Text(TextContent {
                    text: "before".to_string(),
                    text_signature: None,
                }),
                UserContentBlock::Text(TextContent {
                    text: "Image reading is disabled.".to_string(),
                    text_signature: None,
                }),
                UserContentBlock::Text(TextContent {
                    text: "after".to_string(),
                    text_signature: None,
                }),
            ])
        );

        let ai::RichMessage::ToolResult(tool_result) = &filtered[1] else {
            panic!("expected tool result message");
        };
        assert_eq!(
            tool_result.content,
            vec![
                UserContentBlock::Text(TextContent {
                    text: "Image reading is disabled.".to_string(),
                    text_signature: None,
                }),
                UserContentBlock::Text(TextContent {
                    text: "tool text".to_string(),
                    text_signature: None,
                }),
            ]
        );
    }

    #[test]
    fn agent_loop_prepare_next_turn_can_replace_context_before_next_provider_call() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let prepare_calls = Arc::new(AtomicUsize::new(0));
        let prepare_calls_for_hook = prepare_calls.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                prepare_next_turn: Some(Box::new(move |context| {
                    let call_count = prepare_calls_for_hook.fetch_add(1, Ordering::SeqCst);
                    if call_count == 0 {
                        assert_eq!(context.message.content, "checking");
                        assert_eq!(context.tool_results.len(), 1);
                        assert!(context
                            .messages
                            .iter()
                            .any(|message| message.role == MessageRole::Tool));
                        PrepareNextTurnResult {
                            messages: Some(vec![AgentMessage::new(MessageRole::User, "prepared")]),
                            model: None,
                            thinking_level: None,
                        }
                    } else {
                        assert_eq!(context.message.content, "done");
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: None,
                        }
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        assert_eq!(prepare_calls.load(Ordering::SeqCst), 2);
        assert_eq!(new_messages.len(), 4);
        assert_eq!(new_messages[3].content, "done");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].messages.len(), 1);
        assert_eq!(requests[1].messages[0].content, "prepared");
    }

    #[test]
    fn agent_loop_prepare_next_turn_can_switch_model_before_next_provider_call() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "initial-model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Initial Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                prepare_next_turn: Some(Box::new(|context| {
                    if context.message.content == "checking" {
                        PrepareNextTurnResult {
                            messages: None,
                            model: Some(Model {
                                id: "prepared-model".to_string(),
                                provider: "local".to_string(),
                                api: "faux".to_string(),
                                display_name: "Prepared Model".to_string(),
                                context_window: 2000,
                                ..Model::default()
                            }),
                            thinking_level: None,
                        }
                    } else {
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: None,
                        }
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].model.id, "initial-model");
        assert_eq!(requests[1].model.id, "prepared-model");
    }

    #[test]
    fn agent_loop_prepare_next_turn_can_switch_reasoning_before_next_provider_call() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                reasoning: Some(ModelThinkingLevel::Medium),
                prepare_next_turn: Some(Box::new(|context| {
                    if context.message.content == "checking" {
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: Some(ModelThinkingLevel::High),
                        }
                    } else {
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: None,
                        }
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].metadata.get("reasoning"),
            Some(&json!("medium"))
        );
        assert_eq!(requests[1].metadata.get("reasoning"), Some(&json!("high")));
    }

    #[test]
    fn agent_loop_prepare_next_turn_can_clear_reasoning_before_next_provider_call() {
        let provider = ScriptedToolLoopProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                reasoning: Some(ModelThinkingLevel::High),
                prepare_next_turn: Some(Box::new(|context| {
                    if context.message.content == "checking" {
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: Some(ModelThinkingLevel::Off),
                        }
                    } else {
                        PrepareNextTurnResult {
                            messages: None,
                            model: None,
                            thinking_level: None,
                        }
                    }
                })),
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].metadata.get("reasoning"), Some(&json!("high")));
        assert_eq!(requests[1].metadata.get("reasoning"), None);
    }

    #[test]
    fn agent_loop_records_basic_lifecycle_events_like_pi_agent_loop() {
        let provider = ScriptedTextProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(model, "", provider, AgentLoopConfig::default());

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let event_types = loop_runtime
            .events()
            .iter()
            .map(AgentLoopEvent::event_type)
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                "agent_start",
                "turn_start",
                "message_start",
                "message_end",
                "message_start",
                "message_end",
                "turn_end",
                "agent_end",
            ]
        );
    }

    #[test]
    fn agent_loop_records_tool_execution_events_like_pi_agent_loop() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(UpdatingTool)],
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let event_types = loop_runtime
            .events()
            .iter()
            .map(AgentLoopEvent::event_type)
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                "agent_start",
                "turn_start",
                "message_start",
                "message_end",
                "message_update",
                "message_update",
                "message_update",
                "message_update",
                "message_start",
                "message_end",
                "tool_execution_start",
                "tool_execution_update",
                "tool_execution_end",
                "message_start",
                "message_end",
                "turn_end",
                "turn_start",
                "message_start",
                "message_end",
                "turn_end",
                "agent_end",
            ]
        );

        let tool_start = loop_runtime
            .events()
            .iter()
            .find_map(|event| match event {
                AgentLoopEvent::ToolExecutionStart {
                    tool_call_id,
                    tool_name,
                    args,
                } => Some((tool_call_id, tool_name, args)),
                _ => None,
            })
            .expect("tool execution start event");
        assert_eq!(tool_start.0, "call_1");
        assert_eq!(tool_start.1, "read");
        assert_eq!(tool_start.2["path"], json!("README.md"));
    }

    #[test]
    fn agent_loop_records_assistant_message_update_for_text_delta_like_pi_agent_loop() {
        let provider = ScriptedToolLoopProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            provider,
            AgentLoopConfig {
                tools: vec![Box::new(ReadTool)],
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let update = loop_runtime
            .events()
            .iter()
            .find_map(|event| match event {
                AgentLoopEvent::MessageUpdate {
                    message,
                    assistant_message_event,
                } => Some((message, assistant_message_event)),
                _ => None,
            })
            .expect("assistant update event");
        assert_eq!(update.0.role, MessageRole::Assistant);
        assert_eq!(update.0.content, "checking");
        assert!(matches!(
            update.1,
            StreamEvent::TextDelta { text } if text == "checking"
        ));
    }

    #[test]
    fn agent_loop_records_tool_call_delta_message_updates_like_pi_agent_loop() {
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(
            model,
            "",
            ScriptedToolDeltaProvider,
            AgentLoopConfig {
                tools: vec![Box::new(TerminatingReadTool)],
                ..AgentLoopConfig::default()
            },
        );

        loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("loop should run");

        let delta_updates = loop_runtime
            .events()
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::MessageUpdate {
                    assistant_message_event:
                        StreamEvent::ToolCallDelta {
                            content_index,
                            delta,
                        },
                    message,
                } => Some((*content_index, delta.as_str(), message)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(delta_updates.len(), 2);
        assert_eq!(delta_updates[0].0, 0);
        assert_eq!(delta_updates[0].1, "{\"path\"");
        assert!(matches!(
            delta_updates[0].2.content_blocks.first(),
            Some(AssistantContentBlock::ToolCall(tool_call))
                if tool_call.arguments.is_empty()
        ));
        assert_eq!(delta_updates[1].1, ":\"README.md\"}");
        assert!(matches!(
            delta_updates[1].2.content_blocks.first(),
            Some(AssistantContentBlock::ToolCall(tool_call))
                if tool_call.arguments.get("path") == Some(&json!("README.md"))
        ));
    }

    #[test]
    fn agent_loop_records_stream_error_as_final_assistant_message_like_pi_agent_loop() {
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime =
            AgentLoop::new(model, "", ScriptedErrorProvider, AgentLoopConfig::default());

        let new_messages = loop_runtime
            .run(vec![AgentMessage::new(MessageRole::User, "inspect")])
            .expect("stream error should become assistant message");

        assert_eq!(new_messages.len(), 2);
        let assistant = &new_messages[1];
        assert_eq!(assistant.role, MessageRole::Assistant);
        assert_eq!(assistant.content, "");
        assert!(assistant.is_error);
        assert_eq!(assistant.stop_reason, Some(ai::AssistantStopReason::Error));
        assert_eq!(
            assistant.details,
            Some(json!({"errorMessage": "provider failed"}))
        );

        let event_types = loop_runtime
            .events()
            .iter()
            .map(AgentLoopEvent::event_type)
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                "agent_start",
                "turn_start",
                "message_start",
                "message_end",
                "message_start",
                "message_end",
                "turn_end",
                "agent_end",
            ]
        );
    }

    #[test]
    fn agent_loop_continue_from_context_runs_without_adding_prompt_messages() {
        let provider = ScriptedTextProvider::new();
        let requests = provider.requests.clone();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(model, "", provider, AgentLoopConfig::default());

        let new_messages = loop_runtime
            .continue_from_context(vec![AgentMessage::new(MessageRole::User, "retry this")])
            .expect("loop should continue");

        assert_eq!(new_messages.len(), 1);
        assert_eq!(new_messages[0].role, MessageRole::Assistant);
        assert_eq!(new_messages[0].content, "reply 1");

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.len(), 1);
        assert_eq!(requests[0].messages[0].content, "retry this");
    }

    #[test]
    fn agent_loop_continue_from_context_rejects_empty_or_assistant_last_context() {
        let provider = ScriptedTextProvider::new();
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Model".to_string(),
            context_window: 1000,
            ..Model::default()
        };
        let mut loop_runtime = AgentLoop::new(model, "", provider, AgentLoopConfig::default());

        let empty_error = loop_runtime
            .continue_from_context(Vec::new())
            .expect_err("empty context should fail");
        assert_eq!(
            empty_error.to_string(),
            "Cannot continue: no messages in context"
        );

        let assistant_error = loop_runtime
            .continue_from_context(vec![AgentMessage::new(MessageRole::Assistant, "done")])
            .expect_err("assistant context should fail");
        assert_eq!(
            assistant_error.to_string(),
            "Cannot continue from message role: assistant"
        );
    }
}
