use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent::harness::{SessionStorage, SessionTreeEntry};
use serde_json::json;

use crate::session_manager::SessionManager;
use crate::utils::paths::resolve_path;

#[derive(Debug, Clone, Default)]
pub struct JsonlExportOptions {
    pub output_path: Option<PathBuf>,
}

/// 按 pi 的 /export .jsonl 语义导出当前分支，而不是复制完整树。
pub fn export_session_to_jsonl<S: SessionStorage>(
    manager: &SessionManager<S>,
    options: JsonlExportOptions,
) -> Result<PathBuf, String> {
    let output_path = jsonl_output_path(manager, options.output_path.as_deref());
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let header = json!({
        "type": "session",
        "version": crate::session_manager::CURRENT_SESSION_VERSION,
        "id": manager.session_id(),
        "timestamp": manager.storage_metadata().created_at,
        "cwd": manager.cwd().to_string_lossy(),
        "parentSession": manager.storage_metadata().parent_session_path,
    });

    let mut lines = vec![header.to_string()];
    let mut parent_id = None;
    for entry in manager.branch(None)? {
        let linear = reparent_entry(entry, parent_id.clone())?;
        parent_id = Some(linear.id().to_string());
        lines.push(serde_json::to_string(&linear).map_err(|error| error.to_string())?);
    }

    fs::write(&output_path, format!("{}\n", lines.join("\n")))
        .map_err(|error| error.to_string())?;
    Ok(output_path)
}

fn jsonl_output_path<S: SessionStorage>(
    manager: &SessionManager<S>,
    explicit: Option<&Path>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    resolve_path(
        &format!("session-{}.jsonl", export_timestamp_string()),
        manager.cwd(),
        None,
    )
}

fn export_timestamp_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{millis}")
}

fn reparent_entry(
    entry: SessionTreeEntry,
    next_parent_id: Option<String>,
) -> Result<SessionTreeEntry, String> {
    let value = serde_json::to_value(entry).map_err(|error| error.to_string())?;
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| "Session entry did not serialize to an object".to_string())?;
    match next_parent_id {
        Some(parent_id) => {
            object.insert("parentId".to_string(), serde_json::Value::String(parent_id));
        }
        None => {
            object.insert("parentId".to_string(), serde_json::Value::Null);
        }
    }
    serde_json::from_value(serde_json::Value::Object(object)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::AgentMessage;
    use ai::MessageRole;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn exports_current_branch_as_linear_jsonl_like_pi() {
        let dir = temp_dir();
        let out = dir.join("branch.jsonl");
        let mut manager = SessionManager::in_memory(dir.clone());
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first");
        let branch = manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "branch".to_string(),
            ))
            .expect("branch");
        manager.move_to(Some(first)).expect("move to first");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "second".to_string()))
            .expect("second");

        let path = export_session_to_jsonl(
            &manager,
            JsonlExportOptions {
                output_path: Some(out.clone()),
            },
        )
        .expect("export");

        assert_eq!(path, out);
        let records = read_jsonl(&path);
        assert_eq!(records[0]["type"], "session");
        assert_eq!(records[1]["parentId"], Value::Null);
        assert_eq!(records[2]["parentId"], records[1]["id"]);
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record["id"] != branch));
    }

    #[test]
    fn default_jsonl_export_path_matches_pi_cwd_behavior() {
        let dir = temp_dir();
        let mut manager = SessionManager::in_memory(dir.clone());
        manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message");

        let path =
            export_session_to_jsonl(&manager, JsonlExportOptions::default()).expect("export");

        assert_eq!(path.parent(), Some(dir.as_path()));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("jsonl")
        );
        assert!(path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("session-")));
    }

    fn read_jsonl(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .expect("jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).expect("record"))
            .collect()
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-jsonl-export-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
