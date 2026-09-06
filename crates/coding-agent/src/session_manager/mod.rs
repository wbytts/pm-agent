use crate::utils::paths::resolve_path;
use agent::harness::{
    build_session_context, InMemorySessionStorage, JsonlSessionStorage, SessionContext,
    SessionErrorCode, SessionStorage, SessionTreeEntry,
};
use agent::AgentMessage;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod discovery;
mod fork;

pub use discovery::{
    find_most_recent_session, list_all_sessions, list_sessions, list_sessions_from_dir,
    load_entries_from_file, resolve_session_path, ResolvedSession,
};

pub const CURRENT_SESSION_VERSION: u64 = 3;
static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCreateOptions {
    pub id: Option<String>,
    pub parent_session: Option<String>,
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

    pub fn replace_with_new_session(
        &mut self,
        parent_session: Option<String>,
    ) -> Result<(), String> {
        self.replace_with_new_session_with_options(SessionCreateOptions {
            parent_session,
            ..SessionCreateOptions::default()
        })
    }

    pub fn replace_with_new_session_with_options(
        &mut self,
        options: SessionCreateOptions,
    ) -> Result<(), String> {
        self.storage = InMemorySessionStorage::new(
            Vec::new(),
            Some(agent::harness::SessionMetadata {
                id: options.id.unwrap_or_else(session_id),
                created_at: timestamp_string(),
                cwd: Some(self.cwd.to_string_lossy().to_string()),
                path: None,
                parent_session_path: options.parent_session,
            }),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn create_branched_session(&mut self, leaf_id: &str) -> Result<Option<PathBuf>, String> {
        let path = branched_path_entries(&self.storage, leaf_id)?;
        self.storage = InMemorySessionStorage::new(
            path,
            Some(agent::harness::SessionMetadata {
                id: session_id(),
                created_at: timestamp_string(),
                cwd: Some(self.cwd.to_string_lossy().to_string()),
                path: None,
                parent_session_path: None,
            }),
        )
        .map_err(|error| error.to_string())?;
        Ok(None)
    }
}

impl SessionManager<JsonlSessionStorage> {
    pub fn create(cwd: impl Into<PathBuf>, session_dir: Option<PathBuf>) -> Result<Self, String> {
        Self::create_with_parent(cwd, session_dir, None)
    }

    pub fn create_with_parent(
        cwd: impl Into<PathBuf>,
        session_dir: Option<PathBuf>,
        parent_session: Option<String>,
    ) -> Result<Self, String> {
        Self::create_with_options(
            cwd,
            session_dir,
            SessionCreateOptions {
                parent_session,
                ..SessionCreateOptions::default()
            },
        )
    }

    pub fn create_with_options(
        cwd: impl Into<PathBuf>,
        session_dir: Option<PathBuf>,
        options: SessionCreateOptions,
    ) -> Result<Self, String> {
        let cwd = cwd.into();
        let session_dir = session_dir.unwrap_or_else(|| default_session_dir(&cwd));
        fs::create_dir_all(&session_dir)
            .map_err(|error| format!("创建 session 目录失败：{error}"))?;
        let session_id = options.id.unwrap_or_else(session_id);
        let session_file = session_dir.join(format!("{}_{}.jsonl", timestamp_string(), session_id));
        let storage = JsonlSessionStorage::create(
            &session_file,
            cwd.to_string_lossy().to_string(),
            session_id,
            options.parent_session,
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

    pub fn replace_with_new_session(
        &mut self,
        parent_session: Option<String>,
    ) -> Result<(), String> {
        self.replace_with_new_session_with_options(SessionCreateOptions {
            parent_session,
            ..SessionCreateOptions::default()
        })
    }

    pub fn replace_with_new_session_with_options(
        &mut self,
        options: SessionCreateOptions,
    ) -> Result<(), String> {
        *self =
            Self::create_with_options(self.cwd.clone(), Some(self.session_dir.clone()), options)?;
        Ok(())
    }

    pub fn create_branched_session(&mut self, leaf_id: &str) -> Result<Option<PathBuf>, String> {
        let path = branched_path_entries(&self.storage, leaf_id)?;
        let has_assistant = path.iter().any(|entry| {
            matches!(
                entry,
                SessionTreeEntry::Message {
                    message: AgentMessage {
                        role: ai::MessageRole::Assistant,
                        ..
                    },
                    ..
                }
            )
        });
        let previous_session_file = self
            .session_file
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        let (session_file, new_session_id) = next_session_file(&self.session_dir);
        let storage = JsonlSessionStorage::create_with_entries(
            &session_file,
            self.cwd.to_string_lossy().to_string(),
            new_session_id,
            previous_session_file,
            path,
            has_assistant,
        )
        .map_err(|error| error.to_string())?;
        self.storage = storage;
        self.session_file = Some(session_file.clone());
        Ok(Some(session_file))
    }

    pub fn switch_to_session(&mut self, session_path: impl Into<PathBuf>) -> Result<(), String> {
        *self = Self::open(session_path, None)?;
        Ok(())
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

    pub fn entry(&self, id: &str) -> Option<&SessionTreeEntry> {
        self.storage.entry(id)
    }

    pub fn leaf_entry(&self) -> Option<&SessionTreeEntry> {
        self.leaf_id()
            .ok()
            .flatten()
            .and_then(|id| self.storage.entry(&id))
    }

    pub fn branch(&self, from_id: Option<&str>) -> Result<Vec<SessionTreeEntry>, String> {
        let owned_leaf_id = self.leaf_id()?;
        let leaf_id = from_id.or(owned_leaf_id.as_deref());
        self.storage
            .path_to_root(leaf_id)
            .map_err(|error| error.to_string())
    }

    pub fn collect_branch_summary_entries(
        &self,
        old_leaf_id: Option<&str>,
        target_id: &str,
    ) -> Result<crate::compaction::CollectEntriesResult, String> {
        crate::compaction::collect_entries_for_branch_summary(&self.storage, old_leaf_id, target_id)
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

    pub fn append_thinking_level_change(
        &mut self,
        thinking_level: ai::ModelThinkingLevel,
    ) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::ThinkingLevelChange {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            thinking_level,
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn append_model_change(
        &mut self,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::ModelChange {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            provider: provider.into(),
            model_id: model_id.into(),
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn append_compaction(
        &mut self,
        summary: impl Into<String>,
        first_kept_entry_id: impl Into<String>,
        tokens_before: u64,
        details: Option<serde_json::Value>,
        from_hook: bool,
    ) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::Compaction {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            summary: summary.into(),
            first_kept_entry_id: first_kept_entry_id.into(),
            tokens_before,
            details,
            from_hook,
        };
        self.storage
            .append_entry(entry)
            .map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn append_custom_entry(
        &mut self,
        custom_type: impl Into<String>,
        data: Option<serde_json::Value>,
    ) -> Result<String, String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::Custom {
            id: id.clone(),
            parent_id: self.storage.leaf_id().map_err(|error| error.to_string())?,
            timestamp: timestamp_string(),
            custom_type: custom_type.into(),
            data,
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

    pub fn fork_before_user_message(&mut self, entry_id: &str) -> Result<String, String> {
        let target = self
            .storage
            .entry(entry_id)
            .ok_or_else(|| format!("Entry {entry_id} not found"))?;
        let (parent_id, text) = match target {
            SessionTreeEntry::Message {
                parent_id, message, ..
            } if message.role == ai::MessageRole::User => {
                (parent_id.clone(), message.content.clone())
            }
            _ => return Err(format!("Entry {entry_id} is not a user message")),
        };
        self.storage
            .set_leaf_id(parent_id)
            .map_err(|error| error.to_string())?;
        Ok(text)
    }

    pub fn clone_at_leaf(&mut self) -> Result<(), String> {
        let leaf_id = self.leaf_id()?;
        let Some(leaf_id) = leaf_id else {
            return Err("Cannot clone session: no current entry selected".to_string());
        };
        self.storage
            .set_leaf_id(Some(leaf_id))
            .map_err(|error| error.to_string())
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

impl<S: SessionStorage> crate::session_cwd::SessionCwdSource for SessionManager<S> {
    fn cwd(&self) -> &Path {
        self.cwd()
    }

    fn session_file(&self) -> Option<&Path> {
        self.session_file()
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

fn branched_path_entries(
    storage: &impl SessionStorage,
    leaf_id: &str,
) -> Result<Vec<SessionTreeEntry>, String> {
    let path = storage
        .path_to_root(Some(leaf_id))
        .map_err(|error| error.to_string())?;
    if path.is_empty() {
        return Err(format!("Entry {leaf_id} not found"));
    }

    // pi 会排除原路径里的 label entry，再基于当前解析后的 label map 重新串接标签。
    let mut path_without_labels = path
        .into_iter()
        .filter(|entry| !matches!(entry, SessionTreeEntry::Label { .. }))
        .collect::<Vec<_>>();
    let path_entry_ids = path_without_labels
        .iter()
        .map(|entry| entry.id().to_string())
        .collect::<Vec<_>>();
    let mut parent_id = path_without_labels
        .last()
        .map(|entry| entry.id().to_string());

    for target_id in path_entry_ids {
        let Some(label) = storage.label(&target_id) else {
            continue;
        };
        let Some(timestamp) = storage.label_timestamp(&target_id) else {
            continue;
        };
        let id = unique_entry_id(storage, &path_without_labels);
        path_without_labels.push(SessionTreeEntry::Label {
            id: id.clone(),
            parent_id,
            timestamp,
            target_id,
            label: Some(label),
        });
        parent_id = Some(id);
    }

    Ok(path_without_labels)
}

fn unique_entry_id(storage: &impl SessionStorage, entries: &[SessionTreeEntry]) -> String {
    let ids = entries
        .iter()
        .map(|entry| entry.id().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for _ in 0..100 {
        let id = storage.create_entry_id();
        if !ids.contains(&id) {
            return id;
        }
    }
    storage.create_entry_id()
}

pub(super) fn parse_millis(value: &str) -> u128 {
    value.parse::<u128>().unwrap_or(0)
}

fn session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let counter = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let time_low = ((millis >> 16) & 0xffff_ffff) as u32;
    let time_mid = (millis & 0xffff) as u16;
    let random_a = (counter & 0x0fff) as u16;
    let variant_b = 0x8000 | (((counter >> 12) & 0x3fff) as u16);
    let node = counter & 0xffff_ffff_ffff;
    format!("{time_low:08x}-{time_mid:04x}-7{random_a:03x}-{variant_b:04x}-{node:012x}")
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
        .to_string()
}

fn next_session_file(session_dir: &Path) -> (PathBuf, String) {
    loop {
        let id = session_id();
        let path = session_dir.join(format!("{}_{}.jsonl", timestamp_string(), id));
        if !path.exists() {
            return (path, id);
        }
    }
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
    fn memory_session_manager_appends_state_entries_like_pi_tree_traversal() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");

        let thinking = manager
            .append_thinking_level_change(ai::ModelThinkingLevel::High)
            .expect("thinking change should append");
        let model = manager
            .append_model_change("openai", "gpt-4")
            .expect("model change should append");
        let compaction = manager
            .append_compaction("summary", first.clone(), 1000, None, false)
            .expect("compaction should append");
        let custom = manager
            .append_custom_entry("my_data", Some(serde_json::json!({"key": "value"})))
            .expect("custom entry should append");

        let entries = manager.entries();
        assert!(matches!(
            entries.iter().find(|entry| entry.id() == thinking),
            Some(SessionTreeEntry::ThinkingLevelChange {
                parent_id,
                thinking_level,
                ..
            }) if parent_id.as_deref() == Some(first.as_str())
                && *thinking_level == ai::ModelThinkingLevel::High
        ));
        assert!(matches!(
            entries.iter().find(|entry| entry.id() == model),
            Some(SessionTreeEntry::ModelChange {
                parent_id,
                provider,
                model_id,
                ..
            }) if parent_id.as_deref() == Some(thinking.as_str())
                && provider == "openai"
                && model_id == "gpt-4"
        ));
        assert!(matches!(
            entries.iter().find(|entry| entry.id() == compaction),
            Some(SessionTreeEntry::Compaction {
                parent_id,
                summary,
                first_kept_entry_id,
                tokens_before,
                ..
            }) if parent_id.as_deref() == Some(model.as_str())
                && summary == "summary"
                && first_kept_entry_id == &first
                && *tokens_before == 1000
        ));
        assert!(matches!(
            entries.iter().find(|entry| entry.id() == custom),
            Some(SessionTreeEntry::Custom {
                parent_id,
                custom_type,
                data: Some(data),
                ..
            }) if parent_id.as_deref() == Some(compaction.as_str())
                && custom_type == "my_data"
                && data["key"] == "value"
        ));

        let context = manager.build_context().expect("context should build");
        assert_eq!(context.thinking_level, ai::ModelThinkingLevel::High);
        assert_eq!(
            context.model,
            Some(("openai".to_string(), "gpt-4".to_string()))
        );
        assert_eq!(context.messages.len(), 2);
        assert!(context.messages[0].content.contains("summary"));
        assert_eq!(context.messages[1].content, "hello");
    }

    #[test]
    fn memory_session_manager_exposes_entry_and_leaf_entry_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        assert!(manager.leaf_entry().is_none());
        assert!(manager.entry("missing").is_none());

        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first message should append");
        let second = manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "second".to_string(),
            ))
            .expect("second message should append");

        assert!(matches!(
            manager.entry(&first),
            Some(SessionTreeEntry::Message { message, .. })
                if message.role == MessageRole::User && message.content == "first"
        ));
        assert_eq!(
            manager.leaf_entry().map(SessionTreeEntry::id),
            Some(second.as_str())
        );

        manager
            .move_to(Some(first.clone()))
            .expect("leaf should move");
        assert_eq!(
            manager.leaf_entry().map(SessionTreeEntry::id),
            Some(first.as_str())
        );
    }

    #[test]
    fn memory_create_branched_session_replaces_session_with_path_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "1".to_string()))
            .expect("first message should append");
        let second = manager
            .append_message(AgentMessage::new(MessageRole::Assistant, "2".to_string()))
            .expect("second message should append");
        let third = manager
            .append_message(AgentMessage::new(MessageRole::User, "3".to_string()))
            .expect("third message should append");
        manager
            .append_message(AgentMessage::new(MessageRole::Assistant, "4".to_string()))
            .expect("fourth message should append");

        manager
            .move_to(Some(second.clone()))
            .expect("leaf should move to branch point");
        let branch_user = manager
            .append_message(AgentMessage::new(MessageRole::User, "branch".to_string()))
            .expect("branch message should append");
        let branch_assistant = manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "branch answer".to_string(),
            ))
            .expect("branch answer should append");

        assert!(manager
            .create_branched_session(&branch_assistant)
            .expect("memory branch should replace current session")
            .is_none());

        let entry_ids = manager
            .entries()
            .into_iter()
            .map(|entry| entry.id().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            entry_ids,
            vec![first, second, branch_user, branch_assistant]
        );
        assert!(manager.entry(&third).is_none());
    }

    #[test]
    fn memory_create_branched_session_recreates_resolved_labels_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "1".to_string()))
            .expect("first message should append");
        let second = manager
            .append_message(AgentMessage::new(MessageRole::Assistant, "2".to_string()))
            .expect("second message should append");
        manager
            .append_label_change(first.clone(), Some("initial".to_string()))
            .expect("label should append");
        manager
            .append_label_change(first.clone(), Some("renamed".to_string()))
            .expect("label rename should append");

        manager
            .create_branched_session(&second)
            .expect("memory branch should replace current session");

        let entries = manager.entries();
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            entries.last(),
            Some(SessionTreeEntry::Label {
                parent_id,
                target_id,
                label: Some(label),
                ..
            }) if parent_id.as_deref() == Some(second.as_str())
                && target_id == &first
                && label == "renamed"
        ));
        assert_eq!(manager.tree()[0].label.as_deref(), Some("renamed"));
    }

    #[test]
    fn persisted_create_branched_session_writes_immediately_with_assistant_like_pi() {
        let dir = temp_dir();
        let mut manager = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        manager
            .append_message(AgentMessage::new(
                MessageRole::User,
                "first question".to_string(),
            ))
            .expect("user message should append");
        let assistant = manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant message should append");
        manager
            .append_message(AgentMessage::new(
                MessageRole::User,
                "second question".to_string(),
            ))
            .expect("second user message should append");

        let branch_file = manager
            .create_branched_session(&assistant)
            .expect("persisted branch should create")
            .expect("persisted branch should return path");

        assert!(branch_file.exists());
        let content = fs::read_to_string(&branch_file).expect("branch file should read");
        let records = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid jsonl"))
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record["type"] == "session")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record["type"] == "message")
                .count(),
            2
        );
    }

    #[test]
    fn persisted_create_branched_session_defers_file_without_assistant_like_pi() {
        let dir = temp_dir();
        let mut manager = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        let first = manager
            .append_message(AgentMessage::new(
                MessageRole::User,
                "first question".to_string(),
            ))
            .expect("user message should append");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant message should append");

        let branch_file = manager
            .create_branched_session(&first)
            .expect("persisted branch should create")
            .expect("persisted branch should return path");

        assert!(!branch_file.exists());
        manager
            .append_custom_entry("preset-state", Some(serde_json::json!({"name": "plan"})))
            .expect("custom entry should append before assistant");
        assert!(!branch_file.exists());
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "new answer".to_string(),
            ))
            .expect("assistant message should append");

        assert!(branch_file.exists());
        let content = fs::read_to_string(&branch_file).expect("branch file should read");
        let records = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid jsonl"))
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record["type"] == "session")
                .count(),
            1
        );
        let ids = records
            .iter()
            .filter_map(|record| record.get("id").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        let unique_ids = ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique_ids.len(), ids.len());
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
    fn clearing_session_label_removes_resolved_tree_label_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");

        manager
            .append_label_change(first.clone(), Some("checkpoint".to_string()))
            .expect("label should append");
        manager
            .append_label_change(first, None)
            .expect("label clear should append");

        let tree = manager.tree();
        assert_eq!(tree[0].label, None);
        assert_eq!(tree[0].label_timestamp, None);
    }

    #[test]
    fn labeling_missing_entry_fails_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");

        let error = manager
            .append_label_change("non-existent", Some("label".to_string()))
            .expect_err("missing entry should fail");

        assert_eq!(error, "Entry non-existent not found");
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
    fn memory_session_manager_forks_before_user_message_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first message should append");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant message should append");
        let second = manager
            .append_message(AgentMessage::new(MessageRole::User, "second".to_string()))
            .expect("second message should append");

        manager
            .fork_before_user_message(&second)
            .expect("session should fork");

        let messages = manager
            .build_context()
            .expect("context should build")
            .messages;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "answer");
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
    fn new_persisted_session_uses_uuidv7_like_id_like_pi() {
        let dir = temp_dir();
        let manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");

        assert_uuidv7_like(manager.session_id());
        assert_eq!(manager.storage_metadata().id, manager.session_id());
    }

    #[test]
    fn new_persisted_session_uses_custom_id_like_pi() {
        let dir = temp_dir();
        let manager = SessionManager::create_with_options(
            "/tmp/project",
            Some(dir),
            SessionCreateOptions {
                id: Some("my-custom-id".to_string()),
                parent_session: None,
            },
        )
        .expect("session should create");

        assert_eq!(manager.session_id(), "my-custom-id");
        assert_eq!(manager.storage_metadata().id, "my-custom-id");
        assert!(manager
            .session_file()
            .expect("session file should exist")
            .file_name()
            .expect("session file should have name")
            .to_string_lossy()
            .contains("my-custom-id"));
        let content =
            fs::read_to_string(manager.session_file().expect("session file should exist"))
                .expect("session file should read");
        assert!(content.contains(r#""id":"my-custom-id""#));
    }

    #[test]
    fn new_memory_session_uses_custom_id_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");

        manager
            .replace_with_new_session_with_options(SessionCreateOptions {
                id: Some("header-test-id".to_string()),
                parent_session: None,
            })
            .expect("session should replace");

        assert_eq!(manager.session_id(), "header-test-id");
        assert_eq!(manager.storage_metadata().id, "header-test-id");
    }

    #[test]
    fn new_memory_session_without_custom_id_uses_uuidv7_like_id_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");

        manager
            .replace_with_new_session(Some("parent.jsonl".to_string()))
            .expect("session should replace");

        assert_uuidv7_like(manager.session_id());
        assert_eq!(
            manager.storage_metadata().parent_session_path.as_deref(),
            Some("parent.jsonl")
        );
    }

    #[test]
    fn memory_branched_session_uses_new_uuidv7_like_id_like_pi() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        let first = manager
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");

        manager
            .create_branched_session(&first)
            .expect("session should branch");

        assert_uuidv7_like(manager.session_id());
        assert_eq!(manager.storage_metadata().id, manager.session_id());
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

    fn assert_uuidv7_like(value: &str) {
        let parts = value.split('-').collect::<Vec<_>>();
        assert_eq!(parts.len(), 5, "session id should have UUID shape: {value}");
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        assert!(parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(
            parts[2].starts_with('7'),
            "session id should be UUIDv7-like: {value}"
        );
        assert!(
            matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
            "session id should use RFC4122 variant: {value}"
        );
    }
}
