use ai::{AssistantStopReason, MessageRole, Usage};

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
