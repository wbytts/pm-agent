use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCwdIssue {
    pub session_file: Option<PathBuf>,
    pub session_cwd: PathBuf,
    pub fallback_cwd: PathBuf,
}

pub trait SessionCwdSource {
    fn cwd(&self) -> &Path;
    fn session_file(&self) -> Option<&Path>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingSessionCwdError {
    pub issue: SessionCwdIssue,
}

impl std::fmt::Display for MissingSessionCwdError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&format_missing_session_cwd_error(&self.issue))
    }
}

impl std::error::Error for MissingSessionCwdError {}

pub fn get_missing_session_cwd_issue(
    session: &impl SessionCwdSource,
    fallback_cwd: impl Into<PathBuf>,
) -> Option<SessionCwdIssue> {
    let session_file = session.session_file()?;
    let session_cwd = session.cwd();
    if session_cwd.as_os_str().is_empty() || session_cwd.exists() {
        return None;
    }

    Some(SessionCwdIssue {
        session_file: Some(session_file.to_path_buf()),
        session_cwd: session_cwd.to_path_buf(),
        fallback_cwd: fallback_cwd.into(),
    })
}

pub fn format_missing_session_cwd_error(issue: &SessionCwdIssue) -> String {
    let session_file = issue
        .session_file
        .as_ref()
        .map(|path| format!("\nSession file: {}", path.display()))
        .unwrap_or_default();
    format!(
        "Stored session working directory does not exist: {}{session_file}\nCurrent working directory: {}",
        issue.session_cwd.display(),
        issue.fallback_cwd.display()
    )
}

pub fn format_missing_session_cwd_prompt(issue: &SessionCwdIssue) -> String {
    format!(
        "cwd from session file does not exist\n{}\n\ncontinue in current cwd\n{}",
        issue.session_cwd.display(),
        issue.fallback_cwd.display()
    )
}

pub fn assert_session_cwd_exists(
    session: &impl SessionCwdSource,
    fallback_cwd: impl Into<PathBuf>,
) -> Result<(), MissingSessionCwdError> {
    if let Some(issue) = get_missing_session_cwd_issue(session, fallback_cwd) {
        return Err(MissingSessionCwdError { issue });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSession {
        cwd: PathBuf,
        session_file: Option<PathBuf>,
    }

    impl SessionCwdSource for TestSession {
        fn cwd(&self) -> &Path {
            &self.cwd
        }

        fn session_file(&self) -> Option<&Path> {
            self.session_file.as_deref()
        }
    }

    #[test]
    fn reports_missing_session_cwd_when_session_file_exists() {
        let session = TestSession {
            cwd: PathBuf::from("/definitely/missing/pm-agent-session-cwd"),
            session_file: Some(PathBuf::from("/tmp/session.jsonl")),
        };
        let issue = get_missing_session_cwd_issue(&session, "/fallback").expect("issue");
        assert_eq!(
            issue.session_file.as_deref(),
            Some(Path::new("/tmp/session.jsonl"))
        );
        assert!(
            format_missing_session_cwd_error(&issue).contains("Stored session working directory")
        );
        assert!(format_missing_session_cwd_prompt(&issue).contains("continue in current cwd"));
    }

    #[test]
    fn ignores_missing_cwd_without_session_file() {
        let session = TestSession {
            cwd: PathBuf::from("/definitely/missing/pm-agent-session-cwd"),
            session_file: None,
        };
        assert!(get_missing_session_cwd_issue(&session, "/fallback").is_none());
    }
}
