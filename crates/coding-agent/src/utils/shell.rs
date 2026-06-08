use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
}

pub fn get_shell_config(custom_shell_path: Option<&str>) -> Result<ShellConfig, String> {
    let bash_on_path = find_bash_on_path();
    let program_files = std::env::var("ProgramFiles").ok();
    let program_files_x86 = std::env::var("ProgramFiles(x86)").ok();
    resolve_shell_config_for_platform(
        custom_shell_path,
        std::env::consts::OS,
        program_files.as_deref(),
        program_files_x86.as_deref(),
        bash_on_path.as_deref(),
        |path| Path::new(path).exists(),
    )
}

fn resolve_shell_config_for_platform(
    custom_shell_path: Option<&str>,
    platform: &str,
    program_files: Option<&str>,
    program_files_x86: Option<&str>,
    bash_on_path: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> Result<ShellConfig, String> {
    if let Some(path) = custom_shell_path {
        if exists(path) {
            return Ok(ShellConfig {
                shell: path.to_string(),
                args: vec!["-c".to_string()],
            });
        }
        return Err(format!("自定义 shell 路径不存在：{path}"));
    }

    if platform == "windows" {
        let mut searched = Vec::new();
        if let Some(program_files) = program_files {
            searched.push(format!("{program_files}\\Git\\bin\\bash.exe"));
        }
        if let Some(program_files_x86) = program_files_x86 {
            searched.push(format!("{program_files_x86}\\Git\\bin\\bash.exe"));
        }

        for path in &searched {
            if exists(path) {
                return Ok(ShellConfig {
                    shell: path.clone(),
                    args: vec!["-c".to_string()],
                });
            }
        }

        if let Some(path) = bash_on_path.filter(|path| exists(path)) {
            return Ok(ShellConfig {
                shell: path.to_string(),
                args: vec!["-c".to_string()],
            });
        }

        return Err(format!(
            "未找到 bash shell。请安装 Git for Windows、将 bash 加入 PATH，或在 settings.json 设置 shellPath。已搜索：{}",
            searched.join(", ")
        ));
    }

    if exists("/bin/bash") {
        return Ok(ShellConfig {
            shell: "/bin/bash".to_string(),
            args: vec!["-c".to_string()],
        });
    }

    if let Some(path) = bash_on_path.filter(|path| exists(path)) {
        return Ok(ShellConfig {
            shell: path.to_string(),
            args: vec!["-c".to_string()],
        });
    }

    Ok(ShellConfig {
        shell: "sh".to_string(),
        args: vec!["-c".to_string()],
    })
}

fn find_bash_on_path() -> Option<String> {
    let command = if cfg!(target_os = "windows") {
        ("where", "bash.exe")
    } else {
        ("which", "bash")
    };
    let output = Command::new(command.0)
        .arg(command.1)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

pub fn sanitize_binary_output(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            if matches!(*ch, '\t' | '\n' | '\r') {
                return true;
            }
            if code <= 0x1f {
                return false;
            }
            if (0xfff9..=0xfffb).contains(&code) {
                return false;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_shell() {
        let config = get_shell_config(None).expect("shell config");
        assert!(!config.shell.is_empty());
        assert!(!config.args.is_empty());
    }

    #[test]
    fn rejects_missing_custom_shell() {
        let error = get_shell_config(Some("/definitely/missing/pm-agent-shell")).unwrap_err();
        assert!(error.contains("不存在"));
    }

    #[test]
    fn sanitizes_control_characters_but_keeps_line_breaks() {
        assert_eq!(sanitize_binary_output("a\u{0000}\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn sanitize_binary_output_keeps_delete_character_like_pi() {
        assert_eq!(sanitize_binary_output("a\u{007f}b"), "a\u{007f}b");
    }

    #[test]
    fn resolves_windows_git_bash_before_path_like_pi_shell_config() {
        let config = resolve_shell_config_for_platform(
            None,
            "windows",
            Some("C:\\Program Files"),
            Some("C:\\Program Files (x86)"),
            Some("C:\\Tools\\bash.exe"),
            |path| path == "C:\\Program Files\\Git\\bin\\bash.exe" || path == "C:\\Tools\\bash.exe",
        )
        .expect("shell config");

        assert_eq!(config.shell, "C:\\Program Files\\Git\\bin\\bash.exe");
        assert_eq!(config.args, vec!["-c"]);
    }

    #[test]
    fn resolves_windows_path_bash_when_git_bash_is_missing_like_pi_shell_config() {
        let config = resolve_shell_config_for_platform(
            None,
            "windows",
            Some("C:\\Program Files"),
            None,
            Some("C:\\Tools\\bash.exe"),
            |path| path == "C:\\Tools\\bash.exe",
        )
        .expect("shell config");

        assert_eq!(config.shell, "C:\\Tools\\bash.exe");
        assert_eq!(config.args, vec!["-c"]);
    }

    #[test]
    fn resolves_unix_path_bash_before_sh_like_pi_shell_config() {
        let config = resolve_shell_config_for_platform(
            None,
            "linux",
            None,
            None,
            Some("/usr/local/bin/bash"),
            |path| path == "/usr/local/bin/bash",
        )
        .expect("shell config");

        assert_eq!(config.shell, "/usr/local/bin/bash");
        assert_eq!(config.args, vec!["-c"]);
    }
}
