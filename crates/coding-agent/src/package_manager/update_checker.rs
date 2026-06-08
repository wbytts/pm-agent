use super::types::NpmCommandConfig;
use super::updates::UpdateCheck;
use crate::exec::{exec_command, ExecOptions};
use std::path::Path;

const NETWORK_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone)]
pub struct CommandUpdateChecker {
    cwd: String,
    npm_command: NpmCommandConfig,
}

impl CommandUpdateChecker {
    pub fn new(cwd: impl AsRef<Path>, npm_command: Option<NpmCommandConfig>) -> Self {
        Self {
            cwd: cwd.as_ref().to_string_lossy().to_string(),
            npm_command: npm_command.unwrap_or_default(),
        }
    }

    fn run_capture(
        &self,
        command: &str,
        args: &[String],
        cwd: &Path,
        env: Vec<(String, String)>,
    ) -> Result<String, String> {
        let result = exec_command(
            command,
            args,
            &self.cwd,
            Some(ExecOptions {
                timeout_ms: Some(NETWORK_TIMEOUT_MS),
                cwd: Some(cwd.to_string_lossy().to_string()),
                env,
            }),
        )?;
        if result.code != 0 {
            return Err(format!(
                "命令执行失败：{} {}，退出码 {}{}",
                command,
                args.join(" "),
                result.code,
                stderr_suffix(&result.stderr)
            ));
        }
        Ok(result.stdout)
    }

    fn git_upstream_ref(&self, installed_path: &Path) -> Option<String> {
        let output = self
            .run_capture(
                "git",
                &[
                    "rev-parse".to_string(),
                    "--abbrev-ref".to_string(),
                    "@{upstream}".to_string(),
                ],
                installed_path,
                Vec::new(),
            )
            .ok()?;
        let trimmed = output.trim();
        let branch = trimmed.strip_prefix("origin/")?;
        (!branch.is_empty()).then(|| format!("refs/heads/{branch}"))
    }

    fn git_remote_command(&self, installed_path: &Path, args: &[String]) -> Result<String, String> {
        self.run_capture(
            "git",
            args,
            installed_path,
            vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())],
        )
    }
}

impl UpdateCheck for CommandUpdateChecker {
    fn latest_npm_version(&self, package_name: &str) -> Result<Option<String>, String> {
        let mut args = self.npm_command.args.clone();
        args.extend([
            "view".to_string(),
            package_name.to_string(),
            "version".to_string(),
            "--json".to_string(),
        ]);
        let stdout = self.run_capture(
            &self.npm_command.command,
            &args,
            Path::new(&self.cwd),
            Vec::new(),
        )?;
        let raw = stdout.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        serde_json::from_str::<String>(raw)
            .map(Some)
            .map_err(|error| format!("解析 npm version 失败：{error}"))
    }

    fn remote_git_head(&self, installed_path: &Path) -> Result<Option<String>, String> {
        if let Some(upstream_ref) = self.git_upstream_ref(installed_path) {
            let output = self.git_remote_command(
                installed_path,
                &["ls-remote".to_string(), "origin".to_string(), upstream_ref],
            )?;
            if let Some(head) = first_git_head(&output) {
                return Ok(Some(head));
            }
        }

        let output = self.git_remote_command(
            installed_path,
            &[
                "ls-remote".to_string(),
                "origin".to_string(),
                "HEAD".to_string(),
            ],
        )?;
        Ok(first_git_head(&output))
    }
}

fn first_git_head(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (head, _) = line.split_once(char::is_whitespace)?;
        (head.len() == 40 && head.chars().all(|ch| ch.is_ascii_hexdigit()))
            .then(|| head.to_string())
    })
}

fn stderr_suffix(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("，错误输出：{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_git_head_from_ls_remote_output() {
        assert_eq!(
            first_git_head("0123456789abcdef0123456789abcdef01234567\tHEAD\n"),
            Some("0123456789abcdef0123456789abcdef01234567".to_string())
        );
    }

    #[test]
    fn rejects_invalid_git_head_output() {
        assert_eq!(first_git_head("not-a-head\tHEAD\n"), None);
    }
}
