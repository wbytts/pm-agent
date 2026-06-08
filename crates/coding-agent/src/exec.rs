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
    let mut child = Command::new(command)
        .args(args)
        .current_dir(options.cwd.as_deref().unwrap_or(cwd))
        .envs(options.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
            let _ = child.kill();
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
}
