use std::path::{Path, PathBuf};
use std::process::Command;

use crate::utils::{share_viewer_url, AppConfigPaths};

pub const GH_NOT_LOGGED_IN: &str = "GitHub CLI is not logged in. Run 'gh auth login' first.";
pub const GH_NOT_INSTALLED: &str =
    "GitHub CLI (gh) is not installed. Install it from https://cli.github.com/";
pub const GIST_ID_PARSE_FAILED: &str = "Failed to parse gist ID from gh output";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareSessionResult {
    pub gist_url: String,
    pub gist_id: String,
    pub preview_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: Option<i32>,
}

pub trait ShareCommandRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<ShareCommandOutput, String>;
}

#[derive(Debug, Default)]
pub struct SystemShareCommandRunner;

impl ShareCommandRunner for SystemShareCommandRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<ShareCommandOutput, String> {
        let output = Command::new(program).args(args).output().map_err(|error| {
            if program == "gh" {
                GH_NOT_INSTALLED.to_string()
            } else {
                error.to_string()
            }
        })?;
        Ok(ShareCommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            status: output.status.code(),
        })
    }
}

pub fn share_session_html(
    html_path: &Path,
    config: &AppConfigPaths,
) -> Result<ShareSessionResult, String> {
    let mut runner = SystemShareCommandRunner;
    share_session_html_with_runner(html_path, config, &mut runner)
}

pub fn share_session_html_with_runner(
    html_path: &Path,
    config: &AppConfigPaths,
    runner: &mut dyn ShareCommandRunner,
) -> Result<ShareSessionResult, String> {
    let auth = runner.run("gh", &["auth".to_string(), "status".to_string()])?;
    if auth.status != Some(0) {
        return Err(GH_NOT_LOGGED_IN.to_string());
    }

    let path = html_path.to_string_lossy().to_string();
    let gist = runner.run(
        "gh",
        &[
            "gist".to_string(),
            "create".to_string(),
            "--public=false".to_string(),
            path,
        ],
    )?;
    if gist.status != Some(0) {
        let error = gist.stderr.trim();
        return Err(format!(
            "Failed to create gist: {}",
            if error.is_empty() {
                "Unknown error"
            } else {
                error
            }
        ));
    }

    let gist_url = gist.stdout.trim().to_string();
    let gist_id = parse_gist_id(&gist_url).ok_or_else(|| GIST_ID_PARSE_FAILED.to_string())?;
    let preview_url = share_viewer_url(config, &gist_id);
    Ok(ShareSessionResult {
        gist_url,
        gist_id,
        preview_url,
    })
}

fn parse_gist_id(gist_url: &str) -> Option<String> {
    let id = gist_url.trim().trim_end_matches('/').rsplit('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

pub fn temp_share_html_path() -> PathBuf {
    std::env::temp_dir().join("session.html")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct RecordingShareRunner {
        outputs: Vec<Result<ShareCommandOutput, String>>,
        calls: Vec<(String, Vec<String>)>,
    }

    impl RecordingShareRunner {
        fn new(outputs: Vec<Result<ShareCommandOutput, String>>) -> Self {
            Self {
                outputs,
                calls: Vec::new(),
            }
        }
    }

    impl ShareCommandRunner for RecordingShareRunner {
        fn run(&mut self, program: &str, args: &[String]) -> Result<ShareCommandOutput, String> {
            self.calls.push((program.to_string(), args.to_vec()));
            self.outputs.remove(0)
        }
    }

    #[test]
    fn share_session_creates_secret_gist_and_preview_url_like_pi() {
        let mut config = AppConfigPaths::new("/home/alice");
        config.share_viewer_url_value = Some("https://viewer.test/session/".to_string());
        let mut runner = RecordingShareRunner::new(vec![
            Ok(output("", "", Some(0))),
            Ok(output(
                "https://gist.github.com/alice/abc123\n",
                "",
                Some(0),
            )),
        ]);

        let result =
            share_session_html_with_runner(Path::new("/tmp/session.html"), &config, &mut runner)
                .expect("share");

        assert_eq!(result.gist_url, "https://gist.github.com/alice/abc123");
        assert_eq!(result.gist_id, "abc123");
        assert_eq!(result.preview_url, "https://viewer.test/session/#abc123");
        assert_eq!(
            runner.calls,
            vec![
                (
                    "gh".to_string(),
                    vec!["auth".to_string(), "status".to_string()]
                ),
                (
                    "gh".to_string(),
                    vec![
                        "gist".to_string(),
                        "create".to_string(),
                        "--public=false".to_string(),
                        "/tmp/session.html".to_string(),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn share_session_reports_pi_auth_error_without_creating_gist() {
        let config = AppConfigPaths::new("/home/alice");
        let mut runner = RecordingShareRunner::new(vec![Ok(output("", "login required", Some(1)))]);

        let error =
            share_session_html_with_runner(Path::new("/tmp/session.html"), &config, &mut runner)
                .expect_err("auth should fail");

        assert_eq!(error, GH_NOT_LOGGED_IN);
        assert_eq!(runner.calls.len(), 1);
    }

    #[test]
    fn share_session_reports_gist_create_stderr_like_pi() {
        let config = AppConfigPaths::new("/home/alice");
        let mut runner = RecordingShareRunner::new(vec![
            Ok(output("", "", Some(0))),
            Ok(output("", "no scopes", Some(1))),
        ]);

        let error =
            share_session_html_with_runner(Path::new("/tmp/session.html"), &config, &mut runner)
                .expect_err("gist should fail");

        assert_eq!(error, "Failed to create gist: no scopes");
    }

    #[test]
    fn share_session_reports_parse_error_like_pi() {
        let config = AppConfigPaths::new("/home/alice");
        let mut runner = RecordingShareRunner::new(vec![
            Ok(output("", "", Some(0))),
            Ok(output("", "", Some(0))),
        ]);

        let error =
            share_session_html_with_runner(Path::new("/tmp/session.html"), &config, &mut runner)
                .expect_err("parse should fail");

        assert_eq!(error, GIST_ID_PARSE_FAILED);
    }

    #[test]
    fn share_session_maps_missing_gh_error_like_pi() {
        let config = AppConfigPaths::new("/home/alice");
        let mut runner = RecordingShareRunner::new(vec![Err(GH_NOT_INSTALLED.to_string())]);

        let error =
            share_session_html_with_runner(Path::new("/tmp/session.html"), &config, &mut runner)
                .expect_err("missing gh should fail");

        assert_eq!(error, GH_NOT_INSTALLED);
    }

    fn output(stdout: &str, stderr: &str, status: Option<i32>) -> ShareCommandOutput {
        ShareCommandOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            status,
        }
    }
}
