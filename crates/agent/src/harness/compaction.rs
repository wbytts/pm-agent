use ai::{
    AssistantStopReason, MessageRole, RichMessage, Usage, UserContentBlock, UserMessageContent,
};

use crate::harness::messages::{
    BranchSummaryMessage, CompactionSummaryMessage, CustomMessage, BRANCH_SUMMARY_PREFIX,
    BRANCH_SUMMARY_SUFFIX, COMPACTION_SUMMARY_PREFIX, COMPACTION_SUMMARY_SUFFIX,
};
use crate::harness::session::{Session, SessionStorage, SessionTreeEntry};
use crate::harness::types::{SessionError, SessionErrorCode, SessionResult};
use crate::state::AgentMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

pub const DEFAULT_COMPACTION_SETTINGS: CompactionSettings = CompactionSettings {
    enabled: true,
    reserve_tokens: 16_384,
    keep_recent_tokens: 20_000,
};

pub const BRANCH_SUMMARY_PREAMBLE: &str = "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

#[derive(Debug, Clone)]
pub struct BranchSummaryEntries {
    pub entries: Vec<SessionTreeEntry>,
    pub common_ancestor_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOperations {
    pub read: std::collections::BTreeSet<String>,
    pub written: std::collections::BTreeSet<String>,
    pub edited: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct BranchPreparation {
    pub messages: Vec<AgentMessage>,
    pub file_ops: FileOperations,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryResult {
    pub summary: String,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsageEstimate {
    pub tokens: u64,
    pub usage_tokens: u64,
    pub trailing_tokens: u64,
    pub last_usage_index: Option<usize>,
}

pub fn calculate_context_tokens(usage: &Usage) -> u64 {
    if usage.total_tokens > 0 {
        usage.total_tokens as u64
    } else {
        (usage.input + usage.output + usage.cache_read + usage.cache_write) as u64
    }
}

pub fn should_compact(
    context_tokens: u64,
    context_window: u64,
    settings: &CompactionSettings,
) -> bool {
    if !settings.enabled {
        return false;
    }
    context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

pub fn estimate_context_tokens(messages: &[AgentMessage]) -> ContextUsageEstimate {
    if let Some((index, usage)) = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| assistant_usage(message).map(|usage| (index, usage)))
    {
        let usage_tokens = calculate_context_tokens(usage);
        let trailing_tokens = messages[index + 1..].iter().map(estimate_tokens).sum();
        return ContextUsageEstimate {
            tokens: usage_tokens + trailing_tokens,
            usage_tokens,
            trailing_tokens,
            last_usage_index: Some(index),
        };
    }

    let tokens = messages.iter().map(estimate_tokens).sum();
    ContextUsageEstimate {
        tokens,
        usage_tokens: 0,
        trailing_tokens: tokens,
        last_usage_index: None,
    }
}

pub fn estimate_tokens(message: &AgentMessage) -> u64 {
    let chars = match message.role {
        MessageRole::Assistant if !message.content_blocks.is_empty() => message
            .content_blocks
            .iter()
            .map(|block| match block {
                ai::AssistantContentBlock::Text(text) => text.text.chars().count() as u64,
                ai::AssistantContentBlock::Thinking(thinking) => {
                    thinking.thinking.chars().count() as u64
                }
                ai::AssistantContentBlock::ToolCall(tool_call) => {
                    let args = serde_json::to_string(&tool_call.arguments)
                        .unwrap_or_else(|_| "{}".to_string());
                    tool_call.name.chars().count() as u64 + args.chars().count() as u64
                }
            })
            .sum(),
        _ => message.content.chars().count() as u64,
    };
    (chars + 3) / 4
}

fn assistant_usage(message: &AgentMessage) -> Option<&Usage> {
    if message.role == MessageRole::Assistant {
        message
            .usage
            .as_ref()
            .filter(|_| !matches!(message.stop_reason, Some(AssistantStopReason::Aborted)))
            .filter(|_| !matches!(message.stop_reason, Some(AssistantStopReason::Error)))
    } else {
        None
    }
}

pub fn collect_entries_for_branch_summary<S: SessionStorage>(
    session: &Session<S>,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> SessionResult<BranchSummaryEntries> {
    let Some(old_leaf_id) = old_leaf_id else {
        return Ok(BranchSummaryEntries {
            entries: Vec::new(),
            common_ancestor_id: None,
        });
    };
    let old_path = session
        .branch(Some(old_leaf_id))?
        .into_iter()
        .map(|entry| entry.id().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let target_path = session.branch(Some(target_id))?;
    let common_ancestor_id = target_path
        .iter()
        .rev()
        .find(|entry| old_path.contains(entry.id()))
        .map(|entry| entry.id().to_string());

    let mut entries = Vec::new();
    let mut current = Some(old_leaf_id.to_string());
    while let Some(current_id) = current {
        if common_ancestor_id.as_deref() == Some(current_id.as_str()) {
            break;
        }
        let entry = session
            .storage()
            .entry(&current_id)
            .cloned()
            .ok_or_else(|| {
                SessionError::new(
                    SessionErrorCode::InvalidSession,
                    format!("Entry {current_id} not found"),
                )
            })?;
        current = entry.parent_id().map(ToString::to_string);
        entries.push(entry);
    }
    entries.reverse();

    Ok(BranchSummaryEntries {
        entries,
        common_ancestor_id,
    })
}

pub fn prepare_branch_entries(
    entries: &[SessionTreeEntry],
    token_budget: u64,
) -> BranchPreparation {
    let mut messages = Vec::new();
    let mut file_ops = FileOperations::default();
    let mut total_tokens = 0;
    for entry in entries {
        if let SessionTreeEntry::BranchSummary {
            details,
            from_hook: false,
            ..
        } = entry
        {
            collect_file_ops_from_details(details.as_ref(), &mut file_ops);
        }
    }

    for entry in entries.iter().rev() {
        let Some(message) = message_from_branch_entry(entry) else {
            continue;
        };
        extract_file_ops_from_message(&message, &mut file_ops);
        let tokens = estimate_tokens(&message);
        if token_budget > 0 && total_tokens + tokens > token_budget {
            if matches!(
                entry,
                SessionTreeEntry::Compaction { .. } | SessionTreeEntry::BranchSummary { .. }
            ) && total_tokens < token_budget * 9 / 10
            {
                messages.insert(0, message);
                total_tokens += tokens;
            }
            break;
        }
        messages.insert(0, message);
        total_tokens += tokens;
    }

    BranchPreparation {
        messages,
        file_ops,
        total_tokens,
    }
}

pub fn compute_file_lists(file_ops: &FileOperations) -> (Vec<String>, Vec<String>) {
    let modified = file_ops
        .edited
        .union(&file_ops.written)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let read_files = file_ops
        .read
        .difference(&modified)
        .cloned()
        .collect::<Vec<_>>();
    let modified_files = modified.into_iter().collect::<Vec<_>>();
    (read_files, modified_files)
}

pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    let mut sections = Vec::new();
    if !read_files.is_empty() {
        sections.push(format!(
            "<read-files>\n{}\n</read-files>",
            read_files.join("\n")
        ));
    }
    if !modified_files.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            modified_files.join("\n")
        ));
    }
    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

pub fn serialize_conversation(messages: &[RichMessage]) -> String {
    let mut parts = Vec::new();
    for message in messages {
        match message {
            RichMessage::User(user) => {
                let content = user_content_text(&user.content);
                if !content.is_empty() {
                    parts.push(format!("[User]: {content}"));
                }
            }
            RichMessage::Assistant(assistant) => {
                let mut text_parts = Vec::new();
                let mut thinking_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for block in &assistant.content {
                    match block {
                        ai::AssistantContentBlock::Text(text) => text_parts.push(text.text.clone()),
                        ai::AssistantContentBlock::Thinking(thinking) => {
                            thinking_parts.push(thinking.thinking.clone());
                        }
                        ai::AssistantContentBlock::ToolCall(tool_call) => {
                            let args = tool_call
                                .arguments
                                .iter()
                                .map(|(key, value)| {
                                    let value = serde_json::to_string(value)
                                        .unwrap_or_else(|_| "[unserializable]".to_string());
                                    format!("{key}={value}")
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            tool_calls.push(format!("{}({})", tool_call.name, args));
                        }
                    }
                }
                if !thinking_parts.is_empty() {
                    parts.push(format!(
                        "[Assistant thinking]: {}",
                        thinking_parts.join("\n")
                    ));
                }
                if !text_parts.is_empty() {
                    parts.push(format!("[Assistant]: {}", text_parts.join("\n")));
                }
                if !tool_calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
                }
            }
            RichMessage::ToolResult(tool_result) => {
                let content = tool_result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        UserContentBlock::Text(text) => Some(text.text.as_str()),
                        UserContentBlock::Image(_) => None,
                    })
                    .collect::<String>();
                if !content.is_empty() {
                    parts.push(format!(
                        "[Tool result]: {}",
                        truncate_for_summary(&content, 2000)
                    ));
                }
            }
        }
    }
    parts.join("\n\n")
}

pub fn finalize_branch_summary(
    summary_text: impl AsRef<str>,
    file_ops: &FileOperations,
) -> BranchSummaryResult {
    let (read_files, modified_files) = compute_file_lists(file_ops);
    let mut summary = format!("{}{}", BRANCH_SUMMARY_PREAMBLE, summary_text.as_ref());
    summary.push_str(&format_file_operations(&read_files, &modified_files));
    if summary.is_empty() {
        summary = "No summary generated".to_string();
    }
    BranchSummaryResult {
        summary,
        read_files,
        modified_files,
    }
}

fn user_content_text(content: &UserMessageContent) -> String {
    match content {
        UserMessageContent::Text(text) => text.clone(),
        UserMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                UserContentBlock::Text(text) => Some(text.text.as_str()),
                UserContentBlock::Image(_) => None,
            })
            .collect(),
    }
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated_chars = text.chars().count() - max_chars;
    let prefix = text.chars().take(max_chars).collect::<String>();
    format!("{prefix}\n\n[... {truncated_chars} more characters truncated]")
}

fn collect_file_ops_from_details(
    details: Option<&serde_json::Value>,
    file_ops: &mut FileOperations,
) {
    let Some(details) = details.and_then(serde_json::Value::as_object) else {
        return;
    };
    if let Some(read_files) = details
        .get("readFiles")
        .and_then(serde_json::Value::as_array)
    {
        for path in read_files.iter().filter_map(serde_json::Value::as_str) {
            file_ops.read.insert(path.to_string());
        }
    }
    if let Some(modified_files) = details
        .get("modifiedFiles")
        .and_then(serde_json::Value::as_array)
    {
        for path in modified_files.iter().filter_map(serde_json::Value::as_str) {
            file_ops.edited.insert(path.to_string());
        }
    }
}

fn message_from_branch_entry(entry: &SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Message { message, .. } if message.role != MessageRole::Tool => {
            Some(message.clone())
        }
        SessionTreeEntry::CustomMessage {
            custom_type,
            content,
            details,
            ..
        } => Some(custom_agent_message(CustomMessage {
            custom_type: custom_type.clone(),
            content: content.clone(),
            display: true,
            details: details.clone(),
            timestamp: 0,
        })),
        SessionTreeEntry::BranchSummary {
            summary, from_id, ..
        } => Some(branch_summary_agent_message(BranchSummaryMessage {
            summary: summary.clone(),
            from_id: from_id.clone(),
            timestamp: 0,
        })),
        SessionTreeEntry::Compaction {
            summary,
            tokens_before,
            ..
        } => Some(compaction_summary_agent_message(CompactionSummaryMessage {
            summary: summary.clone(),
            tokens_before: *tokens_before,
            timestamp: 0,
        })),
        _ => None,
    }
}

fn extract_file_ops_from_message(message: &AgentMessage, file_ops: &mut FileOperations) {
    if message.role != MessageRole::Assistant {
        return;
    }
    for block in &message.content_blocks {
        let ai::AssistantContentBlock::ToolCall(tool_call) = block else {
            continue;
        };
        let Some(path) = tool_call
            .arguments
            .get("path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        match tool_call.name.as_str() {
            "read" => {
                file_ops.read.insert(path.to_string());
            }
            "write" => {
                file_ops.written.insert(path.to_string());
            }
            "edit" => {
                file_ops.edited.insert(path.to_string());
            }
            _ => {}
        }
    }
}

fn custom_agent_message(message: CustomMessage) -> AgentMessage {
    AgentMessage::new(MessageRole::User, message.content)
}

fn branch_summary_agent_message(message: BranchSummaryMessage) -> AgentMessage {
    AgentMessage::new(
        MessageRole::User,
        format!(
            "{}{}{}",
            BRANCH_SUMMARY_PREFIX, message.summary, BRANCH_SUMMARY_SUFFIX
        ),
    )
}

fn compaction_summary_agent_message(message: CompactionSummaryMessage) -> AgentMessage {
    AgentMessage::new(
        MessageRole::User,
        format!(
            "{}{}{}",
            COMPACTION_SUMMARY_PREFIX, message.summary, COMPACTION_SUMMARY_SUFFIX
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::session::{InMemorySessionStorage, Session};
    use ai::{
        AssistantContentBlock, RichAssistantMessage, RichMessage, TextContent, ThinkingContent,
        ToolCall, ToolResultMessage, Usage, UsageCost, UserContentBlock, UserMessage,
        UserMessageContent,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn message(role: MessageRole, content: &str) -> AgentMessage {
        AgentMessage {
            role,
            content: content.to_string(),
            content_blocks: Vec::new(),
            user_content_blocks: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            details: None,
            is_error: false,
            usage: None,
            stop_reason: None,
        }
    }

    fn assistant_with_usage(content: &str, usage: Usage) -> AgentMessage {
        AgentMessage {
            role: MessageRole::Assistant,
            content: content.to_string(),
            content_blocks: Vec::new(),
            user_content_blocks: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            details: None,
            is_error: false,
            usage: Some(usage),
            stop_reason: Some(AssistantStopReason::Stop),
        }
    }

    fn assistant_with_tool_call(name: &str, path: &str) -> AgentMessage {
        let mut message = message(MessageRole::Assistant, "");
        message.content_blocks = vec![AssistantContentBlock::ToolCall(ToolCall {
            id: format!("call-{name}"),
            name: name.to_string(),
            arguments: BTreeMap::from([("path".to_string(), json!(path))]),
            thought_signature: None,
        })];
        message
    }

    fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64, total: u64) -> Usage {
        Usage {
            input,
            output,
            cache_read,
            cache_write,
            total_tokens: total,
            cost: UsageCost::default(),
        }
    }

    #[test]
    fn calculates_context_tokens_like_pi_usage_fallback() {
        assert_eq!(calculate_context_tokens(&usage(3, 4, 5, 6, 20)), 20);
        assert_eq!(calculate_context_tokens(&usage(3, 4, 5, 6, 0)), 18);
    }

    #[test]
    fn should_compact_respects_enabled_and_reserved_window() {
        let settings = CompactionSettings {
            enabled: true,
            reserve_tokens: 100,
            keep_recent_tokens: 20,
        };
        assert!(!should_compact(899, 1000, &settings));
        assert!(should_compact(901, 1000, &settings));
        assert!(!should_compact(
            10_000,
            1000,
            &CompactionSettings {
                enabled: false,
                ..settings
            }
        ));
    }

    #[test]
    fn estimates_tokens_by_four_character_chunks() {
        assert_eq!(estimate_tokens(&message(MessageRole::User, "")), 0);
        assert_eq!(estimate_tokens(&message(MessageRole::User, "abcd")), 1);
        assert_eq!(estimate_tokens(&message(MessageRole::User, "abcde")), 2);
    }

    #[test]
    fn estimate_context_uses_last_successful_assistant_usage_plus_trailing_tokens() {
        let estimate = estimate_context_tokens(&[
            message(MessageRole::User, "ignored"),
            assistant_with_usage("assistant", usage(10, 5, 2, 1, 0)),
            message(MessageRole::User, "abcdefgh"),
        ]);

        assert_eq!(estimate.usage_tokens, 18);
        assert_eq!(estimate.trailing_tokens, 2);
        assert_eq!(estimate.tokens, 20);
        assert_eq!(estimate.last_usage_index, Some(1));
    }

    #[test]
    fn collects_entries_for_branch_summary_like_pi() {
        let storage = InMemorySessionStorage::default();
        let mut session = Session::new(storage);
        let root = session
            .append_message(message(MessageRole::User, "root"))
            .expect("root should append");
        let old_leaf = session
            .append_message(message(MessageRole::Assistant, "old answer"))
            .expect("old answer should append");
        session
            .move_to(Some(root.clone()), None)
            .expect("should move back to root");
        let target = session
            .append_message(message(MessageRole::User, "new branch"))
            .expect("target branch should append");

        let result = collect_entries_for_branch_summary(&session, Some(&old_leaf), &target)
            .expect("branch entries should collect");

        assert_eq!(result.common_ancestor_id.as_deref(), Some(root.as_str()));
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].id(), old_leaf);
    }

    #[test]
    fn collect_entries_for_branch_summary_returns_empty_without_old_leaf_like_pi() {
        let storage = InMemorySessionStorage::default();
        let session = Session::new(storage);

        let result = collect_entries_for_branch_summary(&session, None, "missing")
            .expect("empty old leaf should short-circuit");

        assert!(result.entries.is_empty());
        assert_eq!(result.common_ancestor_id, None);
    }

    #[test]
    fn prepare_branch_entries_collects_messages_file_ops_and_budget_like_pi() {
        let entries = vec![
            SessionTreeEntry::BranchSummary {
                id: "summary".to_string(),
                parent_id: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
                from_id: "root".to_string(),
                summary: "previous branch summary".to_string(),
                details: Some(json!({
                    "readFiles": ["old-read.md"],
                    "modifiedFiles": ["old-edit.md"]
                })),
                from_hook: false,
            },
            SessionTreeEntry::Message {
                id: "tool-result".to_string(),
                parent_id: Some("summary".to_string()),
                timestamp: "2026-01-01T00:00:01.000Z".to_string(),
                message: message(MessageRole::Tool, "tool output should not summarize"),
            },
            SessionTreeEntry::Message {
                id: "assistant".to_string(),
                parent_id: Some("tool-result".to_string()),
                timestamp: "2026-01-01T00:00:02.000Z".to_string(),
                message: assistant_with_tool_call("read", "new-read.md"),
            },
            SessionTreeEntry::Message {
                id: "user".to_string(),
                parent_id: Some("assistant".to_string()),
                timestamp: "2026-01-01T00:00:03.000Z".to_string(),
                message: message(MessageRole::User, "latest"),
            },
        ];

        let prepared = prepare_branch_entries(&entries, 2);

        assert_eq!(prepared.total_tokens, 2);
        assert_eq!(prepared.messages.len(), 1);
        assert_eq!(prepared.messages[0].content, "latest");
        assert!(prepared.file_ops.read.contains("old-read.md"));
        assert!(prepared.file_ops.read.contains("new-read.md"));
        assert!(prepared.file_ops.edited.contains("old-edit.md"));
        assert!(!prepared.messages.iter().any(|message| {
            message.role == MessageRole::Tool
                && message.content.contains("tool output should not summarize")
        }));
    }

    #[test]
    fn computes_and_formats_file_operations_like_pi() {
        let file_ops = FileOperations {
            read: BTreeMap::from([
                ("z-read.md".to_string(), ()),
                ("edited.md".to_string(), ()),
                ("a-read.md".to_string(), ()),
            ])
            .into_keys()
            .collect(),
            written: BTreeMap::from([("written.md".to_string(), ())])
                .into_keys()
                .collect(),
            edited: BTreeMap::from([("edited.md".to_string(), ())])
                .into_keys()
                .collect(),
        };

        let (read_files, modified_files) = compute_file_lists(&file_ops);
        let formatted = format_file_operations(&read_files, &modified_files);

        assert_eq!(read_files, vec!["a-read.md", "z-read.md"]);
        assert_eq!(modified_files, vec!["edited.md", "written.md"]);
        assert_eq!(
            formatted,
            "\n\n<read-files>\na-read.md\nz-read.md\n</read-files>\n\n<modified-files>\nedited.md\nwritten.md\n</modified-files>"
        );
    }

    #[test]
    fn serializes_rich_conversation_for_summary_like_pi() {
        let messages = vec![
            RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(TextContent {
                        text: "hello ".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Text(TextContent {
                        text: "world".to_string(),
                        text_signature: None,
                    }),
                    UserContentBlock::Image(ai::ImageContent {
                        data: "ignored".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            }),
            RichMessage::Assistant(RichAssistantMessage {
                content: vec![
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "plan".to_string(),
                        thinking_signature: None,
                        redacted: false,
                    }),
                    AssistantContentBlock::Text(TextContent {
                        text: "answer".to_string(),
                        text_signature: None,
                    }),
                    AssistantContentBlock::ToolCall(ToolCall {
                        id: "call-read".to_string(),
                        name: "read".to_string(),
                        arguments: BTreeMap::from([("path".to_string(), json!("README.md"))]),
                        thought_signature: None,
                    }),
                ],
                api: "api".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                response_model: None,
                response_id: None,
                usage: usage(1, 1, 0, 0, 2),
                stop_reason: AssistantStopReason::Stop,
                error_message: None,
                diagnostics: Vec::new(),
                timestamp_millis: 2,
            }),
            RichMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call-read".to_string(),
                tool_name: "read".to_string(),
                content: vec![UserContentBlock::Text(TextContent {
                    text: format!("{}tail", "x".repeat(2000)),
                    text_signature: None,
                })],
                details: None,
                is_error: false,
                timestamp_millis: 3,
            }),
        ];

        let serialized = serialize_conversation(&messages);

        assert!(serialized.contains("[User]: hello world"));
        assert!(serialized.contains("[Assistant thinking]: plan"));
        assert!(serialized.contains("[Assistant]: answer"));
        assert!(serialized.contains("[Assistant tool calls]: read(path=\"README.md\")"));
        assert!(serialized.contains("[Tool result]: "));
        assert!(serialized.contains("[... 4 more characters truncated]"));
    }

    #[test]
    fn finalizes_branch_summary_with_preamble_and_file_tags_like_pi() {
        let file_ops = FileOperations {
            read: BTreeMap::from([
                ("README.md".to_string(), ()),
                ("src/main.rs".to_string(), ()),
            ])
            .into_keys()
            .collect(),
            written: Default::default(),
            edited: BTreeMap::from([("src/main.rs".to_string(), ())])
                .into_keys()
                .collect(),
        };

        let result = finalize_branch_summary("Model summary", &file_ops);

        assert_eq!(result.read_files, vec!["README.md"]);
        assert_eq!(result.modified_files, vec!["src/main.rs"]);
        assert!(result.summary.starts_with(
            "The user explored a different conversation branch before returning here."
        ));
        assert!(result.summary.contains("Model summary"));
        assert!(result
            .summary
            .contains("<read-files>\nREADME.md\n</read-files>"));
        assert!(result
            .summary
            .contains("<modified-files>\nsrc/main.rs\n</modified-files>"));
    }
}
