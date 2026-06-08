use std::io;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpawnOutput {
    pub error: Option<String>,
    pub stderr: String,
    pub stdout: String,
    pub status: Option<i32>,
}

pub fn format_spawn_failure(output: &SpawnOutput) -> String {
    if let Some(error) = output.error.as_ref().filter(|value| !value.is_empty()) {
        return error.clone();
    }

    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    match output.status {
        Some(status) => format!("exit status {status}"),
        None => "exit status unknown".to_string(),
    }
}

pub fn run_sync_command(command: &str, args: &[&str]) -> io::Result<SpawnOutput> {
    match Command::new(command).args(args).output() {
        Ok(output) => Ok(SpawnOutput {
            error: None,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            status: output.status.code(),
        }),
        Err(error) => Ok(SpawnOutput {
            error: Some(error.to_string()),
            stderr: String::new(),
            stdout: String::new(),
            status: None,
        }),
    }
}

pub fn run_extraction_command(command: &str, args: &[&str]) -> io::Result<Option<String>> {
    let output = run_sync_command(command, args)?;
    if output.error.is_none() && output.status == Some(0) {
        return Ok(None);
    }
    Ok(Some(format!(
        "{command}: {}",
        format_spawn_failure(&output)
    )))
}

pub fn find_binary_recursively(
    root_dir: impl AsRef<std::path::Path>,
    binary_file_name: &str,
) -> Option<PathBuf> {
    let mut stack = vec![root_dir.as_ref().to_path_buf()];
    while let Some(current_dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current_dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == binary_file_name)
            {
                return Some(path);
            }
            if file_type.is_dir() {
                stack.push(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_spawn_failure_like_pi_priority_order() {
        assert_eq!(
            format_spawn_failure(&SpawnOutput {
                error: Some("spawn missing ENOENT".to_string()),
                stderr: "stderr detail".to_string(),
                stdout: "stdout detail".to_string(),
                status: Some(2),
            }),
            "spawn missing ENOENT"
        );
        assert_eq!(
            format_spawn_failure(&SpawnOutput {
                error: None,
                stderr: " stderr detail\n".to_string(),
                stdout: "stdout detail".to_string(),
                status: Some(2),
            }),
            "stderr detail"
        );
        assert_eq!(
            format_spawn_failure(&SpawnOutput {
                error: None,
                stderr: " \n".to_string(),
                stdout: " stdout detail\n".to_string(),
                status: Some(2),
            }),
            "stdout detail"
        );
        assert_eq!(
            format_spawn_failure(&SpawnOutput {
                error: None,
                stderr: String::new(),
                stdout: String::new(),
                status: Some(7),
            }),
            "exit status 7"
        );
        assert_eq!(
            format_spawn_failure(&SpawnOutput {
                error: None,
                stderr: String::new(),
                stdout: String::new(),
                status: None,
            }),
            "exit status unknown"
        );
    }

    #[test]
    fn run_extraction_command_returns_none_on_success_and_message_on_failure() {
        assert_eq!(
            run_extraction_command("sh", &["-c", "exit 0"]).expect("command"),
            None
        );

        let failure = run_extraction_command("sh", &["-c", "printf problem >&2; exit 2"])
            .expect("command")
            .expect("failure");

        assert_eq!(failure, "sh: problem");
    }

    #[test]
    fn finds_binary_recursively_like_pi_tools_manager() {
        let root = temp_dir();
        let nested = root.join("ripgrep-14.1.1").join("bin");
        std::fs::create_dir_all(&nested).expect("nested dir should create");
        std::fs::write(nested.join("rg"), "").expect("binary should write");
        std::fs::write(root.join("fd"), "").expect("root binary should write");

        assert_eq!(
            find_binary_recursively(&root, "rg"),
            Some(nested.join("rg"))
        );
        assert_eq!(find_binary_recursively(&root, "fd"), Some(root.join("fd")));
        assert_eq!(find_binary_recursively(&root, "missing"), None);
    }

    fn temp_dir() -> std::path::PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-child-process-test-{id}"));
        std::fs::create_dir_all(&dir).expect("dir should create");
        dir
    }
}
