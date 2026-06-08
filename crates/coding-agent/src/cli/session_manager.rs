use super::CliArgs;
use crate::session_manager::{resolve_session_path, ResolvedSession, SessionManager};
use agent::harness::{InMemorySessionStorage, JsonlSessionStorage};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliSessionError {
    ForkFlagConflict { flags: Vec<&'static str> },
    SessionNotFound { arg: String },
    GlobalSessionNeedsConfirmation { path: String, cwd: String },
    Storage(String),
}

pub enum CliSessionManager {
    Memory(SessionManager<InMemorySessionStorage>),
    Persisted(SessionManager<JsonlSessionStorage>),
}

impl CliSessionManager {
    pub fn session_file(&self) -> Option<&Path> {
        match self {
            Self::Memory(manager) => manager.session_file(),
            Self::Persisted(manager) => manager.session_file(),
        }
    }

    pub fn is_persisted(&self) -> bool {
        matches!(self, Self::Persisted(_))
    }

    pub fn cwd(&self) -> &Path {
        match self {
            Self::Memory(manager) => manager.cwd(),
            Self::Persisted(manager) => manager.cwd(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalSessionPolicy {
    RequireConfirmation,
    Fork,
}

pub fn validate_fork_flags(parsed: &CliArgs) -> Result<(), CliSessionError> {
    if parsed.fork.is_none() {
        return Ok(());
    }

    let mut conflicting_flags = Vec::new();
    if parsed.session.is_some() {
        conflicting_flags.push("--session");
    }
    if parsed.continue_session {
        conflicting_flags.push("--continue");
    }
    if parsed.resume {
        conflicting_flags.push("--resume");
    }
    if parsed.no_session {
        conflicting_flags.push("--no-session");
    }

    if conflicting_flags.is_empty() {
        Ok(())
    } else {
        Err(CliSessionError::ForkFlagConflict {
            flags: conflicting_flags,
        })
    }
}

pub fn create_cli_session_manager(
    parsed: &CliArgs,
    cwd: impl AsRef<Path>,
    sessions_root: Option<&Path>,
    global_session_policy: GlobalSessionPolicy,
) -> Result<CliSessionManager, CliSessionError> {
    validate_fork_flags(parsed)?;

    let cwd = cwd.as_ref();
    let session_dir = parsed.session_dir.as_ref().map(PathBuf::from);

    if parsed.no_session {
        return Ok(CliSessionManager::Memory(SessionManager::in_memory(
            cwd.to_path_buf(),
        )));
    }

    if let Some(fork_arg) = &parsed.fork {
        let resolved = resolve_session_path(fork_arg, cwd, session_dir.as_deref(), sessions_root);
        let source_path = resolved_session_path_or_error(resolved)?;
        return SessionManager::fork_from(source_path, cwd, session_dir)
            .map(CliSessionManager::Persisted)
            .map_err(CliSessionError::Storage);
    }

    if let Some(session_arg) = &parsed.session {
        let resolved =
            resolve_session_path(session_arg, cwd, session_dir.as_deref(), sessions_root);
        return match resolved {
            ResolvedSession::Path { path } | ResolvedSession::Local { path } => {
                SessionManager::open(path, session_dir)
                    .map(CliSessionManager::Persisted)
                    .map_err(CliSessionError::Storage)
            }
            ResolvedSession::Global {
                path,
                cwd: source_cwd,
            } => match global_session_policy {
                GlobalSessionPolicy::RequireConfirmation => {
                    Err(CliSessionError::GlobalSessionNeedsConfirmation {
                        path,
                        cwd: source_cwd,
                    })
                }
                GlobalSessionPolicy::Fork => SessionManager::fork_from(path, cwd, session_dir)
                    .map(CliSessionManager::Persisted)
                    .map_err(CliSessionError::Storage),
            },
            ResolvedSession::NotFound { arg } => Err(CliSessionError::SessionNotFound { arg }),
        };
    }

    if parsed.resume {
        return SessionManager::continue_recent(cwd, session_dir)
            .map(CliSessionManager::Persisted)
            .map_err(CliSessionError::Storage);
    }

    if parsed.continue_session {
        return SessionManager::continue_recent(cwd, session_dir)
            .map(CliSessionManager::Persisted)
            .map_err(CliSessionError::Storage);
    }

    SessionManager::create(cwd, session_dir)
        .map(CliSessionManager::Persisted)
        .map_err(CliSessionError::Storage)
}

fn resolved_session_path_or_error(resolved: ResolvedSession) -> Result<String, CliSessionError> {
    match resolved {
        ResolvedSession::Path { path }
        | ResolvedSession::Local { path }
        | ResolvedSession::Global { path, .. } => Ok(path),
        ResolvedSession::NotFound { arg } => Err(CliSessionError::SessionNotFound { arg }),
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
    fn rejects_fork_conflicting_flags_like_pi() {
        let parsed = CliArgs {
            fork: Some("abc".to_string()),
            session: Some("def".to_string()),
            continue_session: true,
            ..CliArgs::default()
        };

        assert_eq!(
            validate_fork_flags(&parsed),
            Err(CliSessionError::ForkFlagConflict {
                flags: vec!["--session", "--continue"],
            })
        );
    }

    #[test]
    fn creates_memory_session_for_no_session() {
        let manager = create_cli_session_manager(
            &CliArgs {
                no_session: true,
                ..CliArgs::default()
            },
            "/tmp/project",
            None,
            GlobalSessionPolicy::RequireConfirmation,
        )
        .expect("manager should create");

        assert!(!manager.is_persisted());
        assert_eq!(manager.cwd(), Path::new("/tmp/project"));
    }

    #[test]
    fn opens_local_session_from_session_argument() {
        let session_dir = temp_dir("local");
        let mut source = SessionManager::create("/tmp/project", Some(session_dir.clone()))
            .expect("source session should create");
        source
            .append_message(AgentMessage::new(MessageRole::User, "hello".to_string()))
            .expect("message should append");
        let prefix = source.session_id()[..4].to_string();

        let manager = create_cli_session_manager(
            &CliArgs {
                session: Some(prefix),
                session_dir: Some(session_dir.to_string_lossy().to_string()),
                ..CliArgs::default()
            },
            "/tmp/project",
            None,
            GlobalSessionPolicy::RequireConfirmation,
        )
        .expect("session should open");

        assert_eq!(manager.session_file(), source.session_file());
    }

    #[test]
    fn forks_session_from_fork_argument() {
        let source_dir = temp_dir("source");
        let target_dir = temp_dir("target");
        let mut source = SessionManager::create("/tmp/source", Some(source_dir.clone()))
            .expect("source session should create");
        source
            .append_message(AgentMessage::new(
                MessageRole::User,
                "source message".to_string(),
            ))
            .expect("source message should append");
        let source_file = source
            .session_file()
            .expect("source file should exist")
            .to_string_lossy()
            .to_string();

        let manager = create_cli_session_manager(
            &CliArgs {
                fork: Some(source_file.clone()),
                session_dir: Some(target_dir.to_string_lossy().to_string()),
                ..CliArgs::default()
            },
            "/tmp/target",
            None,
            GlobalSessionPolicy::RequireConfirmation,
        )
        .expect("session should fork");

        assert!(manager.is_persisted());
        assert_ne!(
            manager
                .session_file()
                .expect("forked file should exist")
                .to_string_lossy(),
            source_file
        );
    }

    #[test]
    fn returns_confirmation_request_for_global_session() {
        let local_dir = temp_dir("local");
        let global_root = temp_dir("global-root");
        let global_dir = global_root.join("other");
        fs::create_dir_all(&global_dir).expect("global dir should be created");
        let global = SessionManager::create("/tmp/other", Some(global_dir))
            .expect("global session should create");
        let prefix = global.session_id()[..4].to_string();

        let error = match create_cli_session_manager(
            &CliArgs {
                session: Some(prefix),
                session_dir: Some(local_dir.to_string_lossy().to_string()),
                ..CliArgs::default()
            },
            "/tmp/project",
            Some(&global_root),
            GlobalSessionPolicy::RequireConfirmation,
        ) {
            Ok(_) => panic!("global session should require confirmation"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            CliSessionError::GlobalSessionNeedsConfirmation {
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
    fn confirmed_global_session_forks_into_current_cwd() {
        let local_dir = temp_dir("local");
        let global_root = temp_dir("global-root");
        let global_dir = global_root.join("other");
        fs::create_dir_all(&global_dir).expect("global dir should be created");
        let global = SessionManager::create("/tmp/other", Some(global_dir))
            .expect("global session should create");
        let prefix = global.session_id()[..4].to_string();

        let manager = create_cli_session_manager(
            &CliArgs {
                session: Some(prefix),
                session_dir: Some(local_dir.to_string_lossy().to_string()),
                ..CliArgs::default()
            },
            "/tmp/current",
            Some(&global_root),
            GlobalSessionPolicy::Fork,
        )
        .expect("global session should fork after confirmation");

        assert_eq!(manager.cwd(), Path::new("/tmp/current"));
    }

    #[test]
    fn resume_opens_most_recent_local_session() {
        let session_dir = temp_dir("resume-local");
        let mut older = SessionManager::create("/tmp/project", Some(session_dir.clone()))
            .expect("older session should create");
        older
            .append_message(AgentMessage::new(MessageRole::User, "older".to_string()))
            .expect("older message should append");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let mut newer = SessionManager::create("/tmp/project", Some(session_dir.clone()))
            .expect("newer session should create");
        newer
            .append_message(AgentMessage::new(MessageRole::User, "newer".to_string()))
            .expect("newer message should append");

        let manager = create_cli_session_manager(
            &CliArgs {
                resume: true,
                session_dir: Some(session_dir.to_string_lossy().to_string()),
                ..CliArgs::default()
            },
            "/tmp/project",
            None,
            GlobalSessionPolicy::RequireConfirmation,
        )
        .expect("resume should open most recent local session");

        assert_eq!(manager.session_file(), newer.session_file());
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-cli-session-{label}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
