use ai::{AssistantStopReason, MessageRole, Usage};

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

#[derive(Debug, Clone)]
pub struct BranchSummaryEntries {
    pub entries: Vec<SessionTreeEntry>,
    pub common_ancestor_id: Option<String>,
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
    ((message.content.chars().count() as u64) + 3) / 4
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::session::{InMemorySessionStorage, Session};
    use ai::{Usage, UsageCost};

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
}
