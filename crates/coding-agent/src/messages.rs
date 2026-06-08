use ai::{ImageContent, TextContent, UserContentBlock};

pub const COMPACTION_SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";
pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

#[derive(Debug, Clone, PartialEq)]
pub enum CodingAgentLlmMessage {
    BashExecution(BashExecutionMessage),
    Custom(CustomMessage),
    BranchSummary(BranchSummaryMessage),
    CompactionSummary(CompactionSummaryMessage),
    Rich(ai::RichMessage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashExecutionMessage {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
    pub timestamp_millis: u128,
    pub exclude_from_context: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomMessage {
    pub custom_type: String,
    pub content: Vec<UserContentBlock>,
    pub display: bool,
    pub details: Option<serde_json::Value>,
    pub timestamp_millis: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryMessage {
    pub summary: String,
    pub from_id: String,
    pub timestamp_millis: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionSummaryMessage {
    pub summary: String,
    pub tokens_before: u64,
    pub timestamp_millis: u128,
}

pub fn text_block(text: impl Into<String>) -> UserContentBlock {
    UserContentBlock::Text(TextContent {
        text: text.into(),
        text_signature: None,
    })
}

pub fn image_block(data: impl Into<String>, mime_type: impl Into<String>) -> UserContentBlock {
    UserContentBlock::Image(ImageContent {
        data: data.into(),
        mime_type: mime_type.into(),
    })
}

pub fn bash_execution_to_text(message: &BashExecutionMessage) -> String {
    let mut text = format!("Ran `{}`\n", message.command);
    if message.output.is_empty() {
        text.push_str("(no output)");
    } else {
        text.push_str("```\n");
        text.push_str(&message.output);
        text.push_str("\n```");
    }

    if message.cancelled {
        text.push_str("\n\n(command cancelled)");
    } else if message.exit_code.is_some_and(|code| code != 0) {
        text.push_str(&format!(
            "\n\nCommand exited with code {}",
            message.exit_code.unwrap_or_default()
        ));
    }

    if message.truncated {
        if let Some(path) = &message.full_output_path {
            text.push_str(&format!("\n\n[Output truncated. Full output: {path}]"));
        }
    }
    text
}

pub fn convert_to_llm(messages: Vec<CodingAgentLlmMessage>) -> Vec<ai::RichMessage> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            CodingAgentLlmMessage::BashExecution(message) => {
                if message.exclude_from_context {
                    return None;
                }
                Some(user_blocks_message(
                    vec![text_block(bash_execution_to_text(&message))],
                    message.timestamp_millis,
                ))
            }
            CodingAgentLlmMessage::Custom(message) => Some(user_blocks_message(
                message.content,
                message.timestamp_millis,
            )),
            CodingAgentLlmMessage::BranchSummary(message) => Some(user_blocks_message(
                vec![text_block(format!(
                    "{BRANCH_SUMMARY_PREFIX}{}{BRANCH_SUMMARY_SUFFIX}",
                    message.summary
                ))],
                message.timestamp_millis,
            )),
            CodingAgentLlmMessage::CompactionSummary(message) => Some(user_blocks_message(
                vec![text_block(format!(
                    "{COMPACTION_SUMMARY_PREFIX}{}{COMPACTION_SUMMARY_SUFFIX}",
                    message.summary
                ))],
                message.timestamp_millis,
            )),
            CodingAgentLlmMessage::Rich(message) => Some(message),
        })
        .collect()
}

fn user_blocks_message(content: Vec<UserContentBlock>, timestamp_millis: u128) -> ai::RichMessage {
    ai::RichMessage::User(ai::UserMessage {
        content: ai::UserMessageContent::Blocks(content),
        timestamp_millis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_blocks(message: &ai::RichMessage) -> &[UserContentBlock] {
        let ai::RichMessage::User(user) = message else {
            panic!("expected user rich message");
        };
        let ai::UserMessageContent::Blocks(blocks) = &user.content else {
            panic!("expected block content");
        };
        blocks
    }

    #[test]
    fn bash_execution_to_text_matches_pi_messages_transformer() {
        let message = BashExecutionMessage {
            command: "cargo test".to_string(),
            output: "failed".to_string(),
            exit_code: Some(101),
            cancelled: false,
            truncated: true,
            full_output_path: Some("/tmp/full.log".to_string()),
            timestamp_millis: 12,
            exclude_from_context: false,
        };

        assert_eq!(
            bash_execution_to_text(&message),
            "Ran `cargo test`\n```\nfailed\n```\n\nCommand exited with code 101\n\n[Output truncated. Full output: /tmp/full.log]"
        );
    }

    #[test]
    fn convert_to_llm_maps_custom_messages_and_summaries_like_pi_messages() {
        let messages = convert_to_llm(vec![
            CodingAgentLlmMessage::BashExecution(BashExecutionMessage {
                command: "secret".to_string(),
                output: "ignored".to_string(),
                exit_code: Some(0),
                cancelled: false,
                truncated: false,
                full_output_path: None,
                timestamp_millis: 1,
                exclude_from_context: true,
            }),
            CodingAgentLlmMessage::Custom(CustomMessage {
                custom_type: "notice".to_string(),
                content: vec![text_block("look"), image_block("base64", "image/png")],
                display: true,
                details: None,
                timestamp_millis: 2,
            }),
            CodingAgentLlmMessage::BranchSummary(BranchSummaryMessage {
                summary: "branch notes".to_string(),
                from_id: "entry-1".to_string(),
                timestamp_millis: 3,
            }),
            CodingAgentLlmMessage::CompactionSummary(CompactionSummaryMessage {
                summary: "older notes".to_string(),
                tokens_before: 42,
                timestamp_millis: 4,
            }),
        ]);

        assert_eq!(messages.len(), 3);
        assert_eq!(
            user_blocks(&messages[0]),
            &[text_block("look"), image_block("base64", "image/png"),]
        );
        assert_eq!(
            user_blocks(&messages[1]),
            &[text_block(format!(
                "{BRANCH_SUMMARY_PREFIX}branch notes{BRANCH_SUMMARY_SUFFIX}"
            ))]
        );
        assert_eq!(
            user_blocks(&messages[2]),
            &[text_block(format!(
                "{COMPACTION_SUMMARY_PREFIX}older notes{COMPACTION_SUMMARY_SUFFIX}"
            ))]
        );
    }
}
