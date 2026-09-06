use ai::{AssistantContentBlock, AssistantStopReason, RichAssistantMessage};

use crate::user_message::add_osc133_zone_markers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantMessageLine {
    Blank,
    Markdown(String),
    Thinking(String),
    HiddenThinking(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct AssistantMessageState {
    message: RichAssistantMessage,
    hide_thinking_block: bool,
    hidden_thinking_label: String,
}

impl AssistantMessageState {
    pub fn new(message: RichAssistantMessage) -> Self {
        Self {
            message,
            hide_thinking_block: false,
            hidden_thinking_label: "Thinking...".to_string(),
        }
    }

    pub fn set_message(&mut self, message: RichAssistantMessage) {
        self.message = message;
    }

    pub fn set_hide_thinking_block(&mut self, hide: bool) {
        self.hide_thinking_block = hide;
    }

    pub fn set_hidden_thinking_label(&mut self, label: String) {
        self.hidden_thinking_label = label;
    }

    pub fn has_tool_calls(&self) -> bool {
        self.message
            .content
            .iter()
            .any(|content| matches!(content, AssistantContentBlock::ToolCall(_)))
    }

    pub fn render_lines(&self) -> Vec<AssistantMessageLine> {
        let mut lines = Vec::new();
        let has_visible_content = self.message.content.iter().any(visible_content);

        if has_visible_content {
            lines.push(AssistantMessageLine::Blank);
        }

        for (index, content) in self.message.content.iter().enumerate() {
            match content {
                AssistantContentBlock::Text(text) if !text.text.trim().is_empty() => {
                    lines.push(AssistantMessageLine::Markdown(text.text.trim().to_string()));
                }
                AssistantContentBlock::Thinking(thinking)
                    if !thinking.thinking.trim().is_empty() =>
                {
                    let has_visible_content_after = self
                        .message
                        .content
                        .iter()
                        .skip(index + 1)
                        .any(visible_content);
                    if self.hide_thinking_block {
                        lines.push(AssistantMessageLine::HiddenThinking(
                            self.hidden_thinking_label.clone(),
                        ));
                    } else {
                        lines.push(AssistantMessageLine::Thinking(
                            thinking.thinking.trim().to_string(),
                        ));
                    }
                    if has_visible_content_after {
                        lines.push(AssistantMessageLine::Blank);
                    }
                }
                _ => {}
            }
        }

        if !self.has_tool_calls() {
            match self.message.stop_reason {
                AssistantStopReason::Aborted => {
                    lines.push(AssistantMessageLine::Blank);
                    lines.push(AssistantMessageLine::Error(self.abort_message()));
                }
                AssistantStopReason::Error => {
                    lines.push(AssistantMessageLine::Blank);
                    lines.push(AssistantMessageLine::Error(format!(
                        "Error: {}",
                        self.message
                            .error_message
                            .as_deref()
                            .unwrap_or("Unknown error")
                    )));
                }
                _ => {}
            }
        }

        lines
    }

    pub fn render_marked_text_lines(&self) -> Vec<String> {
        let mut lines = self
            .render_lines()
            .into_iter()
            .map(|line| match line {
                AssistantMessageLine::Blank => String::new(),
                AssistantMessageLine::Markdown(text)
                | AssistantMessageLine::Thinking(text)
                | AssistantMessageLine::HiddenThinking(text)
                | AssistantMessageLine::Error(text) => text,
            })
            .collect::<Vec<_>>();
        if !self.has_tool_calls() {
            add_osc133_zone_markers(&mut lines);
        }
        lines
    }

    fn abort_message(&self) -> String {
        match self.message.error_message.as_deref() {
            Some(message) if message != "Request was aborted" => message.to_string(),
            _ => "Operation aborted".to_string(),
        }
    }
}

fn visible_content(content: &AssistantContentBlock) -> bool {
    match content {
        AssistantContentBlock::Text(text) => !text.text.trim().is_empty(),
        AssistantContentBlock::Thinking(thinking) => !thinking.thinking.trim().is_empty(),
        AssistantContentBlock::ToolCall(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{
        AssistantContentBlock, AssistantStopReason, RichAssistantMessage, TextContent,
        ThinkingContent, ToolCall, Usage,
    };
    use std::collections::BTreeMap;

    fn assistant_message(content: Vec<AssistantContentBlock>) -> RichAssistantMessage {
        RichAssistantMessage {
            content,
            api: "responses".to_string(),
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            response_model: None,
            response_id: None,
            usage: Usage::default(),
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        }
    }

    #[test]
    fn assistant_message_renders_text_and_thinking_blocks_in_order_like_pi() {
        let message = assistant_message(vec![
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: " reasoning ".to_string(),
                thinking_signature: None,
                redacted: false,
            }),
            AssistantContentBlock::Text(TextContent {
                text: " answer ".to_string(),
                text_signature: None,
            }),
        ]);

        let state = AssistantMessageState::new(message);

        assert_eq!(
            state.render_lines(),
            vec![
                AssistantMessageLine::Blank,
                AssistantMessageLine::Thinking("reasoning".to_string()),
                AssistantMessageLine::Blank,
                AssistantMessageLine::Markdown("answer".to_string()),
            ]
        );
    }

    #[test]
    fn assistant_message_uses_hidden_thinking_label_when_configured() {
        let message = assistant_message(vec![
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: "private trace".to_string(),
                thinking_signature: None,
                redacted: false,
            }),
            AssistantContentBlock::Text(TextContent {
                text: "visible".to_string(),
                text_signature: None,
            }),
        ]);

        let mut state = AssistantMessageState::new(message);
        state.set_hide_thinking_block(true);
        state.set_hidden_thinking_label("Thinking...".to_string());

        assert_eq!(
            state.render_lines(),
            vec![
                AssistantMessageLine::Blank,
                AssistantMessageLine::HiddenThinking("Thinking...".to_string()),
                AssistantMessageLine::Blank,
                AssistantMessageLine::Markdown("visible".to_string()),
            ]
        );
    }

    #[test]
    fn assistant_message_shows_abort_or_error_only_without_tool_calls() {
        let mut aborted = assistant_message(vec![AssistantContentBlock::Text(TextContent {
            text: "partial".to_string(),
            text_signature: None,
        })]);
        aborted.stop_reason = AssistantStopReason::Aborted;
        aborted.error_message = Some("Request was aborted".to_string());

        let state = AssistantMessageState::new(aborted);
        assert_eq!(
            state.render_lines(),
            vec![
                AssistantMessageLine::Blank,
                AssistantMessageLine::Markdown("partial".to_string()),
                AssistantMessageLine::Blank,
                AssistantMessageLine::Error("Operation aborted".to_string()),
            ]
        );

        let mut error_with_tool =
            assistant_message(vec![AssistantContentBlock::ToolCall(ToolCall {
                id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: BTreeMap::new(),
                thought_signature: None,
            })]);
        error_with_tool.stop_reason = AssistantStopReason::Error;
        error_with_tool.error_message = Some("failed".to_string());

        let state = AssistantMessageState::new(error_with_tool);
        assert!(state.has_tool_calls());
        assert!(state.render_lines().is_empty());
    }

    #[test]
    fn assistant_message_adds_osc133_markers_without_tool_calls_like_pi() {
        let state =
            AssistantMessageState::new(assistant_message(vec![AssistantContentBlock::Text(
                TextContent {
                    text: "hello".to_string(),
                    text_signature: None,
                },
            )]));

        let lines = state.render_marked_text_lines();

        assert!(!lines.is_empty());
        assert!(lines[0].starts_with(crate::user_message::OSC133_ZONE_START));
        assert!(lines.last().is_some_and(|line| line.starts_with(&format!(
            "{}{}",
            crate::user_message::OSC133_ZONE_END,
            crate::user_message::OSC133_ZONE_FINAL
        ))));
    }

    #[test]
    fn assistant_message_skips_osc133_markers_when_tool_calls_exist_like_pi() {
        let state = AssistantMessageState::new(assistant_message(vec![
            AssistantContentBlock::Text(TextContent {
                text: "calling tool".to_string(),
                text_signature: None,
            }),
            AssistantContentBlock::ToolCall(ToolCall {
                id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: BTreeMap::new(),
                thought_signature: None,
            }),
        ]));

        let rendered = state.render_marked_text_lines().join("\n");

        assert!(!rendered.contains(crate::user_message::OSC133_ZONE_START));
        assert!(!rendered.contains(crate::user_message::OSC133_ZONE_END));
        assert!(!rendered.contains(crate::user_message::OSC133_ZONE_FINAL));
    }
}
