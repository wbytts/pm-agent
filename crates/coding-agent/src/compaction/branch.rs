use agent::harness::{SessionStorage, SessionTreeEntry};
use agent::AgentMessage;
use ai::MessageRole;

use crate::compaction::planner::estimate_tokens;
use crate::compaction::utils::{
    compute_file_lists, create_file_ops, extract_file_ops_from_message, format_file_operations,
    serialize_conversation_for_summary, FileOperations,
};

pub const BRANCH_SUMMARY_PREAMBLE: &str = "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

pub const BRANCH_SUMMARY_PROMPT: &str = "Create a structured summary of this conversation branch for context when returning later.\n\nUse this EXACT format:\n\n## Goal\n[What was the user trying to accomplish in this branch?]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Work that was started but not finished]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [What should happen next to continue this work]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryResult {
    pub summary: Option<String>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub aborted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BranchPreparation {
    pub messages: Vec<AgentMessage>,
    pub file_ops: FileOperations,
    pub total_tokens: u64,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CollectEntriesResult {
    pub entries: Vec<SessionTreeEntry>,
    pub common_ancestor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryInstructions {
    pub custom_instructions: Option<String>,
    pub replace_instructions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BranchSummaryDraft {
    pub prompt: String,
    pub messages: Vec<AgentMessage>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

pub fn collect_entries_for_branch_summary<S: SessionStorage>(
    session: &S,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> Result<CollectEntriesResult, String> {
    let Some(old_leaf_id) = old_leaf_id else {
        return Ok(CollectEntriesResult {
            entries: Vec::new(),
            common_ancestor_id: None,
        });
    };

    let old_path = session
        .path_to_root(Some(old_leaf_id))
        .map_err(|error| error.to_string())?;
    let target_path = session
        .path_to_root(Some(target_id))
        .map_err(|error| error.to_string())?;
    let old_ids = old_path
        .iter()
        .map(|entry| entry.id().to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let common_ancestor_id = target_path
        .iter()
        .rev()
        .find(|entry| old_ids.contains(entry.id()))
        .map(|entry| entry.id().to_string());

    let mut entries = Vec::new();
    let mut current = Some(old_leaf_id.to_string());
    while let Some(current_id) = current {
        if common_ancestor_id.as_deref() == Some(current_id.as_str()) {
            break;
        }
        let Some(entry) = session.entry(&current_id).cloned() else {
            break;
        };
        current = entry.parent_id().map(ToString::to_string);
        entries.push(entry);
    }
    entries.reverse();

    Ok(CollectEntriesResult {
        entries,
        common_ancestor_id,
    })
}

pub fn prepare_branch_entries(
    entries: &[SessionTreeEntry],
    token_budget: u64,
) -> BranchPreparation {
    let mut file_ops = create_file_ops();
    for entry in entries {
        if let SessionTreeEntry::BranchSummary {
            details, from_hook, ..
        } = entry
        {
            if !from_hook {
                merge_summary_file_details(details.as_ref(), &mut file_ops);
            }
        }
    }

    let mut messages = Vec::new();
    let mut total_tokens = 0;
    for entry in entries.iter().rev() {
        let Some(message) = message_from_entry(entry) else {
            continue;
        };
        extract_file_ops_from_message(&message, &mut file_ops);
        let tokens = estimate_tokens(&message);
        if token_budget > 0 && total_tokens + tokens > token_budget {
            if matches!(
                entry,
                SessionTreeEntry::Compaction { .. } | SessionTreeEntry::BranchSummary { .. }
            ) && total_tokens < token_budget.saturating_mul(9) / 10
            {
                messages.insert(0, message);
                total_tokens += tokens;
            }
            break;
        }
        messages.insert(0, message);
        total_tokens += tokens;
    }

    let (read_files, modified_files) = compute_file_lists(&file_ops);
    BranchPreparation {
        messages,
        file_ops,
        total_tokens,
        read_files,
        modified_files,
    }
}

pub fn build_branch_summary_prompt(
    entries: &[SessionTreeEntry],
    token_budget: u64,
    instructions: BranchSummaryInstructions,
) -> BranchSummaryDraft {
    let preparation = prepare_branch_entries(entries, token_budget);
    let conversation = serialize_conversation_for_summary(&preparation.messages);
    let prompt_instructions = match (
        instructions.custom_instructions.as_deref(),
        instructions.replace_instructions,
    ) {
        (Some(custom), true) => custom.to_string(),
        (Some(custom), false) => format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom}"),
        (None, _) => BRANCH_SUMMARY_PROMPT.to_string(),
    };
    let prompt =
        format!("<conversation>\n{conversation}\n</conversation>\n\n{prompt_instructions}");

    BranchSummaryDraft {
        prompt,
        messages: preparation.messages,
        read_files: preparation.read_files,
        modified_files: preparation.modified_files,
    }
}

pub fn finalize_branch_summary(summary: &str, file_ops: &FileOperations) -> BranchSummaryResult {
    let (read_files, modified_files) = compute_file_lists(file_ops);
    let mut summary = format!("{BRANCH_SUMMARY_PREAMBLE}{summary}");
    summary.push_str(&format_file_operations(&read_files, &modified_files));
    BranchSummaryResult {
        summary: Some(summary),
        read_files,
        modified_files,
        aborted: false,
        error: None,
    }
}

fn message_from_entry(entry: &SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Message { message, .. } if message.role != MessageRole::Tool => {
            Some(message.clone())
        }
        SessionTreeEntry::CustomMessage {
            content, display, ..
        } if *display => Some(AgentMessage::new(MessageRole::User, content.clone())),
        SessionTreeEntry::BranchSummary {
            summary, from_id, ..
        } => Some(AgentMessage::new(
            MessageRole::User,
            format!("Branch summary from {from_id}:\n\n{summary}"),
        )),
        SessionTreeEntry::Compaction {
            summary,
            tokens_before,
            ..
        } => Some(AgentMessage::new(
            MessageRole::User,
            format!("Compaction summary ({tokens_before} tokens before):\n\n{summary}"),
        )),
        _ => None,
    }
}

fn merge_summary_file_details(details: Option<&serde_json::Value>, file_ops: &mut FileOperations) {
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
    use agent::harness::InMemorySessionStorage;

    #[test]
    fn collects_entries_between_old_leaf_and_target_branch() {
        let storage = InMemorySessionStorage::new(
            vec![
                message_entry("root", None, MessageRole::User, "root"),
                message_entry("left", Some("root"), MessageRole::Assistant, "left"),
                message_entry("right", Some("root"), MessageRole::Assistant, "right"),
                message_entry("left2", Some("left"), MessageRole::User, "left2"),
            ],
            None,
        )
        .expect("storage should build");

        let result = collect_entries_for_branch_summary(&storage, Some("left2"), "right")
            .expect("collection should work");

        assert_eq!(result.common_ancestor_id.as_deref(), Some("root"));
        assert_eq!(
            result
                .entries
                .iter()
                .map(SessionTreeEntry::id)
                .collect::<Vec<_>>(),
            vec!["left", "left2"]
        );
    }

    #[test]
    fn prepares_recent_branch_messages_and_keeps_file_tracking() {
        let entries = vec![
            SessionTreeEntry::BranchSummary {
                id: "summary".to_string(),
                parent_id: None,
                timestamp: "now".to_string(),
                from_id: "old".to_string(),
                summary: "old branch".to_string(),
                details: Some(serde_json::json!({
                    "readFiles": ["read-old.txt"],
                    "modifiedFiles": ["edit-old.txt"]
                })),
                from_hook: false,
            },
            message_entry(
                "assistant",
                Some("summary"),
                MessageRole::Assistant,
                "/read now.txt",
            ),
        ];

        let preparation = prepare_branch_entries(&entries, 1000);

        assert_eq!(preparation.messages.len(), 2);
        assert!(preparation.read_files.contains(&"read-old.txt".to_string()));
        assert!(preparation.read_files.contains(&"now.txt".to_string()));
        assert!(preparation
            .modified_files
            .contains(&"edit-old.txt".to_string()));
    }

    #[test]
    fn builds_prompt_with_replacement_instructions() {
        let entries = vec![message_entry("1", None, MessageRole::User, "hello")];
        let draft = build_branch_summary_prompt(
            &entries,
            0,
            BranchSummaryInstructions {
                custom_instructions: Some("Only list files".to_string()),
                replace_instructions: true,
            },
        );

        assert!(draft.prompt.contains("<conversation>"));
        assert!(draft.prompt.contains("Only list files"));
        assert!(!draft.prompt.contains("Use this EXACT format"));
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
