use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ai::{MessageRole, ModelThinkingLevel};
use serde::{Deserialize, Serialize};

use crate::harness::messages::{BranchSummaryMessage, CompactionSummaryMessage, CustomMessage};
use crate::harness::types::{SessionError, SessionErrorCode, SessionResult};
use crate::state::AgentMessage;

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub id: String,
    pub created_at: String,
    pub cwd: Option<String>,
    pub path: Option<String>,
    pub parent_session_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionTreeEntry {
    Message {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        message: AgentMessage,
    },
    ThinkingLevelChange {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        thinking_level: ModelThinkingLevel,
    },
    ModelChange {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        provider: String,
        model_id: String,
    },
    Compaction {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
        details: Option<serde_json::Value>,
        from_hook: bool,
    },
    Custom {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        custom_type: String,
        data: Option<serde_json::Value>,
    },
    CustomMessage {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        custom_type: String,
        content: String,
        display: bool,
        details: Option<serde_json::Value>,
    },
    Label {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        target_id: String,
        label: Option<String>,
    },
    SessionInfo {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        name: String,
    },
    BranchSummary {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        from_id: String,
        summary: String,
        details: Option<serde_json::Value>,
        from_hook: bool,
    },
    Leaf {
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        target_id: Option<String>,
    },
}

impl SessionTreeEntry {
    pub fn id(&self) -> &str {
        match self {
            Self::Message { id, .. }
            | Self::ThinkingLevelChange { id, .. }
            | Self::ModelChange { id, .. }
            | Self::Compaction { id, .. }
            | Self::Custom { id, .. }
            | Self::CustomMessage { id, .. }
            | Self::Label { id, .. }
            | Self::SessionInfo { id, .. }
            | Self::BranchSummary { id, .. }
            | Self::Leaf { id, .. } => id,
        }
    }

    pub fn parent_id(&self) -> Option<&str> {
        match self {
            Self::Message { parent_id, .. }
            | Self::ThinkingLevelChange { parent_id, .. }
            | Self::ModelChange { parent_id, .. }
            | Self::Compaction { parent_id, .. }
            | Self::Custom { parent_id, .. }
            | Self::CustomMessage { parent_id, .. }
            | Self::Label { parent_id, .. }
            | Self::SessionInfo { parent_id, .. }
            | Self::BranchSummary { parent_id, .. }
            | Self::Leaf { parent_id, .. } => parent_id.as_deref(),
        }
    }

    pub fn leaf_id_after_entry(&self) -> Option<String> {
        match self {
            Self::Leaf { target_id, .. } => target_id.clone(),
            _ => Some(self.id().to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionContext {
    pub messages: Vec<AgentMessage>,
    pub thinking_level: ModelThinkingLevel,
    pub model: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryOptions {
    pub summary: String,
    pub details: Option<serde_json::Value>,
    pub from_hook: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlSessionCreateOptions {
    pub cwd: String,
    pub id: Option<String>,
    pub parent_session_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsonlSessionListOptions {
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkPosition {
    Before,
    At,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlSessionForkOptions {
    pub cwd: String,
    pub id: Option<String>,
    pub parent_session_path: Option<String>,
    pub entry_id: Option<String>,
    pub position: Option<ForkPosition>,
}

pub trait SessionStorage {
    fn metadata(&self) -> &SessionMetadata;
    fn leaf_id(&self) -> SessionResult<Option<String>>;
    fn set_leaf_id(&mut self, leaf_id: Option<String>) -> SessionResult<()>;
    fn create_entry_id(&self) -> String;
    fn append_entry(&mut self, entry: SessionTreeEntry) -> SessionResult<()>;
    fn entry(&self, id: &str) -> Option<&SessionTreeEntry>;
    fn entries(&self) -> Vec<SessionTreeEntry>;
    fn entries_by_type(&self, entry_type: &str) -> Vec<SessionTreeEntry>;
    fn label(&self, id: &str) -> Option<String>;
    fn path_to_root(&self, leaf_id: Option<&str>) -> SessionResult<Vec<SessionTreeEntry>>;
}

pub struct Session<S: SessionStorage> {
    storage: S,
}

impl<S: SessionStorage> Session<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }

    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    pub fn metadata(&self) -> &SessionMetadata {
        self.storage.metadata()
    }

    pub fn branch(&self, from_id: Option<&str>) -> SessionResult<Vec<SessionTreeEntry>> {
        let leaf_id = match from_id {
            Some(id) => Some(id.to_string()),
            None => self.storage.leaf_id()?,
        };
        self.storage.path_to_root(leaf_id.as_deref())
    }

    pub fn build_context(&self) -> SessionResult<SessionContext> {
        build_session_context(&self.branch(None)?)
    }

    pub fn append_message(&mut self, message: AgentMessage) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        let entry = SessionTreeEntry::Message {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            message,
        };
        self.storage.append_entry(entry)?;
        Ok(id)
    }

    pub fn append_thinking_level_change(
        &mut self,
        thinking_level: ModelThinkingLevel,
    ) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage
            .append_entry(SessionTreeEntry::ThinkingLevelChange {
                id: id.clone(),
                parent_id: self.storage.leaf_id()?,
                timestamp: timestamp_string(),
                thinking_level,
            })?;
        Ok(id)
    }

    pub fn append_model_change(
        &mut self,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::ModelChange {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            provider: provider.into(),
            model_id: model_id.into(),
        })?;
        Ok(id)
    }

    pub fn append_compaction(
        &mut self,
        summary: impl Into<String>,
        first_kept_entry_id: impl Into<String>,
        tokens_before: u64,
    ) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::Compaction {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            summary: summary.into(),
            first_kept_entry_id: first_kept_entry_id.into(),
            tokens_before,
            details: None,
            from_hook: false,
        })?;
        Ok(id)
    }

    pub fn append_custom_entry(
        &mut self,
        custom_type: impl Into<String>,
        data: Option<serde_json::Value>,
    ) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::Custom {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            custom_type: custom_type.into(),
            data,
        })?;
        Ok(id)
    }

    pub fn append_custom_message_entry(
        &mut self,
        custom_type: impl Into<String>,
        content: impl Into<String>,
        display: bool,
        details: Option<serde_json::Value>,
    ) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::CustomMessage {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            custom_type: custom_type.into(),
            content: content.into(),
            display,
            details,
        })?;
        Ok(id)
    }

    pub fn append_label(
        &mut self,
        target_id: impl Into<String>,
        label: Option<String>,
    ) -> SessionResult<String> {
        let target_id = target_id.into();
        if self.storage.entry(&target_id).is_none() {
            return Err(SessionError::new(
                SessionErrorCode::NotFound,
                format!("Entry {target_id} not found"),
            ));
        }
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::Label {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            target_id,
            label,
        })?;
        Ok(id)
    }

    pub fn append_session_name(&mut self, name: impl AsRef<str>) -> SessionResult<String> {
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::SessionInfo {
            id: id.clone(),
            parent_id: self.storage.leaf_id()?,
            timestamp: timestamp_string(),
            name: name.as_ref().trim().to_string(),
        })?;
        Ok(id)
    }

    pub fn session_name(&self) -> Option<String> {
        self.storage
            .entries_by_type("session_info")
            .into_iter()
            .filter_map(|entry| match entry {
                SessionTreeEntry::SessionInfo { name, .. } => {
                    let trimmed = name.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                }
                _ => None,
            })
            .next_back()
    }

    pub fn move_to(
        &mut self,
        entry_id: Option<String>,
        summary: Option<BranchSummaryOptions>,
    ) -> SessionResult<Option<String>> {
        if let Some(entry_id) = &entry_id {
            if self.storage.entry(entry_id).is_none() {
                return Err(SessionError::new(
                    SessionErrorCode::NotFound,
                    format!("Entry {entry_id} not found"),
                ));
            }
        }
        self.storage.set_leaf_id(entry_id.clone())?;
        let Some(summary) = summary else {
            return Ok(None);
        };
        let id = self.storage.create_entry_id();
        self.storage.append_entry(SessionTreeEntry::BranchSummary {
            id: id.clone(),
            parent_id: entry_id.clone(),
            timestamp: timestamp_string(),
            from_id: entry_id.unwrap_or_else(|| "root".to_string()),
            summary: summary.summary,
            details: summary.details,
            from_hook: summary.from_hook.unwrap_or(false),
        })?;
        Ok(Some(id))
    }
}

#[derive(Debug, Clone)]
pub struct InMemorySessionStorage {
    metadata: SessionMetadata,
    entries: Vec<SessionTreeEntry>,
    by_id: BTreeMap<String, SessionTreeEntry>,
    labels_by_id: BTreeMap<String, String>,
    leaf_id: Option<String>,
}

impl InMemorySessionStorage {
    pub fn new(
        entries: Vec<SessionTreeEntry>,
        metadata: Option<SessionMetadata>,
    ) -> SessionResult<Self> {
        let mut by_id = BTreeMap::new();
        let mut labels_by_id = BTreeMap::new();
        let mut leaf_id = None;
        for entry in &entries {
            update_label_cache(&mut labels_by_id, entry);
            leaf_id = entry.leaf_id_after_entry();
            by_id.insert(entry.id().to_string(), entry.clone());
        }
        if let Some(leaf_id) = &leaf_id {
            if !by_id.contains_key(leaf_id) {
                return Err(SessionError::new(
                    SessionErrorCode::InvalidSession,
                    format!("Entry {leaf_id} not found"),
                ));
            }
        }

        Ok(Self {
            metadata: metadata.unwrap_or_else(default_metadata),
            entries,
            by_id,
            labels_by_id,
            leaf_id,
        })
    }
}

impl Default for InMemorySessionStorage {
    fn default() -> Self {
        Self::new(Vec::new(), None).expect("empty memory storage should be valid")
    }
}

impl SessionStorage for InMemorySessionStorage {
    fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    fn leaf_id(&self) -> SessionResult<Option<String>> {
        if let Some(leaf_id) = &self.leaf_id {
            if !self.by_id.contains_key(leaf_id) {
                return Err(SessionError::new(
                    SessionErrorCode::InvalidSession,
                    format!("Entry {leaf_id} not found"),
                ));
            }
        }
        Ok(self.leaf_id.clone())
    }

    fn set_leaf_id(&mut self, leaf_id: Option<String>) -> SessionResult<()> {
        if let Some(leaf_id) = &leaf_id {
            if !self.by_id.contains_key(leaf_id) {
                return Err(SessionError::new(
                    SessionErrorCode::NotFound,
                    format!("Entry {leaf_id} not found"),
                ));
            }
        }
        let entry = SessionTreeEntry::Leaf {
            id: self.create_entry_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: timestamp_string(),
            target_id: leaf_id.clone(),
        };
        self.entries.push(entry.clone());
        self.by_id.insert(entry.id().to_string(), entry);
        self.leaf_id = leaf_id;
        Ok(())
    }

    fn create_entry_id(&self) -> String {
        generate_entry_id(|id| self.by_id.contains_key(id))
    }

    fn append_entry(&mut self, entry: SessionTreeEntry) -> SessionResult<()> {
        self.leaf_id = entry.leaf_id_after_entry();
        update_label_cache(&mut self.labels_by_id, &entry);
        self.by_id.insert(entry.id().to_string(), entry.clone());
        self.entries.push(entry);
        Ok(())
    }

    fn entry(&self, id: &str) -> Option<&SessionTreeEntry> {
        self.by_id.get(id)
    }

    fn entries(&self) -> Vec<SessionTreeEntry> {
        self.entries.clone()
    }

    fn entries_by_type(&self, entry_type: &str) -> Vec<SessionTreeEntry> {
        self.entries
            .iter()
            .filter(|entry| entry_type_name(entry) == entry_type)
            .cloned()
            .collect()
    }

    fn label(&self, id: &str) -> Option<String> {
        self.labels_by_id.get(id).cloned()
    }

    fn path_to_root(&self, leaf_id: Option<&str>) -> SessionResult<Vec<SessionTreeEntry>> {
        path_to_root(&self.by_id, leaf_id)
    }
}

#[derive(Debug, Clone)]
pub struct JsonlSessionStorage {
    metadata: SessionMetadata,
    file_path: PathBuf,
    inner: InMemorySessionStorage,
}

impl JsonlSessionStorage {
    pub fn create(
        file_path: impl Into<PathBuf>,
        cwd: impl Into<String>,
        session_id: impl Into<String>,
        parent_session_path: Option<String>,
    ) -> SessionResult<Self> {
        let file_path = file_path.into();
        let metadata = SessionMetadata {
            id: session_id.into(),
            created_at: timestamp_string(),
            cwd: Some(cwd.into()),
            path: Some(file_path.to_string_lossy().to_string()),
            parent_session_path,
        };
        write_header(&file_path, &metadata)?;
        Ok(Self {
            metadata: metadata.clone(),
            file_path,
            inner: InMemorySessionStorage::new(Vec::new(), Some(metadata))?,
        })
    }

    pub fn open(file_path: impl Into<PathBuf>) -> SessionResult<Self> {
        let file_path = file_path.into();
        let content = fs::read_to_string(&file_path).map_err(|error| {
            SessionError::new(
                SessionErrorCode::Storage,
                format!("Failed to read session {}: {error}", file_path.display()),
            )
        })?;
        let mut lines = content.lines().filter(|line| !line.trim().is_empty());
        let header_line = lines.next().ok_or_else(|| {
            SessionError::new(
                SessionErrorCode::InvalidSession,
                format!(
                    "Invalid JSONL session file {}: missing session header",
                    file_path.display()
                ),
            )
        })?;
        let header_value = parse_header_value(header_line, &file_path)?;
        let session_version = session_header_version(&header_value);
        validate_session_version(session_version, &file_path)?;
        let mut metadata = metadata_from_header_value(&header_value, &file_path)?;
        metadata.path = Some(file_path.to_string_lossy().to_string());
        let mut entry_values = lines
            .enumerate()
            .map(|(index, line)| parse_entry_value(line, &file_path, index + 2))
            .collect::<SessionResult<Vec<_>>>()?;
        migrate_session_entry_values(session_version, &mut entry_values);
        let entries = entry_values
            .into_iter()
            .enumerate()
            .map(|(index, value)| parse_entry_value_as_entry(value, &file_path, index + 2))
            .collect::<SessionResult<Vec<_>>>()?;

        Ok(Self {
            metadata: metadata.clone(),
            file_path,
            inner: InMemorySessionStorage::new(entries, Some(metadata))?,
        })
    }
}

impl SessionStorage for JsonlSessionStorage {
    fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    fn leaf_id(&self) -> SessionResult<Option<String>> {
        self.inner.leaf_id()
    }

    fn set_leaf_id(&mut self, leaf_id: Option<String>) -> SessionResult<()> {
        if let Some(leaf_id) = &leaf_id {
            if self.inner.entry(leaf_id).is_none() {
                return Err(SessionError::new(
                    SessionErrorCode::NotFound,
                    format!("Entry {leaf_id} not found"),
                ));
            }
        }
        let entry = SessionTreeEntry::Leaf {
            id: self.create_entry_id(),
            parent_id: self.inner.leaf_id()?,
            timestamp: timestamp_string(),
            target_id: leaf_id,
        };
        append_jsonl_entry(&self.file_path, &entry)?;
        self.inner.append_entry(entry)
    }

    fn create_entry_id(&self) -> String {
        self.inner.create_entry_id()
    }

    fn append_entry(&mut self, entry: SessionTreeEntry) -> SessionResult<()> {
        append_jsonl_entry(&self.file_path, &entry)?;
        self.inner.append_entry(entry)
    }

    fn entry(&self, id: &str) -> Option<&SessionTreeEntry> {
        self.inner.entry(id)
    }

    fn entries(&self) -> Vec<SessionTreeEntry> {
        self.inner.entries()
    }

    fn entries_by_type(&self, entry_type: &str) -> Vec<SessionTreeEntry> {
        self.inner.entries_by_type(entry_type)
    }

    fn label(&self, id: &str) -> Option<String> {
        self.inner.label(id)
    }

    fn path_to_root(&self, leaf_id: Option<&str>) -> SessionResult<Vec<SessionTreeEntry>> {
        self.inner.path_to_root(leaf_id)
    }
}

#[derive(Debug, Clone)]
pub struct JsonlSessionRepo {
    sessions_root: PathBuf,
}

impl JsonlSessionRepo {
    pub fn new(sessions_root: impl Into<PathBuf>) -> Self {
        Self {
            sessions_root: sessions_root.into(),
        }
    }

    pub fn create(
        &self,
        options: JsonlSessionCreateOptions,
    ) -> SessionResult<Session<JsonlSessionStorage>> {
        let id = options.id.unwrap_or_else(uuidv7_like);
        let session_dir = self.session_dir(&options.cwd);
        fs::create_dir_all(&session_dir).map_err(|error| {
            SessionError::new(
                SessionErrorCode::Storage,
                format!(
                    "Failed to create session directory {}: {error}",
                    session_dir.display()
                ),
            )
        })?;
        let file_path = session_dir.join(format!(
            "{}_{}.jsonl",
            timestamp_string().replace([':', '.'], "-"),
            id
        ));
        let storage =
            JsonlSessionStorage::create(file_path, options.cwd, id, options.parent_session_path)?;
        Ok(Session::new(storage))
    }

    pub fn open(&self, metadata: SessionMetadata) -> SessionResult<Session<JsonlSessionStorage>> {
        let path = metadata.path.as_ref().ok_or_else(|| {
            SessionError::new(SessionErrorCode::NotFound, "Session path is missing")
        })?;
        if !Path::new(path).exists() {
            return Err(SessionError::new(
                SessionErrorCode::NotFound,
                format!("Session not found: {path}"),
            ));
        }
        Ok(Session::new(JsonlSessionStorage::open(path)?))
    }

    pub fn list(&self, options: JsonlSessionListOptions) -> SessionResult<Vec<SessionMetadata>> {
        let dirs = match options.cwd {
            Some(cwd) => vec![self.session_dir(&cwd)],
            None => self.list_session_dirs()?,
        };
        let mut sessions = Vec::new();
        for dir in dirs {
            if !dir.exists() {
                continue;
            }
            let entries = fs::read_dir(&dir).map_err(|error| {
                SessionError::new(
                    SessionErrorCode::Storage,
                    format!("Failed to list sessions in {}: {error}", dir.display()),
                )
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| {
                    SessionError::new(
                        SessionErrorCode::Storage,
                        format!("Failed to list sessions in {}: {error}", dir.display()),
                    )
                })?;
                let path = entry.path();
                if path.is_file()
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension == "jsonl")
                {
                    match load_jsonl_session_metadata(&path) {
                        Ok(metadata) => sessions.push(metadata),
                        Err(error) if error.code == SessionErrorCode::InvalidSession => {}
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(sessions)
    }

    pub fn delete(&self, metadata: SessionMetadata) -> SessionResult<()> {
        let path = metadata.path.as_ref().ok_or_else(|| {
            SessionError::new(SessionErrorCode::NotFound, "Session path is missing")
        })?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SessionError::new(
                SessionErrorCode::Storage,
                format!("Failed to delete session {path}: {error}"),
            )),
        }
    }

    pub fn fork(
        &self,
        source_metadata: SessionMetadata,
        options: JsonlSessionForkOptions,
    ) -> SessionResult<Session<JsonlSessionStorage>> {
        let source = self.open(source_metadata.clone())?;
        let forked_entries = entries_to_fork(source.storage(), &options)?;
        let mut forked = self.create(JsonlSessionCreateOptions {
            cwd: options.cwd,
            id: options.id,
            parent_session_path: Some(
                options
                    .parent_session_path
                    .or(source_metadata.path)
                    .unwrap_or_default(),
            )
            .filter(|path| !path.is_empty()),
        })?;
        for entry in forked_entries {
            forked.storage_mut().append_entry(entry)?;
        }
        Ok(forked)
    }

    fn session_dir(&self, cwd: &str) -> PathBuf {
        self.sessions_root.join(encode_cwd(cwd))
    }

    fn list_session_dirs(&self) -> SessionResult<Vec<PathBuf>> {
        if !self.sessions_root.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.sessions_root).map_err(|error| {
            SessionError::new(
                SessionErrorCode::Storage,
                format!(
                    "Failed to list sessions root {}: {error}",
                    self.sessions_root.display()
                ),
            )
        })?;
        let mut dirs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                SessionError::new(
                    SessionErrorCode::Storage,
                    format!(
                        "Failed to list sessions root {}: {error}",
                        self.sessions_root.display()
                    ),
                )
            })?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        Ok(dirs)
    }
}

fn encode_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_start_matches(['/', '\\']);
    let encoded = trimmed
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            _ => character,
        })
        .collect::<String>();
    format!("--{encoded}--")
}

fn load_jsonl_session_metadata(path: &Path) -> SessionResult<SessionMetadata> {
    let content = fs::read_to_string(path).map_err(|error| {
        SessionError::new(
            SessionErrorCode::Storage,
            format!("Failed to read session header {}: {error}", path.display()),
        )
    })?;
    let line = content.lines().find(|line| !line.trim().is_empty());
    let Some(line) = line else {
        return Err(SessionError::new(
            SessionErrorCode::InvalidSession,
            format!(
                "Invalid JSONL session file {}: missing session header",
                path.display()
            ),
        ));
    };
    parse_header(line, path)
}

fn entries_to_fork(
    storage: &impl SessionStorage,
    options: &JsonlSessionForkOptions,
) -> SessionResult<Vec<SessionTreeEntry>> {
    let Some(entry_id) = &options.entry_id else {
        return Ok(storage.entries());
    };
    let target = storage.entry(entry_id).ok_or_else(|| {
        SessionError::new(
            SessionErrorCode::InvalidForkTarget,
            format!("Entry {entry_id} not found"),
        )
    })?;
    let effective_leaf_id = match options.position.unwrap_or(ForkPosition::Before) {
        ForkPosition::At => Some(target.id().to_string()),
        ForkPosition::Before => match target {
            SessionTreeEntry::Message {
                parent_id, message, ..
            } if message.role == MessageRole::User => parent_id.clone(),
            _ => {
                return Err(SessionError::new(
                    SessionErrorCode::InvalidForkTarget,
                    format!("Entry {entry_id} is not a user message"),
                ));
            }
        },
    };
    storage.path_to_root(effective_leaf_id.as_deref())
}

pub fn build_session_context(path_entries: &[SessionTreeEntry]) -> SessionResult<SessionContext> {
    let mut thinking_level = ModelThinkingLevel::Off;
    let mut model = None;
    let mut compaction_index = None;
    let mut compaction_first_kept = None;
    let mut messages = Vec::new();

    for (index, entry) in path_entries.iter().enumerate() {
        match entry {
            SessionTreeEntry::ThinkingLevelChange {
                thinking_level: next,
                ..
            } => {
                thinking_level = *next;
            }
            SessionTreeEntry::ModelChange {
                provider, model_id, ..
            } => {
                model = Some((provider.clone(), model_id.clone()));
            }
            SessionTreeEntry::Compaction {
                first_kept_entry_id,
                ..
            } => {
                compaction_index = Some(index);
                compaction_first_kept = Some(first_kept_entry_id.clone());
            }
            _ => {}
        }
    }

    if let Some(index) = compaction_index {
        if let SessionTreeEntry::Compaction {
            summary,
            tokens_before,
            ..
        } = &path_entries[index]
        {
            messages.push(summary_agent_message(
                CompactionSummaryMessage {
                    summary: summary.clone(),
                    tokens_before: *tokens_before,
                    timestamp: 0,
                },
                true,
            ));
        }
        let first_kept = compaction_first_kept.unwrap_or_default();
        let mut found_first_kept = false;
        for entry in path_entries.iter().take(index) {
            if entry.id() == first_kept {
                found_first_kept = true;
            }
            if found_first_kept {
                append_entry_message(&mut messages, entry);
            }
        }
        for entry in path_entries.iter().skip(index + 1) {
            append_entry_message(&mut messages, entry);
        }
    } else {
        for entry in path_entries {
            append_entry_message(&mut messages, entry);
        }
    }

    Ok(SessionContext {
        messages,
        thinking_level,
        model,
    })
}

fn append_entry_message(messages: &mut Vec<AgentMessage>, entry: &SessionTreeEntry) {
    match entry {
        SessionTreeEntry::Message { message, .. } => messages.push(message.clone()),
        SessionTreeEntry::CustomMessage {
            custom_type,
            content,
            display,
            details,
            ..
        } => {
            if *display {
                messages.push(custom_agent_message(CustomMessage {
                    custom_type: custom_type.clone(),
                    content: content.clone(),
                    display: *display,
                    details: details.clone(),
                    timestamp: 0,
                }));
            }
        }
        SessionTreeEntry::BranchSummary {
            from_id, summary, ..
        } => messages.push(summary_agent_message(
            BranchSummaryMessage {
                summary: summary.clone(),
                from_id: from_id.clone(),
                timestamp: 0,
            },
            false,
        )),
        _ => {}
    }
}

fn custom_agent_message(message: CustomMessage) -> AgentMessage {
    AgentMessage::new(MessageRole::User, message.content)
}

fn summary_agent_message<T: SummaryText>(message: T, compaction: bool) -> AgentMessage {
    AgentMessage::new(MessageRole::User, message.summary_text(compaction))
}

trait SummaryText {
    fn summary_text(&self, compaction: bool) -> String;
}

impl SummaryText for BranchSummaryMessage {
    fn summary_text(&self, _compaction: bool) -> String {
        format!(
            "{}{}{}",
            crate::harness::messages::BRANCH_SUMMARY_PREFIX,
            self.summary,
            crate::harness::messages::BRANCH_SUMMARY_SUFFIX
        )
    }
}

impl SummaryText for CompactionSummaryMessage {
    fn summary_text(&self, _compaction: bool) -> String {
        format!(
            "{}{}{}",
            crate::harness::messages::COMPACTION_SUMMARY_PREFIX,
            self.summary,
            crate::harness::messages::COMPACTION_SUMMARY_SUFFIX
        )
    }
}

fn path_to_root(
    by_id: &BTreeMap<String, SessionTreeEntry>,
    leaf_id: Option<&str>,
) -> SessionResult<Vec<SessionTreeEntry>> {
    let Some(leaf_id) = leaf_id else {
        return Ok(Vec::new());
    };

    let mut path = Vec::new();
    let mut current = by_id.get(leaf_id).ok_or_else(|| {
        SessionError::new(
            SessionErrorCode::NotFound,
            format!("Entry {leaf_id} not found"),
        )
    })?;
    loop {
        path.push(current.clone());
        let Some(parent_id) = current.parent_id() else {
            break;
        };
        current = by_id.get(parent_id).ok_or_else(|| {
            SessionError::new(
                SessionErrorCode::InvalidSession,
                format!("Entry {parent_id} not found"),
            )
        })?;
    }
    path.reverse();
    Ok(path)
}

fn update_label_cache(labels_by_id: &mut BTreeMap<String, String>, entry: &SessionTreeEntry) {
    if let SessionTreeEntry::Label {
        target_id, label, ..
    } = entry
    {
        if let Some(label) = label
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            labels_by_id.insert(target_id.clone(), label.to_string());
        } else {
            labels_by_id.remove(target_id);
        }
    }
}

fn entry_type_name(entry: &SessionTreeEntry) -> &'static str {
    match entry {
        SessionTreeEntry::Message { .. } => "message",
        SessionTreeEntry::ThinkingLevelChange { .. } => "thinking_level_change",
        SessionTreeEntry::ModelChange { .. } => "model_change",
        SessionTreeEntry::Compaction { .. } => "compaction",
        SessionTreeEntry::Custom { .. } => "custom",
        SessionTreeEntry::CustomMessage { .. } => "custom_message",
        SessionTreeEntry::Label { .. } => "label",
        SessionTreeEntry::SessionInfo { .. } => "session_info",
        SessionTreeEntry::BranchSummary { .. } => "branch_summary",
        SessionTreeEntry::Leaf { .. } => "leaf",
    }
}

fn write_header(path: &Path, metadata: &SessionMetadata) -> SessionResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SessionError::new(
                SessionErrorCode::Storage,
                format!(
                    "Failed to create session directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let header = serde_json::json!({
        "type": "session",
        "version": 3,
        "id": metadata.id,
        "timestamp": metadata.created_at,
        "cwd": metadata.cwd.clone().unwrap_or_default(),
        "parentSession": metadata.parent_session_path,
    });
    fs::write(path, format!("{header}\n")).map_err(|error| {
        SessionError::new(
            SessionErrorCode::Storage,
            format!("Failed to create session {}: {error}", path.display()),
        )
    })
}

fn append_jsonl_entry(path: &Path, entry: &SessionTreeEntry) -> SessionResult<()> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|error| {
            SessionError::new(
                SessionErrorCode::Storage,
                format!("Failed to open session {}: {error}", path.display()),
            )
        })?;
    let line = serde_json::to_string(entry).map_err(|error| {
        SessionError::new(
            SessionErrorCode::InvalidEntry,
            format!("Failed to encode session entry {}: {error}", entry.id()),
        )
    })?;
    writeln!(file, "{line}").map_err(|error| {
        SessionError::new(
            SessionErrorCode::Storage,
            format!("Failed to append session {}: {error}", path.display()),
        )
    })
}

fn parse_header(line: &str, path: &Path) -> SessionResult<SessionMetadata> {
    let value = parse_header_value(line, path)?;
    let version = session_header_version(&value);
    validate_session_version(version, path)?;
    metadata_from_header_value(&value, path)
}

fn parse_header_value(line: &str, path: &Path) -> SessionResult<serde_json::Value> {
    let value = serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
        SessionError::new(
            SessionErrorCode::InvalidSession,
            format!(
                "Invalid JSONL session file {}: first line is not JSON: {error}",
                path.display()
            ),
        )
    })?;
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session") {
        return Err(SessionError::new(
            SessionErrorCode::InvalidSession,
            format!(
                "Invalid JSONL session file {}: first line is not a session header",
                path.display()
            ),
        ));
    }
    Ok(value)
}

fn session_header_version(value: &serde_json::Value) -> u64 {
    value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1)
}

fn validate_session_version(version: u64, path: &Path) -> SessionResult<()> {
    if !(1..=3).contains(&version) {
        return Err(SessionError::new(
            SessionErrorCode::InvalidSession,
            format!(
                "Invalid JSONL session file {}: unsupported session version",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn metadata_from_header_value(
    value: &serde_json::Value,
    path: &Path,
) -> SessionResult<SessionMetadata> {
    let id = required_string(&value, "id", path)?;
    let created_at = required_string(&value, "timestamp", path)?;
    let cwd = required_string(&value, "cwd", path)?;
    let parent_session_path = value
        .get("parentSession")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);

    Ok(SessionMetadata {
        id,
        created_at,
        cwd: Some(cwd),
        path: Some(path.to_string_lossy().to_string()),
        parent_session_path,
    })
}

fn required_string(value: &serde_json::Value, key: &str, path: &Path) -> SessionResult<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            SessionError::new(
                SessionErrorCode::InvalidSession,
                format!(
                    "Invalid JSONL session file {}: missing {key}",
                    path.display()
                ),
            )
        })
}

fn parse_entry_value(
    line: &str,
    path: &Path,
    line_number: usize,
) -> SessionResult<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
        SessionError::new(
            SessionErrorCode::InvalidEntry,
            format!(
                "Invalid JSONL session file {}: line {line_number} is invalid: {error}",
                path.display()
            ),
        )
    })
}

fn parse_entry_value_as_entry(
    value: serde_json::Value,
    path: &Path,
    line_number: usize,
) -> SessionResult<SessionTreeEntry> {
    serde_json::from_value::<SessionTreeEntry>(value).map_err(|error| {
        SessionError::new(
            SessionErrorCode::InvalidEntry,
            format!(
                "Invalid JSONL session file {}: line {line_number} is invalid: {error}",
                path.display()
            ),
        )
    })
}

fn migrate_session_entry_values(version: u64, entries: &mut [serde_json::Value]) {
    if version < 2 {
        migrate_session_v1_to_v2(entries);
    }
    if version < 3 {
        migrate_session_v2_to_v3(entries);
    }
}

fn migrate_session_v1_to_v2(entries: &mut [serde_json::Value]) {
    let mut ids = BTreeMap::<usize, String>::new();
    let mut prev_id: Option<String> = None;

    for index in 0..entries.len() {
        let id = generate_entry_id(|candidate| ids.values().any(|existing| existing == candidate));
        if let Some(object) = entries[index].as_object_mut() {
            object.insert("id".to_string(), serde_json::Value::String(id.clone()));
            object.insert(
                "parentId".to_string(),
                prev_id
                    .clone()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
            if object.get("type").and_then(serde_json::Value::as_str) == Some("compaction") {
                if let Some(first_kept_index) = object
                    .remove("firstKeptEntryIndex")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| usize::try_from(value).ok())
                {
                    let entry_index = first_kept_index.saturating_sub(1);
                    if let Some(first_kept_id) = ids.get(&entry_index) {
                        object.insert(
                            "firstKeptEntryId".to_string(),
                            serde_json::Value::String(first_kept_id.clone()),
                        );
                    }
                }
            }
        }
        ids.insert(index, id.clone());
        prev_id = Some(id);
    }
}

fn migrate_session_v2_to_v3(entries: &mut [serde_json::Value]) {
    for entry in entries {
        let Some(object) = entry.as_object_mut() else {
            continue;
        };
        if object.get("type").and_then(serde_json::Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = object
            .get_mut("message")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        if message.get("role").and_then(serde_json::Value::as_str) == Some("hookMessage") {
            let content = message
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let details = message.get("details").cloned();
            object.insert(
                "type".to_string(),
                serde_json::Value::String("custom_message".to_string()),
            );
            object.insert(
                "customType".to_string(),
                serde_json::Value::String("hookMessage".to_string()),
            );
            object.insert("content".to_string(), serde_json::Value::String(content));
            object.insert("display".to_string(), serde_json::Value::Bool(true));
            if let Some(details) = details {
                object.insert("details".to_string(), details);
            }
            object.remove("message");
        }
    }
}

fn default_metadata() -> SessionMetadata {
    SessionMetadata {
        id: uuidv7_like(),
        created_at: timestamp_string(),
        cwd: None,
        path: None,
        parent_session_path: None,
    }
}

fn generate_entry_id(has: impl Fn(&str) -> bool) -> String {
    for _ in 0..100 {
        let id = uuidv7_like().chars().take(8).collect::<String>();
        if !has(&id) {
            return id;
        }
    }
    uuidv7_like()
}

fn uuidv7_like() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp:012x}-{counter:04x}-7000-8000-{counter:012x}")
}

fn timestamp_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    millis.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_session_keeps_branch_context_and_labels() {
        let storage = InMemorySessionStorage::default();
        let mut session = Session::new(storage);
        let first = session
            .append_message(AgentMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
                content_blocks: Vec::new(),
                user_content_blocks: Vec::new(),
                tool_call_id: None,
                tool_name: None,
                details: None,
                is_error: false,
                usage: None,
                stop_reason: None,
            })
            .expect("message should append");
        session
            .append_label(first.clone(), Some("start".to_string()))
            .expect("label should append");
        session
            .append_model_change("openai", "gpt-4o")
            .expect("model should append");

        let context = session.build_context().expect("context should build");
        assert_eq!(context.messages.len(), 1);
        assert_eq!(
            context.model,
            Some(("openai".to_string(), "gpt-4o".to_string()))
        );
        assert_eq!(session.storage().label(&first), Some("start".to_string()));
    }

    #[test]
    fn jsonl_session_roundtrips_entries() {
        let path = temp_dir().join("session.jsonl");
        let mut storage = JsonlSessionStorage::create(&path, "/tmp", "session-1", None)
            .expect("session should create");
        storage
            .append_entry(SessionTreeEntry::Message {
                id: "entry-1".to_string(),
                parent_id: None,
                timestamp: "1".to_string(),
                message: AgentMessage {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                    content_blocks: Vec::new(),
                    user_content_blocks: Vec::new(),
                    tool_call_id: None,
                    tool_name: None,
                    details: None,
                    is_error: false,
                    usage: None,
                    stop_reason: None,
                },
            })
            .expect("entry should append");

        let reopened = JsonlSessionStorage::open(&path).expect("session should reopen");
        assert_eq!(reopened.metadata().id, "session-1");
        assert_eq!(reopened.entries().len(), 1);
        assert_eq!(
            reopened.leaf_id().expect("leaf should read"),
            Some("entry-1".to_string())
        );
    }

    #[test]
    fn session_appends_custom_entries_session_name_and_branch_summary() {
        let storage = InMemorySessionStorage::default();
        let mut session = Session::new(storage);
        let user_id = session
            .append_message(AgentMessage::new(MessageRole::User, "question"))
            .expect("message should append");

        let custom_id = session
            .append_custom_entry("tool_state", Some(serde_json::json!({"ok": true})))
            .expect("custom entry should append");
        let custom_message_id = session
            .append_custom_message_entry("notice", "visible", true, None)
            .expect("custom message should append");
        session
            .append_session_name("  Planning Session  ")
            .expect("session name should append");
        let branch_summary_id = session
            .move_to(
                Some(user_id.clone()),
                Some(BranchSummaryOptions {
                    summary: "earlier branch".to_string(),
                    details: None,
                    from_hook: Some(true),
                }),
            )
            .expect("move should succeed")
            .expect("summary should append");

        assert!(matches!(
            session.storage().entry(&custom_id),
            Some(SessionTreeEntry::Custom {
                custom_type,
                data: Some(data),
                ..
            }) if custom_type == "tool_state" && data["ok"] == true
        ));
        assert_eq!(session.session_name(), Some("Planning Session".to_string()));

        let context = session.build_context().expect("context should build");
        assert_eq!(context.messages.len(), 2);
        assert_eq!(context.messages[0].content, "question");
        assert!(context.messages[1].content.contains("earlier branch"));
        assert!(matches!(
            session.storage().entry(&branch_summary_id),
            Some(SessionTreeEntry::BranchSummary {
                parent_id,
                from_id,
                from_hook,
                ..
            }) if parent_id.as_deref() == Some(user_id.as_str())
                && from_id == &user_id
                && *from_hook
        ));
        assert!(matches!(
            session.storage().entry(&custom_message_id),
            Some(SessionTreeEntry::CustomMessage { display, .. }) if *display
        ));
    }

    #[test]
    fn jsonl_session_repo_creates_lists_opens_deletes_and_forks_sessions() {
        let root = temp_dir().join("sessions");
        let repo = JsonlSessionRepo::new(&root);
        let cwd = "/tmp/pm-agent";

        let mut first = repo
            .create(JsonlSessionCreateOptions {
                cwd: cwd.to_string(),
                id: Some("session-a".to_string()),
                parent_session_path: None,
            })
            .expect("session should create");
        let first_entry = first
            .append_message(AgentMessage::new(MessageRole::User, "first"))
            .expect("message should append");

        let session_dir = root.join("--tmp-pm-agent--");
        let older_path = session_dir.join("2026-01-01T00-00-00-000Z_session-old.jsonl");
        let newer_path = session_dir.join("2026-01-02T00-00-00-000Z_session-new.jsonl");
        fs::write(
            &older_path,
            r#"{"type":"session","version":3,"id":"session-old","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/tmp/pm-agent"}"#.to_string()
                + "\n",
        )
        .expect("older fixture should write");
        fs::write(
            &newer_path,
            r#"{"type":"session","version":3,"id":"session-new","timestamp":"2026-01-02T00:00:00.000Z","cwd":"/tmp/pm-agent"}"#.to_string()
                + "\n",
        )
        .expect("newer fixture should write");
        let all_for_cwd = repo
            .list(JsonlSessionListOptions {
                cwd: Some(cwd.to_string()),
            })
            .expect("sessions should list");

        assert_eq!(all_for_cwd.len(), 3);
        assert_eq!(all_for_cwd[0].id, "session-new");
        assert_eq!(all_for_cwd[1].id, "session-old");
        assert!(first
            .metadata()
            .path
            .as_deref()
            .expect("path should be present")
            .contains("--tmp-pm-agent--"));

        let reopened = repo
            .open(first.metadata().clone())
            .expect("session should reopen");
        assert_eq!(
            reopened
                .branch(None)
                .expect("branch should read")
                .first()
                .expect("entry should exist")
                .id(),
            first_entry
        );

        let forked = repo
            .fork(
                first.metadata().clone(),
                JsonlSessionForkOptions {
                    cwd: cwd.to_string(),
                    id: Some("session-fork".to_string()),
                    parent_session_path: None,
                    entry_id: Some(first_entry.clone()),
                    position: Some(ForkPosition::At),
                },
            )
            .expect("session should fork");
        assert_eq!(
            forked.metadata().parent_session_path,
            first.metadata().path.clone()
        );
        assert_eq!(
            forked.branch(None).expect("fork branch should read").len(),
            1
        );

        repo.delete(SessionMetadata {
            id: "session-new".to_string(),
            created_at: "2026-01-02T00:00:00.000Z".to_string(),
            cwd: Some(cwd.to_string()),
            path: Some(newer_path.to_string_lossy().to_string()),
            parent_session_path: None,
        })
        .expect("delete should remove file");
        assert!(matches!(
            repo.open(SessionMetadata {
                id: "session-new".to_string(),
                created_at: "2026-01-02T00:00:00.000Z".to_string(),
                cwd: Some(cwd.to_string()),
                path: Some(newer_path.to_string_lossy().to_string()),
                parent_session_path: None,
            }),
            Err(error) if error.code == SessionErrorCode::NotFound
        ));
    }

    #[test]
    fn jsonl_session_open_accepts_v2_sessions_like_pi_migration() {
        let path = temp_dir().join("v2-session.jsonl");
        fs::write(
            &path,
            [
                r#"{"type":"session","version":2,"id":"session-v2","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/tmp/pm-agent"}"#,
                r#"{"type":"message","id":"entry-1","parentId":null,"timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"User","content":"legacy user"}}"#,
            ]
            .join("\n")
                + "\n",
        )
        .expect("v2 fixture should write");

        let storage = JsonlSessionStorage::open(&path).expect("v2 session should migrate");
        let entries = storage.entries();

        assert_eq!(storage.metadata().id, "session-v2");
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            SessionTreeEntry::Message { message, .. }
                if message.role == MessageRole::User && message.content == "legacy user"
        ));
        fs::remove_file(path).ok();
    }

    #[test]
    fn jsonl_session_open_converts_legacy_hook_messages_to_custom_messages() {
        let path = temp_dir().join("v2-hook-session.jsonl");
        fs::write(
            &path,
            [
                r#"{"type":"session","version":2,"id":"session-v2-hook","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/tmp/pm-agent"}"#,
                r#"{"type":"message","id":"entry-1","parentId":null,"timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"hookMessage","content":"legacy hook"}}"#,
            ]
            .join("\n")
                + "\n",
        )
        .expect("v2 hook fixture should write");

        let storage = JsonlSessionStorage::open(&path).expect("v2 hook message should migrate");
        let entries = storage.entries();

        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            SessionTreeEntry::CustomMessage { custom_type, content, display, .. }
                if custom_type == "hookMessage" && content == "legacy hook" && *display
        ));
        fs::remove_file(path).ok();
    }

    #[test]
    fn jsonl_session_open_migrates_v1_ids_parent_chain_and_compaction_index_like_pi() {
        let path = temp_dir().join("v1-session.jsonl");
        fs::write(
            &path,
            [
                r#"{"type":"session","version":1,"id":"session-v1","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/tmp/pm-agent"}"#,
                r#"{"type":"message","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"User","content":"first"}}"#,
                r#"{"type":"message","timestamp":"2026-01-01T00:00:02.000Z","message":{"role":"Assistant","content":"second"}}"#,
                r#"{"type":"compaction","timestamp":"2026-01-01T00:00:03.000Z","summary":"summary","firstKeptEntryIndex":1,"tokensBefore":42,"fromHook":false}"#,
            ]
            .join("\n")
                + "\n",
        )
        .expect("v1 fixture should write");

        let storage = JsonlSessionStorage::open(&path).expect("v1 session should migrate");
        let entries = storage.entries();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].parent_id(), None);
        assert_eq!(entries[1].parent_id(), Some(entries[0].id()));
        assert_eq!(entries[2].parent_id(), Some(entries[1].id()));
        assert!(matches!(
            &entries[2],
            SessionTreeEntry::Compaction { first_kept_entry_id, .. }
                if first_kept_entry_id == entries[0].id()
        ));
        fs::remove_file(path).ok();
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-jsonl-session-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
