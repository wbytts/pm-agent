use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    pub timeout_ms: Option<u64>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    pub killed: bool,
}

pub fn exec_command(
    command: &str,
    args: &[String],
    cwd: &str,
    options: Option<ExecOptions>,
) -> Result<ExecResult, String> {
    let options = options.unwrap_or_default();
    let mut process = Command::new(command);
    process
        .args(args)
        .current_dir(options.cwd.as_deref().unwrap_or(cwd))
        .envs(options.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut process);
    let mut child = process
        .spawn()
        .map_err(|error| format!("启动命令失败：{error}"))?;

    let deadline = options
        .timeout_ms
        .map(|timeout| Instant::now() + Duration::from_millis(timeout));
    let mut killed = false;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("等待命令失败：{error}"))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("读取命令输出失败：{error}"))?;
            return Ok(ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                code: status.code().unwrap_or(1),
                killed,
            });
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            killed = true;
            kill_process_tree(&mut child);
            let output = child
                .wait_with_output()
                .map_err(|error| format!("读取超时命令输出失败：{error}"))?;
            return Ok(ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                code: 1,
                killed,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{pid}")])
        .status();
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_command_and_captures_output() {
        let result = exec_command(
            "sh",
            &["-lc".to_string(), "printf hello".to_string()],
            ".",
            None,
        )
        .expect("command should run");
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.code, 0);
    }

    #[test]
    fn timeout_kills_descendant_processes_like_pi() {
        let result = exec_command(
            "sh",
            &[
                "-lc".to_string(),
                "printf before; sleep 2; printf after".to_string(),
            ],
            ".",
            Some(ExecOptions {
                timeout_ms: Some(100),
                ..ExecOptions::default()
            }),
        )
        .expect("command should return timeout result");

        assert!(result.killed);
        assert!(result.stdout.contains("before"));
        assert!(!result.stdout.contains("after"));
    }
}
