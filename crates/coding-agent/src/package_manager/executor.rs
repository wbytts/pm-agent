use super::types::PackageCommandStep;
use crate::exec::{exec_command, ExecOptions};
use crate::utils::paths::{cloud_sync_ignore_commands, CloudSyncIgnoreCommand};
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExecution {
    pub step: PackageCommandStep,
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

pub trait PackageCommandRunner {
    fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String>;
}

#[derive(Debug, Clone, Default)]
pub struct PackageCommandExecutor;

impl PackageCommandRunner for PackageCommandExecutor {
    fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
        if step.command == "git_ensure_ref" {
            return run_git_ensure_ref(step);
        }

        if step.command == "ensure_npm_project" {
            let path = step
                .args
                .first()
                .ok_or_else(|| "ensure_npm_project 缺少目标路径".to_string())?;
            ensure_npm_project(path)?;
            return Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            });
        }

        if step.command == "ensure_git_root" {
            let path = step
                .args
                .first()
                .ok_or_else(|| "ensure_git_root 缺少目标路径".to_string())?;
            ensure_git_root(path)?;
            return Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            });
        }

        if step.command == "ensure_dir" {
            let path = step
                .args
                .first()
                .ok_or_else(|| "ensure_dir 缺少目标路径".to_string())?;
            ensure_dir(path)?;
            return Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            });
        }

        if step.command == "remove_dir" {
            let path = step
                .args
                .first()
                .ok_or_else(|| "remove_dir 缺少目标路径".to_string())?;
            if fs::metadata(path).is_ok() {
                fs::remove_dir_all(path).map_err(|error| format!("删除目录失败：{error}"))?;
                if let Some(root) = step.args.get(1) {
                    prune_empty_parent_dirs(Path::new(path), Path::new(root));
                }
            }
            return Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            });
        }

        if step.command == "run_if_package_json" {
            return run_if_package_json(step);
        }

        let cwd = step.cwd.as_deref().unwrap_or(".");
        let result = exec_command(
            &step.command,
            &step.args,
            cwd,
            Some(ExecOptions {
                cwd: step.cwd.clone(),
                timeout_ms: None,
                ..ExecOptions::default()
            }),
        )?;
        if result.code != 0 {
            return Err(format!(
                "命令执行失败：{} {}，退出码 {}{}",
                step.command,
                step.args.join(" "),
                result.code,
                format_stderr(&result.stderr)
            ));
        }
        Ok(CommandExecution {
            step: step.clone(),
            stdout: result.stdout,
            stderr: result.stderr,
            code: result.code,
        })
    }
}

fn run_git_ensure_ref(step: &PackageCommandStep) -> Result<CommandExecution, String> {
    let cwd = step
        .cwd
        .as_deref()
        .ok_or_else(|| "git_ensure_ref 缺少 cwd".to_string())?;
    let Some((reset_ref, fetch_args)) = step.args.split_first() else {
        return Err("git_ensure_ref 缺少目标 ref".to_string());
    };
    let fetch_args = fetch_args
        .strip_prefix(&["--".to_string()])
        .unwrap_or(fetch_args);

    run_git(fetch_args, cwd)?;
    let local = run_git_capture(&["rev-parse".to_string(), "HEAD".to_string()], cwd)?;
    let target = run_git_capture(&["rev-parse".to_string(), reset_ref.to_string()], cwd)?;
    if local.trim() == target.trim() {
        return Ok(CommandExecution {
            step: step.clone(),
            stdout: "unchanged".to_string(),
            stderr: String::new(),
            code: 0,
        });
    }

    run_git(
        &[
            "reset".to_string(),
            "--hard".to_string(),
            reset_ref.to_string(),
        ],
        cwd,
    )?;
    run_git(&["clean".to_string(), "-fdx".to_string()], cwd)?;
    Ok(CommandExecution {
        step: step.clone(),
        stdout: "changed".to_string(),
        stderr: String::new(),
        code: 0,
    })
}

fn run_git(args: &[String], cwd: &str) -> Result<(), String> {
    let result = exec_command(
        "git",
        args,
        cwd,
        Some(ExecOptions {
            cwd: Some(cwd.to_string()),
            ..ExecOptions::default()
        }),
    )?;
    if result.code != 0 {
        return Err(format!(
            "命令执行失败：git {}，退出码 {}{}",
            args.join(" "),
            result.code,
            format_stderr(&result.stderr)
        ));
    }
    Ok(())
}

fn run_git_capture(args: &[String], cwd: &str) -> Result<String, String> {
    let result = exec_command(
        "git",
        args,
        cwd,
        Some(ExecOptions {
            cwd: Some(cwd.to_string()),
            ..ExecOptions::default()
        }),
    )?;
    if result.code != 0 {
        return Err(format!(
            "命令执行失败：git {}，退出码 {}{}",
            args.join(" "),
            result.code,
            format_stderr(&result.stderr)
        ));
    }
    Ok(result.stdout)
}

fn run_if_package_json(step: &PackageCommandStep) -> Result<CommandExecution, String> {
    let cwd = step
        .cwd
        .as_deref()
        .ok_or_else(|| "run_if_package_json 缺少 cwd".to_string())?;
    if !Path::new(cwd).join("package.json").exists() {
        return Ok(CommandExecution {
            step: step.clone(),
            stdout: "skipped".to_string(),
            stderr: String::new(),
            code: 0,
        });
    }
    let Some((command, args)) = step.args.split_first() else {
        return Err("run_if_package_json 缺少命令".to_string());
    };
    let result = exec_command(
        command,
        args,
        cwd,
        Some(ExecOptions {
            cwd: step.cwd.clone(),
            timeout_ms: None,
            ..ExecOptions::default()
        }),
    )?;
    if result.code != 0 {
        return Err(format!(
            "命令执行失败：{} {}，退出码 {}{}",
            command,
            args.join(" "),
            result.code,
            format_stderr(&result.stderr)
        ));
    }
    Ok(CommandExecution {
        step: step.clone(),
        stdout: result.stdout,
        stderr: result.stderr,
        code: result.code,
    })
}

fn prune_empty_parent_dirs(target_dir: &Path, install_root: &Path) {
    let resolved_root =
        fs::canonicalize(install_root).unwrap_or_else(|_| install_root.to_path_buf());
    let mut current = target_dir.parent();

    while let Some(dir) = current {
        let resolved_dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if resolved_dir == resolved_root || !resolved_dir.starts_with(&resolved_root) {
            break;
        }
        if !dir.exists() {
            current = dir.parent();
            continue;
        }
        let is_empty = fs::read_dir(dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            break;
        }
        if fs::remove_dir_all(dir).is_err() {
            break;
        }
        current = dir.parent();
    }
}

fn ensure_git_root(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    fs::create_dir_all(path).map_err(|error| format!("创建 git 安装目录失败：{error}"))?;
    ensure_gitignore(path, "写入 git .gitignore 失败")
}

fn ensure_dir(path: impl AsRef<Path>) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("创建目录失败：{error}"))
}

fn ensure_npm_project(path: impl AsRef<Path>) -> Result<(), String> {
    ensure_npm_project_with_cloud_sync_marker(
        path,
        |path| cloud_sync_ignore_commands(path),
        run_cloud_sync_ignore_command,
    )
}

fn ensure_npm_project_with_cloud_sync_marker(
    path: impl AsRef<Path>,
    commands_for_path: impl Fn(&Path) -> Vec<CloudSyncIgnoreCommand>,
    mut run_command: impl FnMut(&CloudSyncIgnoreCommand) -> io::Result<()>,
) -> Result<(), String> {
    let path = path.as_ref();
    fs::create_dir_all(path).map_err(|error| format!("创建 npm 安装目录失败：{error}"))?;
    for command in commands_for_path(path) {
        // pi 对云同步忽略标记采用 best-effort：xattr/setfattr 失败不阻断安装。
        let _ = run_command(&command);
    }
    let package_json = path.join("package.json");
    if !package_json.exists() {
        fs::write(
            &package_json,
            "{\n  \"name\": \"pi-extensions\",\n  \"private\": true\n}\n",
        )
        .map_err(|error| format!("写入 npm package.json 失败：{error}"))?;
    }
    ensure_gitignore(path, "写入 npm .gitignore 失败")?;
    Ok(())
}

fn run_cloud_sync_ignore_command(command: &CloudSyncIgnoreCommand) -> io::Result<()> {
    let status = Command::new(&command.program)
        .args(&command.args)
        .status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("{} exited with status {status}", command.program),
        ));
    }
    Ok(())
}

fn ensure_gitignore(path: &Path, error_context: &str) -> Result<(), String> {
    let ignore_path = path.join(".gitignore");
    if !ignore_path.exists() {
        fs::write(&ignore_path, "*\n!.gitignore\n")
            .map_err(|error| format!("{error_context}：{error}"))?;
    }
    Ok(())
}

fn format_stderr(stderr: &str) -> String {
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
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn remove_dir_step_is_handled_without_external_command() {
        let dir = temp_dir();
        fs::write(dir.join("file.txt"), "demo").expect("file should be written");
        let step = PackageCommandStep {
            command: "remove_dir".to_string(),
            args: vec![dir.to_string_lossy().to_string()],
            cwd: None,
        };

        let result = PackageCommandExecutor
            .run(&step)
            .expect("remove_dir should succeed");

        assert_eq!(result.code, 0);
        assert!(!dir.exists());
    }

    #[test]
    fn remove_dir_step_prunes_empty_git_parent_dirs_like_pi() {
        let root = temp_dir().join("git").join("github.com");
        let target = root.join("user").join("repo");
        fs::create_dir_all(&target).expect("target dir should be created");
        fs::write(target.join("file.txt"), "demo").expect("file should be written");
        let step = PackageCommandStep {
            command: "remove_dir".to_string(),
            args: vec![
                target.to_string_lossy().to_string(),
                root.to_string_lossy().to_string(),
            ],
            cwd: None,
        };

        PackageCommandExecutor
            .run(&step)
            .expect("remove_dir should succeed");

        assert!(!target.exists());
        assert!(!root.join("user").exists());
        assert!(root.exists());
    }

    #[test]
    fn remove_dir_step_prunes_empty_host_parent_to_git_root_like_pi() {
        let root = temp_dir().join("git");
        let host = root.join("github.com");
        let owner = host.join("user");
        let target = owner.join("repo");
        fs::create_dir_all(&target).expect("target dir should be created");
        fs::write(target.join("file.txt"), "demo").expect("file should be written");
        let step = PackageCommandStep {
            command: "remove_dir".to_string(),
            args: vec![
                target.to_string_lossy().to_string(),
                root.to_string_lossy().to_string(),
            ],
            cwd: None,
        };

        PackageCommandExecutor
            .run(&step)
            .expect("remove_dir should succeed");

        assert!(!target.exists());
        assert!(!owner.exists());
        assert!(!host.exists());
        assert!(root.exists());
    }

    #[test]
    fn ensure_npm_project_creates_project_files() {
        let dir = temp_dir().join("npm-root");
        let step = PackageCommandStep {
            command: "ensure_npm_project".to_string(),
            args: vec![dir.to_string_lossy().to_string()],
            cwd: None,
        };

        PackageCommandExecutor
            .run(&step)
            .expect("npm project should be created");

        assert!(dir.join("package.json").exists());
        assert_eq!(
            fs::read_to_string(dir.join("package.json")).expect("package.json should read"),
            "{\n  \"name\": \"pi-extensions\",\n  \"private\": true\n}\n"
        );
        assert!(dir.join(".gitignore").exists());
    }

    #[test]
    fn ensure_npm_project_marks_cloud_sync_ignored_like_pi_without_failing_on_command_errors() {
        let dir = temp_dir().join("npm-root");
        let mut commands = Vec::new();
        ensure_npm_project_with_cloud_sync_marker(
            &dir,
            |path| {
                vec![CloudSyncIgnoreCommand {
                    program: "missing-cloud-sync-marker".to_string(),
                    args: vec![path.to_string_lossy().to_string()],
                }]
            },
            |command| {
                commands.push(command.clone());
                Err(std::io::Error::new(std::io::ErrorKind::Other, "boom"))
            },
        )
        .expect("cloud sync marker errors should be ignored");

        assert_eq!(commands.len(), 1);
        assert!(dir.join("package.json").exists());
        assert!(dir.join(".gitignore").exists());
    }

    #[test]
    fn ensure_git_root_creates_gitignore_without_npm_package_file_like_pi() {
        let dir = temp_dir().join("git-root");
        let step = PackageCommandStep {
            command: "ensure_git_root".to_string(),
            args: vec![dir.to_string_lossy().to_string()],
            cwd: None,
        };

        PackageCommandExecutor
            .run(&step)
            .expect("git root should be created");

        assert_eq!(
            fs::read_to_string(dir.join(".gitignore")).expect("gitignore should read"),
            "*\n!.gitignore\n"
        );
        assert!(!dir.join("package.json").exists());
    }

    #[test]
    fn run_if_package_json_skips_command_when_package_file_is_missing_like_pi() {
        let dir = temp_dir().join("git-package");
        fs::create_dir_all(&dir).expect("git package dir should be created");
        let step = PackageCommandStep {
            command: "run_if_package_json".to_string(),
            args: vec!["definitely-missing-command".to_string()],
            cwd: Some(dir.to_string_lossy().to_string()),
        };

        let result = PackageCommandExecutor
            .run(&step)
            .expect("missing package.json should skip command");

        assert_eq!(result.code, 0);
        assert_eq!(result.stdout, "skipped");
    }

    #[test]
    fn git_ensure_ref_recovers_from_rewritten_remote_history_like_pi() {
        let root = temp_dir();
        let remote = root.join("remote");
        let checkout = root.join("checkout");
        fs::create_dir_all(&remote).expect("remote dir should be created");
        git(&remote, &["init", "--initial-branch=main"]);
        git(&remote, &["config", "user.email", "test@test.com"]);
        git(&remote, &["config", "user.name", "Test"]);
        write_commit(&remote, "extension.ts", "// v1", "v1");

        git(
            &root,
            &["clone", remote.to_string_lossy().as_ref(), "checkout"],
        );
        assert_eq!(read_file(&checkout, "extension.ts"), "// v1");

        write_commit(&remote, "extension.ts", "// v2", "v2");
        let first_update = PackageCommandStep {
            command: "git_ensure_ref".to_string(),
            args: vec![
                "@{upstream}^{commit}".to_string(),
                "--".to_string(),
                "fetch".to_string(),
                "--prune".to_string(),
                "--no-tags".to_string(),
                "origin".to_string(),
                "+refs/heads/main:refs/remotes/origin/main".to_string(),
            ],
            cwd: Some(checkout.to_string_lossy().to_string()),
        };
        let first_result = PackageCommandExecutor
            .run(&first_update)
            .expect("first git update should succeed");
        assert_eq!(first_result.stdout, "changed");
        assert_eq!(read_file(&checkout, "extension.ts"), "// v2");

        git(&remote, &["reset", "--hard", "HEAD~1"]);
        let rewritten_commit = write_commit(&remote, "extension.ts", "// rewritten", "rewrite");
        let junk = checkout.join("untracked.tmp");
        fs::write(&junk, "junk").expect("junk file should write");

        let rewritten_result = PackageCommandExecutor
            .run(&first_update)
            .expect("rewritten history update should succeed");

        assert_eq!(rewritten_result.stdout, "changed");
        assert_eq!(
            git_output(&checkout, &["rev-parse", "HEAD"]),
            rewritten_commit
        );
        assert_eq!(read_file(&checkout, "extension.ts"), "// rewritten");
        assert!(!junk.exists(), "git clean should remove untracked files");
    }

    #[test]
    fn git_ensure_ref_does_not_reset_matching_annotated_tag_checkout_like_pi() {
        let root = temp_dir();
        let remote = root.join("remote");
        let checkout = root.join("checkout");
        fs::create_dir_all(&remote).expect("remote dir should be created");
        git(&remote, &["init", "--initial-branch=main"]);
        git(&remote, &["config", "user.email", "test@test.com"]);
        git(&remote, &["config", "user.name", "Test"]);
        let tagged_commit = write_commit(&remote, "extension.ts", "// v1", "v1");
        git(&remote, &["tag", "-a", "v1", "-m", "v1"]);

        git(
            &root,
            &["clone", remote.to_string_lossy().as_ref(), "checkout"],
        );
        git(&checkout, &["checkout", "v1"]);
        assert_eq!(git_output(&checkout, &["rev-parse", "HEAD"]), tagged_commit);

        let step = PackageCommandStep {
            command: "git_ensure_ref".to_string(),
            args: vec![
                "FETCH_HEAD^{commit}".to_string(),
                "--".to_string(),
                "fetch".to_string(),
                "origin".to_string(),
                "v1".to_string(),
            ],
            cwd: Some(checkout.to_string_lossy().to_string()),
        };

        let result = PackageCommandExecutor
            .run(&step)
            .expect("annotated tag update should succeed");

        assert_eq!(result.stdout, "unchanged");
        assert_eq!(git_output(&checkout, &["rev-parse", "HEAD"]), tagged_commit);
        assert_eq!(read_file(&checkout, "extension.ts"), "// v1");
    }

    #[test]
    fn ensure_dir_creates_nested_directory_like_pi_git_install_parent() {
        let dir = temp_dir().join("git").join("github.com").join("user");
        let step = PackageCommandStep {
            command: "ensure_dir".to_string(),
            args: vec![dir.to_string_lossy().to_string()],
            cwd: None,
        };

        PackageCommandExecutor
            .run(&step)
            .expect("directory should be created");

        assert!(dir.is_dir());
    }

    #[test]
    fn command_runner_preserves_argv_entries_containing_spaces_like_pi() {
        let value = "C:\\Users\\A B\\.pi\\npm";
        let step = PackageCommandStep {
            command: "/usr/bin/printf".to_string(),
            args: vec!["%s".to_string(), value.to_string()],
            cwd: None,
        };

        let result = PackageCommandExecutor
            .run(&step)
            .expect("argv should be passed without shell splitting");

        assert_eq!(result.stdout.trim(), value);
    }

    #[test]
    fn command_runner_captures_startup_chatter_for_non_interactive_stdout_cleanliness_like_pi() {
        let step = PackageCommandStep {
            command: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "printf 'changed 1 package in 471ms\\n'; printf 'found 0 vulnerabilities\\n' >&2"
                    .to_string(),
            ],
            cwd: None,
        };

        let result = PackageCommandExecutor
            .run(&step)
            .expect("startup package command output should be captured");

        assert_eq!(result.stdout, "changed 1 package in 471ms\n");
        assert_eq!(result.stderr, "found 0 vulnerabilities\n");
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-package-executor-test-{id}-{count}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_commit(repo: &Path, file: &str, content: &str, message: &str) -> String {
        fs::write(repo.join(file), content).expect("tracked file should write");
        git(repo, &["add", file]);
        git(repo, &["commit", "-m", message]);
        git_output(repo, &["rev-parse", "HEAD"])
    }

    fn read_file(dir: &Path, file: &str) -> String {
        fs::read_to_string(dir.join(file)).expect("file should read")
    }

    fn git(cwd: &Path, args: &[&str]) {
        git_output(cwd, args);
    }

    fn git_output(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command should start");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
