use super::{default_session_dir, parse_millis, SessionInfo};
use crate::utils::paths::resolve_path;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSession {
    Path { path: String },
    Local { path: String },
    Global { path: String, cwd: String },
    NotFound { arg: String },
}

pub fn find_most_recent_session(session_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(session_dir).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .filter(|path| is_valid_session_file(path))
        .filter_map(|path| {
            let modified = path.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

pub fn list_sessions(cwd: &Path, session_dir: Option<&Path>) -> Vec<SessionInfo> {
    let dir = session_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_session_dir(cwd));
    list_sessions_from_dir(&dir)
}

pub fn list_all_sessions(sessions_root: Option<&Path>) -> Vec<SessionInfo> {
    let root = sessions_root
        .map(Path::to_path_buf)
        .unwrap_or_else(default_sessions_root);
    let mut sessions = fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry)
        })
        .flat_map(|entry| list_sessions_from_dir(&entry.path()))
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));
    sessions
}

pub fn list_sessions_from_dir(session_dir: &Path) -> Vec<SessionInfo> {
    let mut sessions = fs::read_dir(session_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .filter_map(|path| build_session_info(&path))
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));
    sessions
}

pub fn resolve_session_path(
    session_arg: &str,
    cwd: &Path,
    session_dir: Option<&Path>,
    sessions_root: Option<&Path>,
) -> ResolvedSession {
    if looks_like_session_path(session_arg) {
        return ResolvedSession::Path {
            path: resolve_path_arg(session_arg, cwd)
                .to_string_lossy()
                .to_string(),
        };
    }

    let local_sessions = list_sessions(cwd, session_dir);
    if let Some(session) = local_sessions
        .into_iter()
        .find(|session| session.id.starts_with(session_arg))
    {
        return ResolvedSession::Local { path: session.path };
    }

    let all_sessions = list_all_sessions(sessions_root);
    if let Some(session) = all_sessions
        .into_iter()
        .find(|session| session.id.starts_with(session_arg))
    {
        return ResolvedSession::Global {
            path: session.path,
            cwd: session.cwd,
        };
    }

    ResolvedSession::NotFound {
        arg: session_arg.to_string(),
    }
}

pub(super) fn build_session_info(path: &Path) -> Option<SessionInfo> {
    let content = fs::read_to_string(path).ok()?;
    let entries = parse_session_values_lenient(&content);
    let header = entries.first()?;
    if header.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let id = header.get("id").and_then(Value::as_str)?.to_string();
    let header_timestamp = header
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let parent_session_path = header
        .get("parentSession")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let stat = path.metadata().ok()?;
    let stat_modified_millis = stat
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    let mut message_count = 0;
    let mut first_message = String::new();
    let mut all_messages = Vec::new();
    let mut name = None;
    for entry in &entries {
        match entry.get("type").and_then(Value::as_str) {
            Some("session_info") => {
                name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
            }
            Some("message") => {
                message_count += 1;
                let Some((role, content)) = message_role_and_content(entry) else {
                    continue;
                };
                if role == "user" && first_message.is_empty() {
                    first_message = content.clone();
                }
                if role == "user" || role == "assistant" {
                    all_messages.push(content);
                }
            }
            _ => {}
        }
    }
    let modified_millis =
        session_modified_millis(&entries, &header_timestamp, stat_modified_millis);

    Some(SessionInfo {
        path: path.to_string_lossy().to_string(),
        id,
        cwd,
        name,
        parent_session_path,
        created_millis: parse_millis(&header_timestamp),
        modified_millis,
        message_count,
        first_message: if first_message.is_empty() {
            "(no messages)".to_string()
        } else {
            first_message
        },
        all_messages_text: all_messages.join(" "),
    })
}

fn parse_session_values_lenient(content: &str) -> Vec<Value> {
    content
        .trim()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                None
            } else {
                serde_json::from_str::<Value>(line).ok()
            }
        })
        .collect()
}

fn message_role_and_content(entry: &Value) -> Option<(String, String)> {
    let message = entry.get("message")?;
    let role = message.get("role")?.as_str()?.to_ascii_lowercase();
    let content = message.get("content")?;
    let text = if let Some(text) = content.as_str() {
        text.to_string()
    } else {
        content
            .as_array()?
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Some((role, text))
}

fn session_modified_millis(
    entries: &[Value],
    header_timestamp: &str,
    stat_modified_millis: u128,
) -> u128 {
    entries
        .iter()
        .filter_map(|entry| match entry {
            Value::Object(object)
                if object.get("type").and_then(Value::as_str) == Some("message") =>
            {
                let role = object
                    .get("message")
                    .and_then(|message| message.get("role"))
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)?;
                if role != "user" && role != "assistant" {
                    return None;
                }
                object
                    .get("message")
                    .and_then(|message| message.get("timestamp"))
                    .and_then(Value::as_u64)
                    .map(u128::from)
                    .or_else(|| {
                        object
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(parse_millis)
                    })
            }
            _ => None,
        })
        .max()
        .filter(|value| *value > 0)
        .or_else(|| {
            let header_millis = parse_millis(header_timestamp);
            (header_millis > 0).then_some(header_millis)
        })
        .unwrap_or(stat_modified_millis)
}

fn looks_like_session_path(session_arg: &str) -> bool {
    session_arg.contains('/') || session_arg.contains('\\') || session_arg.ends_with(".jsonl")
}

fn resolve_path_arg(session_arg: &str, cwd: &Path) -> PathBuf {
    resolve_path(session_arg, cwd, None)
}

fn default_sessions_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".pm-agent").join("agent").join("sessions")
}

fn is_valid_session_file(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let Some(first_line) = content.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(first_line)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .as_deref()
        == Some("session")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::SessionManager;
    use agent::AgentMessage;
    use ai::MessageRole;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_path_arguments_relative_to_cwd() {
        let resolved = resolve_session_path(
            "sessions/demo.jsonl",
            Path::new("/tmp/project"),
            None,
            Some(Path::new("/tmp/no-sessions")),
        );

        assert_eq!(
            resolved,
            ResolvedSession::Path {
                path: "/tmp/project/sessions/demo.jsonl".to_string()
            }
        );
    }

    #[test]
    fn resolves_path_arguments_with_dot_segments_like_pi() {
        let resolved = resolve_session_path(
            "sessions/../demo.jsonl",
            Path::new("/tmp/project"),
            None,
            Some(Path::new("/tmp/no-sessions")),
        );

        assert_eq!(
            resolved,
            ResolvedSession::Path {
                path: "/tmp/project/demo.jsonl".to_string()
            }
        );
    }

    #[test]
    fn resolves_local_session_id_prefix_before_global() {
        let local_dir = temp_dir("local");
        let global_root = temp_dir("global-root");
        let global_dir = global_root.join("other");
        fs::create_dir_all(&global_dir).expect("global dir should be created");

        let mut local = SessionManager::create("/tmp/project", Some(local_dir.clone()))
            .expect("local session should create");
        local
            .append_message(AgentMessage::new(MessageRole::User, "local".to_string()))
            .expect("local message should append");
        let mut global = SessionManager::create("/tmp/other", Some(global_dir))
            .expect("global session should create");
        global
            .append_message(AgentMessage::new(MessageRole::User, "global".to_string()))
            .expect("global message should append");

        let prefix = &local.session_id()[..4];
        let resolved = resolve_session_path(
            prefix,
            Path::new("/tmp/project"),
            Some(&local_dir),
            Some(&global_root),
        );

        assert_eq!(
            resolved,
            ResolvedSession::Local {
                path: local
                    .session_file()
                    .expect("local session file should exist")
                    .to_string_lossy()
                    .to_string()
            }
        );
    }

    #[test]
    fn resolves_global_session_id_prefix() {
        let local_dir = temp_dir("empty-local");
        let global_root = temp_dir("global-root");
        let global_dir = global_root.join("other");
        fs::create_dir_all(&global_dir).expect("global dir should be created");
        let global = SessionManager::create("/tmp/other", Some(global_dir))
            .expect("global session should create");

        let prefix = &global.session_id()[..4];
        let resolved = resolve_session_path(
            prefix,
            Path::new("/tmp/project"),
            Some(&local_dir),
            Some(&global_root),
        );

        assert_eq!(
            resolved,
            ResolvedSession::Global {
                path: global
                    .session_file()
                    .expect("global session file should exist")
                    .to_string_lossy()
                    .to_string(),
                cwd: "/tmp/other".to_string(),
            }
        );
    }

    #[test]
    fn list_sessions_sorts_by_message_activity_timestamp_like_pi() {
        let dir = temp_dir("activity-sort");
        let old_file = dir.join("newer-file-old-activity.jsonl");
        let new_file = dir.join("older-file-new-activity.jsonl");
        write_session_file(
            &old_file,
            "old-activity",
            "1000",
            &[("message-old", "1001", "User", "old")],
        );
        write_session_file(
            &new_file,
            "new-activity",
            "1000",
            &[("message-new", "2000", "User", "new")],
        );

        let sessions = list_sessions_from_dir(&dir);

        assert_eq!(sessions[0].id, "new-activity");
        assert_eq!(sessions[0].modified_millis, 2000);
        assert_eq!(sessions[1].id, "old-activity");
    }

    #[test]
    fn list_sessions_skips_malformed_jsonl_entries_like_pi() {
        let dir = temp_dir("malformed-lines");
        let file = dir.join("session.jsonl");
        fs::write(
            &file,
            concat!(
                r#"{"type":"session","version":3,"id":"malformed-lines","timestamp":"1000","cwd":"/tmp/project"}"#,
                "\n",
                "not json\n",
                r#"{"type":"message","id":"message-1","parentId":null,"timestamp":"2000","message":{"role":"User","content":"kept"}}"#,
                "\n"
            ),
        )
        .expect("session file should write");

        let sessions = list_sessions_from_dir(&dir);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "malformed-lines");
        assert_eq!(sessions[0].message_count, 1);
        assert_eq!(sessions[0].first_message, "kept");
        assert_eq!(sessions[0].modified_millis, 2000);
    }

    fn write_session_file(
        path: &Path,
        id: &str,
        header_timestamp: &str,
        messages: &[(&str, &str, &str, &str)],
    ) {
        let mut lines = vec![format!(
            r#"{{"type":"session","version":3,"id":"{id}","timestamp":"{header_timestamp}","cwd":"/tmp/project"}}"#
        )];
        for (entry_id, timestamp, role, content) in messages {
            lines.push(format!(
                r#"{{"type":"message","id":"{entry_id}","parentId":null,"timestamp":"{timestamp}","message":{{"role":"{role}","content":"{content}"}}}}"#
            ));
        }
        fs::write(path, format!("{}\n", lines.join("\n"))).expect("session file should write");
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-session-{label}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
