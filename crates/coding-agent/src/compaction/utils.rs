use agent::AgentMessage;
use ai::MessageRole;
use std::collections::BTreeSet;

const TOOL_RESULT_MAX_CHARS: usize = 2000;
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOperations {
    pub read: BTreeSet<String>,
    pub written: BTreeSet<String>,
    pub edited: BTreeSet<String>,
}

pub fn create_file_ops() -> FileOperations {
    FileOperations::default()
}

pub fn extract_file_ops_from_message(message: &AgentMessage, file_ops: &mut FileOperations) {
    if message.role != MessageRole::Assistant {
        return;
    }

    // 当前 Rust 消息模型还没有结构化 toolCall block，先兼容工具提示文本。
    // 后续迁移完整 tool call 消息后，这里可以无缝增加结构化解析。
    for line in message.content.lines().map(str::trim) {
        let Some((tool, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let path = rest
            .split_whitespace()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match (tool.trim_start_matches('/'), path) {
            ("read", Some(path)) => {
                file_ops.read.insert(path.to_string());
            }
            ("write", Some(path)) => {
                file_ops.written.insert(path.to_string());
            }
            ("edit", Some(path)) => {
                file_ops.edited.insert(path.to_string());
            }
            _ => {}
        }
    }
}

pub fn compute_file_lists(file_ops: &FileOperations) -> (Vec<String>, Vec<String>) {
    let modified = file_ops
        .written
        .union(&file_ops.edited)
        .cloned()
        .collect::<BTreeSet<_>>();
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

pub fn serialize_conversation_for_summary(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .filter_map(|message| {
            let content = truncate_for_summary(&message.content, TOOL_RESULT_MAX_CHARS);
            match message.role {
                MessageRole::User => Some(format!("[User]: {content}")),
                MessageRole::Assistant => Some(format!("[Assistant]: {content}")),
                MessageRole::Tool => Some(format!("[Tool result]: {content}")),
                MessageRole::System => Some(format!("[System]: {content}")),
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated = text.chars().take(max_chars).collect::<String>();
    let truncated_chars = text.chars().count() - max_chars;
    format!("{truncated}\n\n[... {truncated_chars} more characters truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_read_and_modified_file_lists() {
        let mut ops = create_file_ops();
        ops.read.insert("a.txt".to_string());
        ops.read.insert("b.txt".to_string());
        ops.edited.insert("b.txt".to_string());
        ops.written.insert("c.txt".to_string());

        let (read_files, modified_files) = compute_file_lists(&ops);

        assert_eq!(read_files, vec!["a.txt"]);
        assert_eq!(modified_files, vec!["b.txt", "c.txt"]);
    }

    #[test]
    fn serializes_messages_without_continuing_conversation_shape() {
        let text = serialize_conversation_for_summary(&[
            AgentMessage::new(MessageRole::User, "hello".to_string()),
            AgentMessage::new(MessageRole::Assistant, "world".to_string()),
        ]);

        assert!(text.contains("[User]: hello"));
        assert!(text.contains("[Assistant]: world"));
    }
}
