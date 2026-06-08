use super::{default_session_dir, parse_millis, SessionInfo};
use agent::harness::{JsonlSessionStorage, SessionStorage, SessionTreeEntry};
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
    let storage = JsonlSessionStorage::open(path).ok()?;
    let metadata = storage.metadata().clone();
    let entries = storage.entries();
    let stat = path.metadata().ok()?;
    let modified_millis = stat
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    let mut message_count = 0;
    let mut first_message = String::new();
    let mut all_messages = Vec::new();
    let mut name = None;
    for entry in entries {
        match entry {
            SessionTreeEntry::SessionInfo {
                name: next_name, ..
            } => {
                name = (!next_name.trim().is_empty()).then_some(next_name);
            }
            SessionTreeEntry::Message { message, .. } => {
                message_count += 1;
                if message.role == ai::MessageRole::User && first_message.is_empty() {
                    first_message = message.content.clone();
                }
                if matches!(
                    message.role,
                    ai::MessageRole::User | ai::MessageRole::Assistant
                ) {
                    all_messages.push(message.content);
                }
            }
            _ => {}
        }
    }

    Some(SessionInfo {
        path: path.to_string_lossy().to_string(),
        id: metadata.id,
        cwd: metadata.cwd.unwrap_or_default(),
        name,
        parent_session_path: metadata.parent_session_path,
        created_millis: parse_millis(&metadata.created_at),
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

fn looks_like_session_path(session_arg: &str) -> bool {
    session_arg.contains('/') || session_arg.contains('\\') || session_arg.ends_with(".jsonl")
}

fn resolve_path_arg(session_arg: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(session_arg);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
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
