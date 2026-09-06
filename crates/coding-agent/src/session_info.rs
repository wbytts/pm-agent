use serde::{Deserialize, Serialize};

use crate::session_manager::SessionStats;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfoSummary {
    pub name: Option<String>,
    pub stats: SessionStats,
    pub cost: f64,
    pub text: String,
}

pub fn session_info_summary(stats: SessionStats, name: Option<String>) -> SessionInfoSummary {
    let cost = stats.cost_micros as f64 / 1_000_000.0;
    let text = format_session_info(&stats, name.as_deref(), cost);
    SessionInfoSummary {
        name,
        stats,
        cost,
        text,
    }
}

pub fn format_session_info(stats: &SessionStats, name: Option<&str>, cost: f64) -> String {
    let mut info = String::from("Session Info\n\n");
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        info.push_str(&format!("Name: {name}\n"));
    }
    info.push_str(&format!(
        "File: {}\n",
        stats.session_file.as_deref().unwrap_or("In-memory")
    ));
    info.push_str(&format!("ID: {}\n\n", stats.session_id));
    info.push_str("Messages\n");
    info.push_str(&format!("User: {}\n", stats.user_messages));
    info.push_str(&format!("Assistant: {}\n", stats.assistant_messages));
    info.push_str(&format!("Tool Calls: {}\n", stats.tool_calls));
    info.push_str(&format!("Tool Results: {}\n", stats.tool_results));
    info.push_str(&format!("Total: {}\n\n", stats.total_messages));
    info.push_str("Tokens\n");
    info.push_str(&format!("Input: {}\n", format_number(stats.tokens.input)));
    info.push_str(&format!("Output: {}\n", format_number(stats.tokens.output)));
    if stats.tokens.cache_read > 0 {
        info.push_str(&format!(
            "Cache Read: {}\n",
            format_number(stats.tokens.cache_read)
        ));
    }
    if stats.tokens.cache_write > 0 {
        info.push_str(&format!(
            "Cache Write: {}\n",
            format_number(stats.tokens.cache_write)
        ));
    }
    info.push_str(&format!("Total: {}", format_number(stats.tokens.total)));

    if cost > 0.0 {
        info.push_str("\n\nCost\n");
        info.push_str(&format!("Total: {cost:.4}"));
    }
    info
}

fn format_number(value: u64) -> String {
    let value = value.to_string();
    let mut formatted = String::new();
    for (index, character) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::SessionTokenStats;

    #[test]
    fn formats_session_info_like_pi_session_command() {
        let stats = stats();

        let info = format_session_info(&stats, Some("Demo"), 1.2345);

        assert_eq!(
            info,
            "Session Info\n\nName: Demo\nFile: /tmp/session.jsonl\nID: session-1\n\nMessages\nUser: 2\nAssistant: 3\nTool Calls: 4\nTool Results: 5\nTotal: 10\n\nTokens\nInput: 1,234\nOutput: 56,789\nCache Read: 10\nCache Write: 20\nTotal: 58,053\n\nCost\nTotal: 1.2345"
        );
    }

    #[test]
    fn omits_empty_optional_sections_like_pi_session_command() {
        let mut stats = stats();
        stats.session_file = None;
        stats.tokens.cache_read = 0;
        stats.tokens.cache_write = 0;
        stats.cost_micros = 0;

        let summary = session_info_summary(stats, None);

        assert_eq!(summary.cost, 0.0);
        assert!(summary.text.contains("File: In-memory"));
        assert!(!summary.text.contains("Name:"));
        assert!(!summary.text.contains("Cache Read:"));
        assert!(!summary.text.contains("Cost"));
    }

    fn stats() -> SessionStats {
        SessionStats {
            session_file: Some("/tmp/session.jsonl".to_string()),
            session_id: "session-1".to_string(),
            user_messages: 2,
            assistant_messages: 3,
            tool_calls: 4,
            tool_results: 5,
            total_messages: 10,
            tokens: SessionTokenStats {
                input: 1_234,
                output: 56_789,
                cache_read: 10,
                cache_write: 20,
                total: 58_053,
            },
            cost_micros: 1_234_500,
        }
    }
}
