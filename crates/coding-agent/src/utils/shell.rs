use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
}

pub fn get_shell_config(custom_shell_path: Option<&str>) -> Result<ShellConfig, String> {
    if let Some(path) = custom_shell_path {
        if Path::new(path).exists() {
            return Ok(ShellConfig {
                shell: path.to_string(),
                args: vec!["-c".to_string()],
            });
        }
        return Err(format!("自定义 shell 路径不存在：{path}"));
    }

    if cfg!(target_os = "windows") {
        return Ok(ShellConfig {
            shell: "cmd".to_string(),
            args: vec!["/C".to_string()],
        });
    }

    if Path::new("/bin/bash").exists() {
        return Ok(ShellConfig {
            shell: "/bin/bash".to_string(),
            args: vec!["-c".to_string()],
        });
    }

    Ok(ShellConfig {
        shell: "sh".to_string(),
        args: vec!["-c".to_string()],
    })
}

pub fn sanitize_binary_output(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            if matches!(*ch, '\t' | '\n' | '\r') {
                return true;
            }
            if code <= 0x1f || code == 0x7f {
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
}
