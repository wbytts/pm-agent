use serde::{Deserialize, Serialize};

pub const COMPACTION_SUMMARY_PREFIX: &str = "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";
pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BashExecutionMessage {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
    pub timestamp: u64,
    pub exclude_from_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessage {
    pub custom_type: String,
    pub content: String,
    pub display: bool,
    pub details: Option<serde_json::Value>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryMessage {
    pub summary: String,
    pub from_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryMessage {
    pub summary: String,
    pub tokens_before: u64,
    pub timestamp: u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bash_execution_for_context() {
        let text = bash_execution_to_text(&BashExecutionMessage {
            command: "cargo test".to_string(),
            output: "failed".to_string(),
            exit_code: Some(101),
            cancelled: false,
            truncated: true,
            full_output_path: Some("/tmp/out".to_string()),
            timestamp: 1,
            exclude_from_context: false,
        });

        assert!(text.contains("Ran `cargo test`"));
        assert!(text.contains("Command exited with code 101"));
        assert!(text.contains("/tmp/out"));
    }
}
