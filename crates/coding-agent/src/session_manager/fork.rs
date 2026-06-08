use super::{default_session_dir, session_id, timestamp_string, SessionManager};
use agent::harness::{JsonlSessionStorage, SessionStorage};
use std::path::{Path, PathBuf};

impl SessionManager<JsonlSessionStorage> {
    pub fn fork_from(
        source_path: impl Into<PathBuf>,
        target_cwd: impl Into<PathBuf>,
        session_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let source_path = source_path.into();
        let target_cwd = target_cwd.into();
        let source_storage =
            JsonlSessionStorage::open(&source_path).map_err(|error| error.to_string())?;
        let source_entries = source_storage.entries();
        let session_dir = session_dir.unwrap_or_else(|| default_session_dir(&target_cwd));
        let new_session_id = session_id();
        let session_file =
            session_dir.join(format!("{}_{}.jsonl", timestamp_string(), new_session_id));
        let mut storage = JsonlSessionStorage::create(
            &session_file,
            target_cwd.to_string_lossy().to_string(),
            new_session_id,
            Some(source_path.to_string_lossy().to_string()),
        )
        .map_err(|error| error.to_string())?;

        for entry in source_entries {
            storage
                .append_entry(entry)
                .map_err(|error| error.to_string())?;
        }

        Ok(Self {
            storage,
            cwd: target_cwd,
            session_dir,
            session_file: Some(session_file),
            persist: true,
        })
    }

    pub fn open_with_cwd(
        path: impl Into<PathBuf>,
        session_dir: Option<PathBuf>,
        cwd_override: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let session_file = path.into();
        let cwd = cwd_override.into();
        let storage =
            JsonlSessionStorage::open(&session_file).map_err(|error| error.to_string())?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::AgentMessage;
    use ai::MessageRole;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn forks_session_into_target_cwd_with_parent_pointer() {
        let source_dir = temp_dir("source");
        let target_dir = temp_dir("target");
        let mut source = SessionManager::create("/tmp/source-project", Some(source_dir))
            .expect("source session should create");
        source
            .append_message(AgentMessage::new(
                MessageRole::User,
                "source request".to_string(),
            ))
            .expect("source message should append");
        let source_file = source
            .session_file()
            .expect("source file should exist")
            .to_path_buf();

        let forked =
            SessionManager::fork_from(&source_file, "/tmp/target-project", Some(target_dir))
                .expect("session should fork");

        assert_eq!(forked.cwd(), Path::new("/tmp/target-project"));
        assert_eq!(
            forked.storage_metadata().parent_session_path.as_deref(),
            Some(source_file.to_string_lossy().as_ref())
        );
        assert_eq!(
            forked
                .build_context()
                .expect("context should build")
                .messages
                .len(),
            1
        );
        assert_eq!(
            forked
                .build_context()
                .expect("context should build")
                .messages[0]
                .content,
            "source request"
        );
    }

    #[test]
    fn open_with_cwd_overrides_header_cwd() {
        let dir = temp_dir("open");
        let source = SessionManager::create("/tmp/source-project", Some(dir.clone()))
            .expect("session should create");
        let opened = SessionManager::open_with_cwd(
            source.session_file().expect("session file should exist"),
            Some(dir),
            "/tmp/override",
        )
        .expect("session should open");

        assert_eq!(opened.cwd(), Path::new("/tmp/override"));
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-session-fork-{label}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
