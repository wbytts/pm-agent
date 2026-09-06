use agent::harness::{build_session_context, SessionTreeEntry};
use agent::AgentMessage;
use ai::MessageRole;
use serde::{Deserialize, Serialize};

use crate::compaction::utils::{
    compute_file_lists, create_file_ops, extract_file_ops_from_message, FileOperations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsageEstimate {
    pub tokens: u64,
    pub usage_tokens: u64,
    pub trailing_tokens: u64,
    pub last_usage_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutPointResult {
    pub first_kept_entry_index: usize,
    pub turn_start_index: Option<usize>,
    pub is_split_turn: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreparation {
    pub first_kept_entry_id: String,
    pub messages_to_summarize: Vec<AgentMessage>,
    pub turn_prefix_messages: Vec<AgentMessage>,
    pub is_split_turn: bool,
    pub tokens_before: u64,
    pub previous_summary: Option<String>,
    pub file_ops: FileOperations,
    pub settings: CompactionSettings,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

pub fn should_compact(
    context_tokens: u64,
    context_window: u64,
    settings: CompactionSettings,
) -> bool {
    settings.enabled && context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

pub fn estimate_context_tokens(messages: &[AgentMessage]) -> ContextUsageEstimate {
    let trailing_tokens = messages.iter().map(estimate_tokens).sum();
    ContextUsageEstimate {
        tokens: trailing_tokens,
        usage_tokens: 0,
        trailing_tokens,
        last_usage_index: None,
    }
}

pub fn estimate_tokens(message: &AgentMessage) -> u64 {
    let chars = message.content.chars().count() as u64;
    chars.div_ceil(4)
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
        if let SessionTreeEntry::Message { message, .. } = &entries[index] {
            accumulated_tokens += estimate_tokens(message);
            if accumulated_tokens >= keep_recent_tokens {
                cut_index = cut_points
                    .iter()
                    .copied()
                    .find(|cut_point| *cut_point >= index)
                    .unwrap_or(cut_index);
                break;
            }
        }
    }

    while cut_index > start_index {
        let previous = &entries[cut_index - 1];
        if matches!(
            previous,
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

pub fn prepare_compaction(
    path_entries: &[SessionTreeEntry],
    settings: CompactionSettings,
) -> Result<Option<CompactionPreparation>, String> {
    if matches!(
        path_entries.last(),
        Some(SessionTreeEntry::Compaction { .. })
    ) {
        return Ok(None);
    }

    let previous_compaction_index = path_entries
        .iter()
        .rposition(|entry| matches!(entry, SessionTreeEntry::Compaction { .. }));

    let mut previous_summary = None;
    let mut boundary_start = 0;
    if let Some(index) = previous_compaction_index {
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

    let context = build_session_context(path_entries).map_err(|error| error.to_string())?;
    let tokens_before = estimate_context_tokens(&context.messages).tokens;
    let boundary_end = path_entries.len();
    if boundary_start >= boundary_end {
        return Ok(None);
    }

    let cut_point = find_cut_point(
        path_entries,
        boundary_start,
        boundary_end,
        settings.keep_recent_tokens,
    );
    let Some(first_kept_entry) = path_entries.get(cut_point.first_kept_entry_index) else {
        return Ok(None);
    };
    let first_kept_entry_id = first_kept_entry.id().to_string();

    let history_end = if cut_point.is_split_turn {
        cut_point
            .turn_start_index
            .unwrap_or(cut_point.first_kept_entry_index)
    } else {
        cut_point.first_kept_entry_index
    };
    let messages_to_summarize =
        entry_messages_for_compaction(path_entries, boundary_start, history_end);
    let turn_prefix_messages = if cut_point.is_split_turn {
        entry_messages_for_compaction(
            path_entries,
            cut_point.turn_start_index.unwrap_or(history_end),
            cut_point.first_kept_entry_index,
        )
    } else {
        Vec::new()
    };

    let mut file_ops = extract_file_operations(
        &messages_to_summarize,
        path_entries,
        previous_compaction_index,
    );
    for message in &turn_prefix_messages {
        extract_file_ops_from_message(message, &mut file_ops);
    }
    let (read_files, modified_files) = compute_file_lists(&file_ops);

    Ok(Some(CompactionPreparation {
        first_kept_entry_id,
        messages_to_summarize,
        turn_prefix_messages,
        is_split_turn: cut_point.is_split_turn,
        tokens_before,
        previous_summary,
        file_ops,
        settings,
        read_files,
        modified_files,
    }))
}

fn find_valid_cut_points(
    entries: &[SessionTreeEntry],
    start_index: usize,
    end_index: usize,
) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .take(end_index)
        .skip(start_index)
        .filter_map(|(index, entry)| match entry {
            SessionTreeEntry::Message { message, .. }
                if matches!(message.role, MessageRole::User | MessageRole::Assistant) =>
            {
                Some(index)
            }
            SessionTreeEntry::BranchSummary { .. } | SessionTreeEntry::CustomMessage { .. } => {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

fn find_turn_start_index(
    entries: &[SessionTreeEntry],
    entry_index: usize,
    start_index: usize,
) -> Option<usize> {
    (start_index..=entry_index)
        .rev()
        .find(|index| match &entries[*index] {
            SessionTreeEntry::Message { message, .. } => message.role == MessageRole::User,
            SessionTreeEntry::BranchSummary { .. } | SessionTreeEntry::CustomMessage { .. } => true,
            _ => false,
        })
}

fn entry_messages_for_compaction(
    entries: &[SessionTreeEntry],
    start_index: usize,
    end_index: usize,
) -> Vec<AgentMessage> {
    entries
        .iter()
        .take(end_index)
        .skip(start_index)
        .filter_map(message_from_entry_for_compaction)
        .collect()
}

fn message_from_entry_for_compaction(entry: &SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Message { message, .. } => Some(message.clone()),
        SessionTreeEntry::CustomMessage {
            content, display, ..
        } if *display => Some(AgentMessage::new(MessageRole::User, content.clone())),
        SessionTreeEntry::BranchSummary { summary, .. } => {
            Some(AgentMessage::new(MessageRole::User, summary.clone()))
        }
        SessionTreeEntry::Compaction { .. } => None,
        _ => None,
    }
}

fn extract_file_operations(
    messages: &[AgentMessage],
    entries: &[SessionTreeEntry],
    previous_compaction_index: Option<usize>,
) -> FileOperations {
    let mut file_ops = create_file_ops();
    if let Some(index) = previous_compaction_index {
        if let SessionTreeEntry::Compaction {
            details, from_hook, ..
        } = &entries[index]
        {
            if !from_hook {
                merge_previous_file_details(details.as_ref(), &mut file_ops);
            }
        }
    }
    for message in messages {
        extract_file_ops_from_message(message, &mut file_ops);
    }
    file_ops
}

fn merge_previous_file_details(details: Option<&serde_json::Value>, file_ops: &mut FileOperations) {
    let Some(details) = details else {
        return;
    };
    if let Some(files) = details.get("readFiles").and_then(|value| value.as_array()) {
        for file in files.iter().filter_map(|value| value.as_str()) {
            file_ops.read.insert(file.to_string());
        }
    }
    if let Some(files) = details
        .get("modifiedFiles")
        .and_then(|value| value.as_array())
    {
        for file in files.iter().filter_map(|value| value.as_str()) {
            file_ops.edited.insert(file.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_when_context_exceeds_reserved_window() {
        let settings = CompactionSettings {
            enabled: true,
            reserve_tokens: 20,
            keep_recent_tokens: 10,
        };

        assert!(should_compact(90, 100, settings));
        assert!(!should_compact(70, 100, settings));
    }

    #[test]
    fn finds_split_turn_cut_point_at_assistant_message() {
        let entries = vec![
            message_entry("1", None, MessageRole::User, "u1"),
            message_entry("2", Some("1"), MessageRole::Assistant, "a1"),
            message_entry("3", Some("2"), MessageRole::User, "u2"),
            message_entry("4", Some("3"), MessageRole::Assistant, &"a2 ".repeat(200)),
        ];

        let cut = find_cut_point(&entries, 0, entries.len(), 10);

        assert_eq!(cut.first_kept_entry_index, 3);
        assert_eq!(cut.turn_start_index, Some(2));
        assert!(cut.is_split_turn);
    }

    #[test]
    fn prepares_compaction_with_previous_file_details() {
        let entries = vec![
            message_entry("1", None, MessageRole::User, "first"),
            SessionTreeEntry::Compaction {
                id: "2".to_string(),
                parent_id: Some("1".to_string()),
                timestamp: "now".to_string(),
                summary: "previous".to_string(),
                first_kept_entry_id: "3".to_string(),
                tokens_before: 100,
                details: Some(serde_json::json!({
                    "readFiles": ["old-read.txt"],
                    "modifiedFiles": ["old-edit.txt"]
                })),
                from_hook: false,
            },
            message_entry("3", Some("2"), MessageRole::User, "next"),
            message_entry("4", Some("3"), MessageRole::Assistant, "/read current.txt"),
            message_entry("5", Some("4"), MessageRole::User, &"recent ".repeat(80)),
        ];

        let preparation = prepare_compaction(
            &entries,
            CompactionSettings {
                enabled: true,
                reserve_tokens: 20,
                keep_recent_tokens: 10,
            },
        )
        .expect("preparation should succeed")
        .expect("preparation should exist");

        assert_eq!(preparation.previous_summary.as_deref(), Some("previous"));
        assert_eq!(preparation.first_kept_entry_id, "5");
        assert!(preparation.read_files.contains(&"old-read.txt".to_string()));
        assert!(preparation.read_files.contains(&"current.txt".to_string()));
        assert!(preparation
            .modified_files
            .contains(&"old-edit.txt".to_string()));
    }

    fn message_entry(
        id: &str,
        parent_id: Option<&str>,
        role: MessageRole,
        content: &str,
    ) -> SessionTreeEntry {
        SessionTreeEntry::Message {
            id: id.to_string(),
            parent_id: parent_id.map(ToString::to_string),
            timestamp: "now".to_string(),
            message: AgentMessage::new(role, content.to_string()),
        }
    }
}
