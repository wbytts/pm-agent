use crate::utils::paths::resolve_path;
use agent::harness::{
    build_session_context, InMemorySessionStorage, JsonlSessionStorage, SessionContext,
    SessionErrorCode, SessionStorage, SessionTreeEntry,
};
use agent::AgentMessage;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod discovery;
mod fork;

pub use discovery::{
    find_most_recent_session, list_all_sessions, list_sessions, list_sessions_from_dir,
    resolve_session_path, ResolvedSession,
};

pub const CURRENT_SESSION_VERSION: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub path: String,
    pub id: String,
    pub cwd: String,
    pub name: Option<String>,
    pub parent_session_path: Option<String>,
    pub created_millis: u128,
    pub modified_millis: u128,
    pub message_count: usize,
    pub first_message: String,
    pub all_messages_text: String,
}

#[derive(Debug, Clone)]
pub struct SessionTreeNode {
    pub entry: SessionTreeEntry,
    pub children: Vec<SessionTreeNode>,
    pub label: Option<String>,
    pub label_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenStats {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub session_file: Option<String>,
    pub session_id: String,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub total_messages: usize,
    pub tokens: SessionTokenStats,
    pub cost_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForkMessage {
    pub entry_id: String,
    pub text: String,
}

pub struct SessionManager<S: SessionStorage> {
    storage: S,
    cwd: PathBuf,
    session_dir: PathBuf,
    session_file: Option<PathBuf>,
    persist: bool,
}

impl SessionManager<InMemorySessionStorage> {
    pub fn in_memory(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        Self {
            storage: InMemorySessionStorage::default(),
            session_dir: PathBuf::new(),
            session_file: None,
            persist: false,
            cwd,
        }
    }
}

impl SessionManager<JsonlSessionStorage> {
    pub fn create(cwd: impl Into<PathBuf>, session_dir: Option<PathBuf>) -> Result<Self, String> {
        let cwd = cwd.into();
        let session_dir = session_dir.unwrap_or_else(|| default_session_dir(&cwd));
        fs::create_dir_all(&session_dir)
            .map_err(|error| format!("创建 session 目录失败：{error}"))?;
        let session_id = session_id();
        let session_file = session_dir.join(format!("{}_{}.jsonl", timestamp_string(), session_id));
        let storage = JsonlSessionStorage::create(
            &session_file,
            cwd.to_string_lossy().to_string(),
            session_id,
            None,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            storage,
            cwd,
            session_dir,
            session_file: Some(session_file),
            persist: true,
        })
    }

    pub fn open(path: impl Into<PathBuf>, session_dir: Option<PathBuf>) -> Result<Self, String> {
        let raw_session_file = path.into();
        let session_file = resolve_path(&raw_session_file.to_string_lossy(), "", None);
        if !session_file.exists()
            || session_file
                .metadata()
                .is_ok_and(|metadata| metadata.len() == 0)
        {
            return Self::create_explicit_session_file(session_file, session_dir);
        }
        let storage = match JsonlSessionStorage::open(&session_file) {
            Ok(storage) => storage,
            Err(error) if error.code == SessionErrorCode::InvalidSession => {
                return Self::create_explicit_session_file(session_file, session_dir);
            }
            Err(error) => return Err(error.to_string()),
        };
        let cwd = storage
            .metadata()
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let session_dir = session_dir.unwrap_or_else(|| {
            session_file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| default_session_dir(&cwd))
        });
        Ok(Self {
            storage,
            cwd,
            session_dir,
            session_file: Some(session_file),
            persist: true,
        })
    }

    fn create_explicit_session_file(
        session_file: PathBuf,
        session_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let session_dir = session_dir.unwrap_or_else(|| {
            session_file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| default_session_dir(&cwd))
        });
        fs::create_dir_all(&session_dir)
            .map_err(|error| format!("创建 session 目录失败：{error}"))?;
        let storage = JsonlSessionStorage::create(
            &session_file,
            cwd.to_string_lossy().to_string(),
            session_id(),
            None,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            storage,
            cwd,
            session_dir,
            session_file: Some(session_file),
            persist: true,
        })
    }

    pub fn continue_recent(
        cwd: impl Into<PathBuf>,
        session_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let cwd = cwd.into();
        let session_dir = session_dir.unwrap_or_else(|| default_session_dir(&cwd));
        if let Some(path) = find_most_recent_session(&session_dir) {
            return Self::open_with_cwd(path, Some(session_dir), cwd);
        }
        Self::create(cwd, Some(session_dir))
    }
}

impl<S: SessionStorage> SessionManager<S> {
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn session_file(&self) -> Option<&Path> {
        self.session_file.as_deref()
    }

    pub fn session_id(&self) -> &str {
        &self.storage.metadata().id
    }

    pub fn storage_metadata(&self) -> &agent::harness::SessionMetadata {
        self.storage.metadata()
    }

    pub fn is_persisted(&self) -> bool {
        self.persist
    }

    pub fn leaf_id(&self) -> Result<Option<String>, String> {
        self.storage.leaf_id().map_err(|error| error.to_string())
    }

    pub fn entries(&self) -> Vec<SessionTreeEntry> {
        self.storage.entries()
    }

    pub fn branch(&self, from_id: Option<&str>) -> Result<Vec<SessionTreeEntry>, String> {
        let owned_leaf_id = self.leaf_id()?;
        let leaf_id = from_id.or(owned_leaf_id.as_deref());
        self.storage
            .path_to_root(leaf_id)
            .map_err(|error| error.to_string())
    }

    pub fn build_context(&self) -> Result<SessionContext, String> {
        build_session_context(&self.branch(None)?).map_err(|error| error.to_string())
    }

    pub fn session_stats(&self) -> SessionStats {
        let mut user_messages = 0;
        let mut assistant_messages = 0;
        let mut tool_results = 0;
        let mut total_messages = 0;

        for entry in self.storage.entries() {
            if let SessionTreeEntry::Message { message, .. } = entry {
                total_messages += 1;
                match message.role {
                    ai::MessageRole::User => user_messages += 1,
                    ai::MessageRole::Assistant => assistant_messages += 1,
                    ai::MessageRole::Tool => tool_results += 1,
                    ai::MessageRole::System => {}
                }
            }
        }

        SessionStats {
            session_file: self
                .session_file
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            session_id: self.session_id().to_string(),
            user_messages,
            assistant_messages,
            tool_calls: 0,
            tool_results,
            total_messages,
            tokens: SessionTokenStats {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total: 0,
            },
            cost_micros: 0,
        }
    }

    pub fn fork_messages(&self) -> Vec<ForkMessage> {
        self.storage
            .entries()
            .into_iter()
            .filter_map(|entry| match entry {
                SessionTreeEntry::Message { id, message, .. }
                    if message.role == ai::MessageRole::User
                        && !message.content.trim().is_empty() =>
                {
                    Some(ForkMessage {
                        entry_id: id,
                        text: message.content,
                    })
                }
                _ => None,
            })
            .collect()
    }

    pub fn last_assistant_text(&self) -> Option<String> {
        self.storage
            .entries()
            .into_iter()
            .rev()
            .find_map(|entry| match entry {
                SessionTreeEntry::Message { message, .. }
                    if message.role == ai::MessageRole::Assistant
                        && !message.content.is_empty() =>
                {
                    Some(message.content)
                }
                _ => None,
            })
    }

    pub fn append_message(&mut self, message: AgentMessage) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::Message {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            message,
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn append_session_name(&mut self, name: impl Into<String>) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::SessionInfo {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            name: name.into().trim().to_string(),
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn session_name(&self) -> Option<String> {
        self.storage
            .entries_by_type("session_info")
            .into_iter()
            .rev()
            .find_map(|entry| match entry {
                SessionTreeEntry::SessionInfo { name, .. } => {
                    let name = name.trim().to_string();
                    (!name.is_empty()).then_some(name)
                }
                _ => None,
            })
    }

    pub fn append_label_change(
        &mut self,
        target_id: impl Into<String>,
        label: Option<String>,
    ) -> Result<String, String> {
        let target_id = target_id.into();
        if self.storage.entry(&target_id).is_none() {
            return Err(format!("Entry {target_id} not found"));
        }
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::Label {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            target_id,
            label,
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn append_branch_summary(
        &mut self,
        branch_from_id: Option<String>,
        summary: impl Into<String>,
        details: Option<serde_json::Value>,
        from_hook: bool,
    ) -> Result<String, String> {
        if let Some(branch_from_id) = &branch_from_id {
            if self.storage.entry(branch_from_id).is_none() {
                return Err(format!("Entry {branch_from_id} not found"));
            }
        }
        self.storage
            .set_leaf_id(branch_from_id.clone())
            .map_err(|error| error.to_string())?;
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::BranchSummary {
            id: id.clone(),
            parent_id: branch_from_id.clone(),
            timestamp: timestamp_string(),
            from_id: branch_from_id.unwrap_or_else(|| "root".to_string()),
            summary: summary.into(),
            details,
            from_hook,
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn move_to(&mut self, entry_id: Option<String>) -> Result<(), String> {
        self.storage
            .set_leaf_id(entry_id)
            .map_err(|error| error.to_string())
    }

    pub fn tree(&self) -> Vec<SessionTreeNode> {
        let entries = self.storage.entries();
        let mut roots = Vec::new();
        for entry in entries.iter().filter(|entry| entry.parent_id().is_none()) {
            roots.push(build_tree_node(entry, &entries, &self.storage));
        }
        roots
    }
}

pub fn default_session_dir(cwd: &Path) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let safe_path = format!(
        "--{}--",
        cwd.to_string_lossy()
            .trim_start_matches('/')
            .replace(['/', '\\', ':'], "-")
    );
    home.join(".pm-agent")
        .join("agent")
        .join("sessions")
        .join(safe_path)
}

fn build_tree_node<S: SessionStorage>(
    entry: &SessionTreeEntry,
    entries: &[SessionTreeEntry],
    storage: &S,
) -> SessionTreeNode {
    let mut children = entries
        .iter()
        .filter(|candidate| candidate.parent_id() == Some(entry.id()))
        .map(|child| build_tree_node(child, entries, storage))
        .collect::<Vec<_>>();
    children.sort_by(|a, b| entry_timestamp(&a.entry).cmp(&entry_timestamp(&b.entry)));
    SessionTreeNode {
        entry: entry.clone(),
        children,
        label: storage.label(entry.id()),
        label_timestamp: storage.label_timestamp(entry.id()),
    }
}

fn entry_timestamp(entry: &SessionTreeEntry) -> String {
    match entry {
        SessionTreeEntry::Message { timestamp, .. }
        | SessionTreeEntry::ThinkingLevelChange { timestamp, .. }
        | SessionTreeEntry::ModelChange { timestamp, .. }
        | SessionTreeEntry::Compaction { timestamp, .. }
        | SessionTreeEntry::Custom { timestamp, .. }
        | SessionTreeEntry::CustomMessage { timestamp, .. }
        | SessionTreeEntry::Label { timestamp, .. }
        | SessionTreeEntry::SessionInfo { timestamp, .. }
        | SessionTreeEntry::BranchSummary { timestamp, .. }
        | SessionTreeEntry::Leaf { timestamp, .. } => timestamp.clone(),
    }
}

pub(super) fn parse_millis(value: &str) -> u128 {
    value.parse::<u128>().unwrap_or(0)
}

fn session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("{millis:x}")
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::MessageRole;

    #[test]
    fn default_session_dir_encodes_cwd() {
        let dir = default_session_dir(Path::new("/tmp/my:project"));
        assert!(dir.to_string_lossy().contains("--tmp-my-project--"));
    }

    #[test]
    fn memory_session_manager_builds_tree_and_context() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");
        manager
            .append_label_change(first, Some("start".to_string()))
            .expect("label should append");

        let context = manager.build_context().expect("context should build");
        assert_eq!(context.messages.len(), 1);
        assert_eq!(manager.tree().len(), 1);
    }

    #[test]
    fn session_tree_exposes_label_timestamp_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");
        manager
            .append_label_change(first, Some("start".to_string()))
            .expect("label should append");

        let tree = manager.tree();

        assert_eq!(tree[0].label.as_deref(), Some("start"));
        assert!(tree[0].label_timestamp.is_some());
    }

    #[test]
    fn session_queries_support_rpc_helpers() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(
                MessageRole::User,
                "first request".to_string(),
            ))
            .expect("user message should append");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant message should append");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Tool,
                "tool output".to_string(),
            ))
            .expect("tool message should append");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "   ".to_string()))
            .expect("blank user message should append");

        let stats = manager.session_stats();
        assert_eq!(stats.user_messages, 2);
        assert_eq!(stats.assistant_messages, 1);
        assert_eq!(stats.tool_results, 1);
        assert_eq!(stats.total_messages, 4);
        assert_eq!(
            manager.last_assistant_text().as_deref(),
            Some("first answer")
        );

        let fork_messages = manager.fork_messages();
        assert_eq!(fork_messages.len(), 1);
        assert_eq!(fork_messages[0].entry_id, first);
        assert_eq!(fork_messages[0].text, "first request");
    }

    #[test]
    fn persisted_session_manager_lists_sessions() {
        let dir = temp_dir();
        let mut manager = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");
        manager
            .append_session_name("Demo")
            .expect("session name should append");

        let sessions = list_sessions_from_dir(&dir);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name.as_deref(), Some("Demo"));
        assert_eq!(
            find_most_recent_session(&dir),
            manager.session_file().map(Path::to_path_buf)
        );
    }

    #[test]
    fn continue_recent_keeps_requested_cwd_like_pi() {
        let dir = temp_dir();
        let original = SessionManager::create("/tmp/original", Some(dir.clone()))
            .expect("session should create");

        let continued = SessionManager::continue_recent("/tmp/current", Some(dir))
            .expect("session should continue");

        assert_eq!(continued.session_file(), original.session_file());
        assert_eq!(continued.cwd(), Path::new("/tmp/current"));
    }

    #[test]
    fn open_missing_explicit_path_starts_new_session_like_pi() {
        let dir = temp_dir();
        let explicit_path = dir.join("explicit.jsonl");

        let manager = SessionManager::open(&explicit_path, None).expect("session should open");

        assert_eq!(manager.session_file(), Some(explicit_path.as_path()));
        assert_eq!(manager.session_dir(), dir.as_path());
    }

    #[test]
    fn open_empty_explicit_path_rewrites_new_session_like_pi() {
        let dir = temp_dir();
        let explicit_path = dir.join("empty.jsonl");
        fs::write(&explicit_path, "").expect("empty session file should write");

        let manager = SessionManager::open(&explicit_path, None).expect("session should open");

        assert_eq!(manager.session_file(), Some(explicit_path.as_path()));
        let content = fs::read_to_string(&explicit_path).expect("session file should read");
        assert!(content.contains(r#""type":"session""#));
    }

    #[test]
    fn open_corrupt_explicit_path_rewrites_new_session_like_pi() {
        let dir = temp_dir();
        let explicit_path = dir.join("corrupt.jsonl");
        fs::write(&explicit_path, "not json\n{\"type\":\"message\"}\n")
            .expect("corrupt session file should write");

        let manager = SessionManager::open(&explicit_path, None).expect("session should open");

        assert_eq!(manager.session_file(), Some(explicit_path.as_path()));
        let content = fs::read_to_string(&explicit_path).expect("session file should read");
        assert!(content.contains(r#""type":"session""#));
        assert!(!content.contains("not json"));
    }

    #[test]
    fn open_resolves_explicit_path_dot_segments_like_pi() {
        let dir = temp_dir();
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("nested dir should create");
        let explicit_path = dir.join("explicit.jsonl");
        let manager = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        fs::copy(
            manager.session_file().expect("session file should exist"),
            &explicit_path,
        )
        .expect("session file should copy");
        let path_with_dot_segments = nested.join("..").join("explicit.jsonl");

        let opened =
            SessionManager::open(&path_with_dot_segments, None).expect("session should open");

        assert_eq!(opened.session_file(), Some(explicit_path.as_path()));
        assert_eq!(opened.session_dir(), dir.as_path());
    }

    fn temp_dir() -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-session-manager-test-{millis}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
