use ai::{
    AssistantStopReason, LanguageModelProvider, Message, MessageRole, Model, RichMessage,
    StreamRequest, Usage, UserContentBlock, UserMessageContent,
};
use thiserror::Error;

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
pub const BRANCH_SUMMARY_PROMPT: &str = "Create a structured summary of this conversation branch for context when returning later.\n\nUse this EXACT format:\n\n## Goal\n[What was the user trying to accomplish in this branch?]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Work that was started but not finished]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [What should happen next to continue this work]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";
pub const SUMMARIZATION_PROMPT: &str = "The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.\n\nUse this EXACT format:\n\n## Goal\n[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned by user]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Current work]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [Ordered list of what should happen next]\n\n## Critical Context\n- [Any data, examples, or references needed to continue]\n- [Or \"(none)\" if not applicable]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";
pub const UPDATE_SUMMARIZATION_PROMPT: &str = "The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.\n\nUpdate the existing structured summary with new information. RULES:\n- PRESERVE all existing information from the previous summary\n- ADD new progress, decisions, and context from the new messages\n- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed\n- UPDATE \"Next Steps\" based on what was accomplished\n- PRESERVE exact file paths, function names, and error messages\n- If something is no longer relevant, you may remove it\n\nUse this EXACT format:\n\n## Goal\n[Preserve existing goals, add new ones if the task expanded]\n\n## Constraints & Preferences\n- [Preserve existing, add new ones discovered]\n\n## Progress\n### Done\n- [x] [Include previously done items AND newly completed items]\n\n### In Progress\n- [ ] [Current work - update based on progress]\n\n### Blocked\n- [Current blockers - remove if resolved]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale] (preserve all previous, add new)\n\n## Next Steps\n1. [Update based on current state]\n\n## Critical Context\n- [Preserve important context, add new if needed]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";
pub const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = "This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.";

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

#[derive(Debug, Clone)]
pub struct CompactionPreparation {
    pub first_kept_entry_id: String,
    pub messages_to_summarize: Vec<AgentMessage>,
    pub turn_prefix_messages: Vec<AgentMessage>,
    pub is_split_turn: bool,
    pub tokens_before: u64,
    pub previous_summary: Option<String>,
    pub file_ops: FileOperations,
    pub settings: CompactionSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryResult {
    pub summary: String,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionDetails {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    pub details: CompactionDetails,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenerateBranchSummaryOptions {
    pub custom_instructions: Option<String>,
    pub replace_instructions: bool,
    pub reserve_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateSummaryOptions {
    pub custom_instructions: Option<String>,
    pub previous_summary: Option<String>,
    pub reserve_tokens: u64,
}

impl Default for GenerateSummaryOptions {
    fn default() -> Self {
        Self {
            custom_instructions: None,
            previous_summary: None,
            reserve_tokens: DEFAULT_COMPACTION_SETTINGS.reserve_tokens,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactOptions {
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutPointResult {
    pub first_kept_entry_index: usize,
    pub turn_start_index: Option<usize>,
    pub is_split_turn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSummaryErrorCode {
    Aborted,
    SummarizationFailed,
    InvalidSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionErrorCode {
    Aborted,
    SummarizationFailed,
    InvalidSession,
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct BranchSummaryError {
    pub code: BranchSummaryErrorCode,
    pub message: String,
}

impl BranchSummaryError {
    pub fn new(code: BranchSummaryErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct CompactionError {
    pub code: CompactionErrorCode,
    pub message: String,
}

impl CompactionError {
    pub fn new(code: CompactionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
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

pub fn prepare_compaction(
    path_entries: &[SessionTreeEntry],
    settings: CompactionSettings,
) -> Result<Option<CompactionPreparation>, BranchSummaryError> {
    if path_entries.is_empty()
        || matches!(
            path_entries.last(),
            Some(SessionTreeEntry::Compaction { .. })
        )
    {
        return Ok(None);
    }

    let prev_compaction_index = path_entries
        .iter()
        .rposition(|entry| matches!(entry, SessionTreeEntry::Compaction { .. }));

    let mut previous_summary = None;
    let mut boundary_start = 0;
    if let Some(index) = prev_compaction_index {
        if let SessionTreeEntry::Compaction {
            summary,
            first_kept_entry_id,
            ..
        } = &path_entries[index]
        {
            previous_summary = Some(summary.clone());
            boundary_start = path_entries
                .iter()
                .position(|entry| entry.id() == first_kept_entry_id)
                .unwrap_or(index + 1);
        }
    }
    let boundary_end = path_entries.len();
    let branch_context =
        crate::harness::session::build_session_context(path_entries).map_err(|error| {
            BranchSummaryError::new(BranchSummaryErrorCode::InvalidSession, error.message)
        })?;
    let tokens_before = estimate_context_tokens(&branch_context.messages).tokens;
    let cut_point = find_cut_point(
        path_entries,
        boundary_start,
        boundary_end,
        settings.keep_recent_tokens,
    );
    let first_kept_entry = path_entries
        .get(cut_point.first_kept_entry_index)
        .ok_or_else(|| {
            BranchSummaryError::new(
                BranchSummaryErrorCode::InvalidSession,
                "First kept entry has no UUID - session may need migration",
            )
        })?;
    let first_kept_entry_id = first_kept_entry.id().to_string();
    if first_kept_entry_id.is_empty() {
        return Err(BranchSummaryError::new(
            BranchSummaryErrorCode::InvalidSession,
            "First kept entry has no UUID - session may need migration",
        ));
    }

    let history_end = if cut_point.is_split_turn {
        cut_point
            .turn_start_index
            .unwrap_or(cut_point.first_kept_entry_index)
    } else {
        cut_point.first_kept_entry_index
    };
    let mut messages_to_summarize = Vec::new();
    for entry in &path_entries[boundary_start..history_end] {
        if let Some(message) = message_from_compaction_entry(entry) {
            messages_to_summarize.push(message);
        }
    }
    let mut turn_prefix_messages = Vec::new();
    if cut_point.is_split_turn {
        let turn_start = cut_point
            .turn_start_index
            .unwrap_or(cut_point.first_kept_entry_index);
        for entry in &path_entries[turn_start..cut_point.first_kept_entry_index] {
            if let Some(message) = message_from_compaction_entry(entry) {
                turn_prefix_messages.push(message);
            }
        }
    }

    let mut file_ops = FileOperations::default();
    if let Some(index) = prev_compaction_index {
        if let SessionTreeEntry::Compaction {
            details,
            from_hook: false,
            ..
        } = &path_entries[index]
        {
            collect_file_ops_from_details(details.as_ref(), &mut file_ops);
        }
    }
    for message in messages_to_summarize
        .iter()
        .chain(turn_prefix_messages.iter())
    {
        extract_file_ops_from_message(message, &mut file_ops);
    }

    Ok(Some(CompactionPreparation {
        first_kept_entry_id,
        messages_to_summarize,
        turn_prefix_messages,
        is_split_turn: cut_point.is_split_turn,
        tokens_before,
        previous_summary,
        file_ops,
        settings,
    }))
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

pub fn generate_branch_summary<P: LanguageModelProvider>(
    entries: &[SessionTreeEntry],
    provider: &P,
    model: Model,
    options: GenerateBranchSummaryOptions,
) -> Result<BranchSummaryResult, BranchSummaryError> {
    let context_window = model.context_window as u64;
    let reserve_tokens = options.reserve_tokens.unwrap_or(16_384);
    let token_budget = context_window.saturating_sub(reserve_tokens);
    let preparation = prepare_branch_entries(entries, token_budget);
    if preparation.messages.is_empty() {
        return Ok(BranchSummaryResult {
            summary: "No content to summarize".to_string(),
            read_files: Vec::new(),
            modified_files: Vec::new(),
        });
    }

    let rich_messages = agent_messages_to_summary_rich_messages(&preparation.messages);
    let conversation_text = serialize_conversation(&rich_messages);
    let instructions = branch_summary_instructions(&options);
    let prompt_text =
        format!("<conversation>\n{conversation_text}\n</conversation>\n\n{instructions}");
    let request = StreamRequest {
        model,
        messages: vec![Message {
            role: MessageRole::User,
            content: prompt_text,
        }],
        rich_messages: Vec::new(),
        tools: Vec::new(),
        metadata: Default::default(),
    };
    let response = provider.stream(request).map_err(|error| {
        BranchSummaryError::new(
            BranchSummaryErrorCode::SummarizationFailed,
            format!("Branch summary failed: {error}"),
        )
    })?;
    let message = ai::stream::provider_events_to_stream(response)
        .map_err(|error| {
            BranchSummaryError::new(
                BranchSummaryErrorCode::SummarizationFailed,
                format!("Branch summary failed: {error}"),
            )
        })?
        .into_result()
        .ok_or_else(|| {
            BranchSummaryError::new(
                BranchSummaryErrorCode::SummarizationFailed,
                "Branch summary failed: stream ended without final result",
            )
        })?;
    match message.stop_reason {
        AssistantStopReason::Aborted => {
            return Err(BranchSummaryError::new(
                BranchSummaryErrorCode::Aborted,
                message
                    .error_message
                    .unwrap_or_else(|| "Branch summary aborted".to_string()),
            ));
        }
        AssistantStopReason::Error => {
            return Err(BranchSummaryError::new(
                BranchSummaryErrorCode::SummarizationFailed,
                format!(
                    "Branch summary failed: {}",
                    message
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string())
                ),
            ));
        }
        _ => {}
    }

    let summary_text = if !message.content.is_empty() {
        message.content
    } else {
        message
            .content_blocks
            .iter()
            .filter_map(|block| match block {
                ai::AssistantContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(finalize_branch_summary(summary_text, &preparation.file_ops))
}

pub fn generate_summary<P: LanguageModelProvider>(
    current_messages: &[AgentMessage],
    provider: &P,
    model: Model,
    options: GenerateSummaryOptions,
) -> Result<String, CompactionError> {
    let rich_messages = agent_messages_to_summary_rich_messages(current_messages);
    let conversation_text = serialize_conversation(&rich_messages);
    let mut prompt_text = format!("<conversation>\n{conversation_text}\n</conversation>\n\n");
    if let Some(previous_summary) = options.previous_summary.as_deref() {
        prompt_text.push_str(&format!(
            "<previous-summary>\n{previous_summary}\n</previous-summary>\n\n"
        ));
    }
    prompt_text.push_str(&summary_instructions(&options));

    let max_tokens = summary_max_tokens(&model, options.reserve_tokens, 0.8);
    let response = stream_summary(
        provider,
        StreamRequest {
            model: model_with_max_tokens(model, max_tokens),
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                },
                Message {
                    role: MessageRole::User,
                    content: prompt_text,
                },
            ],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        },
    )?;
    map_summary_message(response, "Summarization")
}

pub fn generate_turn_prefix_summary<P: LanguageModelProvider>(
    messages: &[AgentMessage],
    provider: &P,
    model: Model,
    reserve_tokens: u64,
) -> Result<String, CompactionError> {
    let rich_messages = agent_messages_to_summary_rich_messages(messages);
    let conversation_text = serialize_conversation(&rich_messages);
    let prompt_text =
        format!("<conversation>\n{conversation_text}\n</conversation>\n\n{TURN_PREFIX_SUMMARIZATION_PROMPT}");
    let max_tokens = summary_max_tokens(&model, reserve_tokens, 0.5);
    let response = stream_summary(
        provider,
        StreamRequest {
            model: model_with_max_tokens(model, max_tokens),
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                },
                Message {
                    role: MessageRole::User,
                    content: prompt_text,
                },
            ],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        },
    )?;
    map_summary_message(response, "Turn prefix summarization")
}

pub fn compact<P: LanguageModelProvider>(
    preparation: CompactionPreparation,
    provider: &P,
    model: Model,
    options: CompactOptions,
) -> Result<CompactionResult, CompactionError> {
    if preparation.first_kept_entry_id.is_empty() {
        return Err(CompactionError::new(
            CompactionErrorCode::InvalidSession,
            "First kept entry has no UUID - session may need migration",
        ));
    }

    let mut summary = if preparation.is_split_turn && !preparation.turn_prefix_messages.is_empty() {
        let history_summary = if preparation.messages_to_summarize.is_empty() {
            "No prior history.".to_string()
        } else {
            generate_summary(
                &preparation.messages_to_summarize,
                provider,
                model.clone(),
                GenerateSummaryOptions {
                    custom_instructions: options.custom_instructions.clone(),
                    previous_summary: preparation.previous_summary.clone(),
                    reserve_tokens: preparation.settings.reserve_tokens,
                },
            )?
        };
        let turn_prefix_summary = generate_turn_prefix_summary(
            &preparation.turn_prefix_messages,
            provider,
            model,
            preparation.settings.reserve_tokens,
        )?;
        format!(
            "{history_summary}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix_summary}"
        )
    } else {
        generate_summary(
            &preparation.messages_to_summarize,
            provider,
            model,
            GenerateSummaryOptions {
                custom_instructions: options.custom_instructions,
                previous_summary: preparation.previous_summary,
                reserve_tokens: preparation.settings.reserve_tokens,
            },
        )?
    };

    let (read_files, modified_files) = compute_file_lists(&preparation.file_ops);
    summary.push_str(&format_file_operations(&read_files, &modified_files));

    Ok(CompactionResult {
        summary,
        first_kept_entry_id: preparation.first_kept_entry_id,
        tokens_before: preparation.tokens_before,
        details: CompactionDetails {
            read_files,
            modified_files,
        },
    })
}

pub fn find_cut_point(
    entries: &[SessionTreeEntry],
    start_index: usize,
    end_index: usize,
    keep_recent_tokens: u64,
) -> CutPointResult {
    let cut_points = find_valid_cut_points(entries, start_index, end_index);
    if cut_points.is_empty() {
        return CutPointResult {
            first_kept_entry_index: start_index,
            turn_start_index: None,
            is_split_turn: false,
        };
    }

    let mut accumulated_tokens = 0;
    let mut cut_index = cut_points[0];
    for index in (start_index..end_index).rev() {
        let entry = &entries[index];
        let SessionTreeEntry::Message { message, .. } = entry else {
            continue;
        };
        accumulated_tokens += estimate_tokens(message);
        if accumulated_tokens >= keep_recent_tokens {
            if let Some(next_cut) = cut_points.iter().find(|cut| **cut >= index) {
                cut_index = *next_cut;
            }
            break;
        }
    }

    while cut_index > start_index {
        let prev_entry = &entries[cut_index - 1];
        if matches!(
            prev_entry,
            SessionTreeEntry::Compaction { .. } | SessionTreeEntry::Message { .. }
        ) {
            break;
        }
        cut_index -= 1;
    }

    let is_user_message = matches!(
        entries.get(cut_index),
        Some(SessionTreeEntry::Message { message, .. }) if message.role == MessageRole::User
    );
    let turn_start_index = if is_user_message {
        None
    } else {
        find_turn_start_index(entries, cut_index, start_index)
    };
    CutPointResult {
        first_kept_entry_index: cut_index,
        turn_start_index,
        is_split_turn: !is_user_message && turn_start_index.is_some(),
    }
}

fn find_valid_cut_points(
    entries: &[SessionTreeEntry],
    start_index: usize,
    end_index: usize,
) -> Vec<usize> {
    let mut cut_points = Vec::new();
    for index in start_index..end_index {
        let entry = &entries[index];
        match entry {
            SessionTreeEntry::Message { message, .. } => match message.role {
                MessageRole::User | MessageRole::Assistant => cut_points.push(index),
                MessageRole::System | MessageRole::Tool => {}
            },
            SessionTreeEntry::BranchSummary { .. } | SessionTreeEntry::CustomMessage { .. } => {
                cut_points.push(index);
            }
            _ => {}
        }
    }
    cut_points
}

pub fn find_turn_start_index(
    entries: &[SessionTreeEntry],
    entry_index: usize,
    start_index: usize,
) -> Option<usize> {
    for index in (start_index..=entry_index).rev() {
        match &entries[index] {
            SessionTreeEntry::BranchSummary { .. } | SessionTreeEntry::CustomMessage { .. } => {
                return Some(index);
            }
            SessionTreeEntry::Message { message, .. } if message.role == MessageRole::User => {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn branch_summary_instructions(options: &GenerateBranchSummaryOptions) -> String {
    match (
        options.replace_instructions,
        options.custom_instructions.as_deref(),
    ) {
        (true, Some(custom)) => custom.to_string(),
        (false, Some(custom)) => format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom}"),
        _ => BRANCH_SUMMARY_PROMPT.to_string(),
    }
}

fn summary_instructions(options: &GenerateSummaryOptions) -> String {
    let base_prompt = if options.previous_summary.is_some() {
        UPDATE_SUMMARIZATION_PROMPT
    } else {
        SUMMARIZATION_PROMPT
    };
    match options.custom_instructions.as_deref() {
        Some(custom_instructions) => {
            format!("{base_prompt}\n\nAdditional focus: {custom_instructions}")
        }
        None => base_prompt.to_string(),
    }
}

fn summary_max_tokens(model: &Model, reserve_tokens: u64, ratio: f64) -> usize {
    let reserve_limit = ((reserve_tokens as f64) * ratio).floor() as usize;
    match model.max_tokens {
        Some(model_limit) => reserve_limit.min(model_limit),
        None => reserve_limit,
    }
}

fn model_with_max_tokens(mut model: Model, max_tokens: usize) -> Model {
    model.max_tokens = Some(max_tokens);
    model
}

fn stream_summary<P: LanguageModelProvider>(
    provider: &P,
    request: StreamRequest,
) -> Result<ai::AssistantMessage, CompactionError> {
    let response = provider.stream(request).map_err(|error| {
        CompactionError::new(
            CompactionErrorCode::SummarizationFailed,
            format!("Summarization failed: {error}"),
        )
    })?;
    ai::stream::provider_events_to_stream(response)
        .map_err(|error| {
            CompactionError::new(
                CompactionErrorCode::SummarizationFailed,
                format!("Summarization failed: {error}"),
            )
        })?
        .into_result()
        .ok_or_else(|| {
            CompactionError::new(
                CompactionErrorCode::SummarizationFailed,
                "Summarization failed: stream ended without final result",
            )
        })
}

fn map_summary_message(
    message: ai::AssistantMessage,
    operation: &str,
) -> Result<String, CompactionError> {
    match message.stop_reason {
        AssistantStopReason::Aborted => {
            return Err(CompactionError::new(
                CompactionErrorCode::Aborted,
                message
                    .error_message
                    .unwrap_or_else(|| format!("{operation} aborted")),
            ));
        }
        AssistantStopReason::Error => {
            return Err(CompactionError::new(
                CompactionErrorCode::SummarizationFailed,
                format!(
                    "{operation} failed: {}",
                    message
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string())
                ),
            ));
        }
        _ => {}
    }

    if !message.content.is_empty() {
        return Ok(message.content);
    }
    Ok(message
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            ai::AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn agent_messages_to_summary_rich_messages(messages: &[AgentMessage]) -> Vec<RichMessage> {
    messages
        .iter()
        .filter_map(|message| match message.role {
            MessageRole::User => Some(RichMessage::User(ai::UserMessage {
                content: if message.user_content_blocks.is_empty() {
                    ai::UserMessageContent::Text(message.content.clone())
                } else {
                    ai::UserMessageContent::Blocks(message.user_content_blocks.clone())
                },
                timestamp_millis: 0,
            })),
            MessageRole::Assistant => Some(RichMessage::Assistant(ai::RichAssistantMessage {
                content: if message.content_blocks.is_empty() {
                    vec![ai::AssistantContentBlock::Text(ai::TextContent {
                        text: message.content.clone(),
                        text_signature: None,
                    })]
                } else {
                    message.content_blocks.clone()
                },
                api: String::new(),
                provider: String::new(),
                model: String::new(),
                response_model: None,
                response_id: None,
                usage: message.usage.clone().unwrap_or_default(),
                stop_reason: message
                    .stop_reason
                    .clone()
                    .unwrap_or(AssistantStopReason::Stop),
                error_message: None,
                diagnostics: Vec::new(),
                timestamp_millis: 0,
            })),
            MessageRole::Tool => Some(RichMessage::ToolResult(ai::ToolResultMessage {
                tool_call_id: message.tool_call_id.clone().unwrap_or_default(),
                tool_name: message.tool_name.clone().unwrap_or_default(),
                content: if message.user_content_blocks.is_empty() {
                    vec![ai::UserContentBlock::Text(ai::TextContent {
                        text: message.content.clone(),
                        text_signature: None,
                    })]
                } else {
                    message.user_content_blocks.clone()
                },
                details: message.details.clone(),
                is_error: message.is_error,
                timestamp_millis: 0,
            })),
            MessageRole::System => None,
        })
        .collect()
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

fn message_from_compaction_entry(entry: &SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Compaction { .. } => None,
        _ => message_from_branch_entry(entry),
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
        AiResult, AssistantContentBlock, LanguageModelProvider, Model, RichAssistantMessage,
        RichMessage, StreamEvent, StreamRequest, TextContent, ThinkingContent, ToolCall,
        ToolResultMessage, Usage, UsageCost, UserContentBlock, UserMessage, UserMessageContent,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

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

    #[derive(Debug, Clone)]
    struct BranchSummaryProvider {
        requests: Arc<Mutex<Vec<StreamRequest>>>,
        responses: Arc<Mutex<Vec<Vec<StreamEvent>>>>,
    }

    impl BranchSummaryProvider {
        fn new(events: Vec<StreamEvent>) -> Self {
            Self::new_sequence(vec![events])
        }

        fn new_sequence(responses: Vec<Vec<StreamEvent>>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses)),
            }
        }
    }

    impl LanguageModelProvider for BranchSummaryProvider {
        fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            self.requests.lock().expect("requests lock").push(request);
            let mut responses = self.responses.lock().expect("responses lock");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                Ok(responses.first().cloned().unwrap_or_default())
            }
        }
    }

    fn summary_model(context_window: usize) -> Model {
        Model {
            id: "summary-model".to_string(),
            provider: "test".to_string(),
            api: "test".to_string(),
            display_name: "Summary Model".to_string(),
            context_window,
            ..Model::default()
        }
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

    #[test]
    fn generate_branch_summary_calls_provider_and_finalizes_like_pi() {
        let provider = BranchSummaryProvider::new(vec![StreamEvent::Finished {
            message: ai::Message {
                role: MessageRole::Assistant,
                content: "Generated summary".to_string(),
            },
        }]);
        let entries = vec![SessionTreeEntry::Message {
            id: "user".to_string(),
            parent_id: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            message: message(MessageRole::User, "summarize this branch"),
        }];

        let result = generate_branch_summary(
            &entries,
            &provider,
            summary_model(32_000),
            GenerateBranchSummaryOptions {
                custom_instructions: Some("focus on files".to_string()),
                replace_instructions: false,
                reserve_tokens: Some(16_384),
            },
        )
        .expect("summary should generate");

        assert!(result.summary.contains("Generated summary"));
        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.len(), 1);
        assert!(requests[0].messages[0]
            .content
            .contains("<conversation>\n[User]: summarize this branch\n</conversation>"));
        assert!(requests[0].messages[0]
            .content
            .contains("Additional focus: focus on files"));
    }

    #[test]
    fn generate_branch_summary_maps_provider_error_like_pi() {
        let provider = BranchSummaryProvider::new(vec![StreamEvent::Error {
            message: "provider failed".to_string(),
        }]);
        let entries = vec![SessionTreeEntry::Message {
            id: "user".to_string(),
            parent_id: None,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            message: message(MessageRole::User, "summarize this branch"),
        }];

        let error = generate_branch_summary(
            &entries,
            &provider,
            summary_model(32_000),
            GenerateBranchSummaryOptions::default(),
        )
        .expect_err("provider error should map");

        assert_eq!(error.code, BranchSummaryErrorCode::SummarizationFailed);
        assert!(error.message.contains("provider failed"));
    }

    #[test]
    fn generate_summary_includes_previous_summary_and_custom_focus_like_pi() {
        let provider = BranchSummaryProvider::new(vec![StreamEvent::Finished {
            message: ai::Message {
                role: MessageRole::Assistant,
                content: "Updated compaction summary".to_string(),
            },
        }]);
        let messages = vec![message(MessageRole::User, "new progress")];

        let summary = generate_summary(
            &messages,
            &provider,
            summary_model(32_000),
            GenerateSummaryOptions {
                custom_instructions: Some("focus on migration gaps".to_string()),
                previous_summary: Some("old compacted context".to_string()),
                reserve_tokens: 10_000,
            },
        )
        .expect("summary should generate");

        assert_eq!(summary, "Updated compaction summary");
        let requests = provider.requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.len(), 2);
        assert_eq!(requests[0].messages[0].role, MessageRole::System);
        assert_eq!(requests[0].messages[0].content, SUMMARIZATION_SYSTEM_PROMPT);
        assert!(requests[0].messages[1]
            .content
            .contains("<conversation>\n[User]: new progress\n</conversation>"));
        assert!(requests[0].messages[1]
            .content
            .contains("<previous-summary>\nold compacted context\n</previous-summary>"));
        assert!(requests[0].messages[1]
            .content
            .contains("Additional focus: focus on migration gaps"));
        assert_eq!(requests[0].model.max_tokens, Some(8_000));
    }

    #[test]
    fn compact_combines_split_turn_summary_and_file_tags_like_pi() {
        let provider = BranchSummaryProvider::new_sequence(vec![
            vec![StreamEvent::Finished {
                message: ai::Message {
                    role: MessageRole::Assistant,
                    content: "History summary".to_string(),
                },
            }],
            vec![StreamEvent::Finished {
                message: ai::Message {
                    role: MessageRole::Assistant,
                    content: "Turn prefix summary".to_string(),
                },
            }],
        ]);
        let preparation = CompactionPreparation {
            first_kept_entry_id: "kept-entry".to_string(),
            messages_to_summarize: vec![message(MessageRole::User, "old work")],
            turn_prefix_messages: vec![message(MessageRole::User, "split turn request")],
            is_split_turn: true,
            tokens_before: 12_345,
            previous_summary: None,
            file_ops: FileOperations {
                read: BTreeMap::from([("read.md".to_string(), ())])
                    .into_keys()
                    .collect(),
                written: BTreeMap::from([("write.md".to_string(), ())])
                    .into_keys()
                    .collect(),
                edited: BTreeMap::from([("edit.md".to_string(), ())])
                    .into_keys()
                    .collect(),
            },
            settings: CompactionSettings {
                enabled: true,
                reserve_tokens: 10_000,
                keep_recent_tokens: 2_000,
            },
        };

        let result = compact(
            preparation,
            &provider,
            summary_model(32_000),
            CompactOptions::default(),
        )
        .expect("compaction should generate");

        assert_eq!(result.first_kept_entry_id, "kept-entry");
        assert_eq!(result.tokens_before, 12_345);
        assert_eq!(result.details.read_files, vec!["read.md"]);
        assert_eq!(result.details.modified_files, vec!["edit.md", "write.md"]);
        assert!(result.summary.contains(
            "History summary\n\n---\n\n**Turn Context (split turn):**\n\nTurn prefix summary"
        ));
        assert!(result
            .summary
            .contains("<read-files>\nread.md\n</read-files>"));
        assert!(result
            .summary
            .contains("<modified-files>\nedit.md\nwrite.md\n</modified-files>"));
    }

    #[test]
    fn find_cut_point_marks_split_turn_like_pi() {
        let entries = vec![
            SessionTreeEntry::Message {
                id: "old-user".to_string(),
                parent_id: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
                message: message(MessageRole::User, "old request"),
            },
            SessionTreeEntry::Message {
                id: "old-assistant".to_string(),
                parent_id: Some("old-user".to_string()),
                timestamp: "2026-01-01T00:00:01.000Z".to_string(),
                message: message(MessageRole::Assistant, "old answer"),
            },
            SessionTreeEntry::Message {
                id: "new-user".to_string(),
                parent_id: Some("old-assistant".to_string()),
                timestamp: "2026-01-01T00:00:02.000Z".to_string(),
                message: message(MessageRole::User, "new request"),
            },
            SessionTreeEntry::Message {
                id: "new-assistant".to_string(),
                parent_id: Some("new-user".to_string()),
                timestamp: "2026-01-01T00:00:03.000Z".to_string(),
                message: message(MessageRole::Assistant, "long assistant suffix"),
            },
        ];

        let cut = find_cut_point(&entries, 0, entries.len(), 2);

        assert_eq!(cut.first_kept_entry_index, 3);
        assert_eq!(cut.turn_start_index, Some(2));
        assert!(cut.is_split_turn);
    }

    #[test]
    fn prepare_compaction_uses_previous_summary_and_split_turn_like_pi() {
        let entries = vec![
            SessionTreeEntry::Compaction {
                id: "compact-1".to_string(),
                parent_id: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
                summary: "previous summary".to_string(),
                first_kept_entry_id: "old-user".to_string(),
                tokens_before: 100,
                details: Some(json!({
                    "readFiles": ["previous-read.md"],
                    "modifiedFiles": ["previous-edit.md"]
                })),
                from_hook: false,
            },
            SessionTreeEntry::Message {
                id: "old-user".to_string(),
                parent_id: Some("compact-1".to_string()),
                timestamp: "2026-01-01T00:00:01.000Z".to_string(),
                message: message(MessageRole::User, "old request"),
            },
            SessionTreeEntry::Message {
                id: "old-assistant".to_string(),
                parent_id: Some("old-user".to_string()),
                timestamp: "2026-01-01T00:00:02.000Z".to_string(),
                message: assistant_with_tool_call("read", "new-read.md"),
            },
            SessionTreeEntry::Message {
                id: "new-user".to_string(),
                parent_id: Some("old-assistant".to_string()),
                timestamp: "2026-01-01T00:00:03.000Z".to_string(),
                message: message(MessageRole::User, "new request"),
            },
            SessionTreeEntry::Message {
                id: "new-assistant".to_string(),
                parent_id: Some("new-user".to_string()),
                timestamp: "2026-01-01T00:00:04.000Z".to_string(),
                message: message(MessageRole::Assistant, "long assistant suffix"),
            },
        ];

        let preparation = prepare_compaction(
            &entries,
            CompactionSettings {
                enabled: true,
                reserve_tokens: 10,
                keep_recent_tokens: 2,
            },
        )
        .expect("preparation should succeed")
        .expect("compaction should be applicable");

        assert_eq!(preparation.first_kept_entry_id, "new-assistant");
        assert_eq!(
            preparation.previous_summary.as_deref(),
            Some("previous summary")
        );
        assert!(preparation.is_split_turn);
        assert_eq!(preparation.messages_to_summarize.len(), 2);
        assert_eq!(preparation.turn_prefix_messages.len(), 1);
        assert!(preparation.file_ops.read.contains("previous-read.md"));
        assert!(preparation.file_ops.read.contains("new-read.md"));
        assert!(preparation.file_ops.edited.contains("previous-edit.md"));
    }
}
