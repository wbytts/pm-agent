use super::operations::{conditional_package_json_step, plan_install};
use super::source::git_install_path;
use super::types::{NpmCommandConfig, PackageCommandStep, SourceScope};
use crate::utils::git::GitSource;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitUpdateTarget {
    pub reset_ref: String,
    pub fetch_args: Vec<String>,
}

pub fn plan_git_update_steps(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &GitSource,
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
    npm_command_configured: bool,
) -> Vec<PackageCommandStep> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let target_dir = git_install_path(agent_dir, cwd, source, scope);
    if !target_dir.exists() {
        return plan_install(
            agent_dir,
            cwd,
            &source_string(source),
            scope,
            npm_command_configured.then(|| npm_command.clone()),
        )
        .steps;
    }

    let target = git_update_target(&target_dir, source);
    let target_dir = display_path(&target_dir);
    let mut steps = vec![PackageCommandStep {
        command: "git_ensure_ref".to_string(),
        args: std::iter::once(target.reset_ref)
            .chain(std::iter::once("--".to_string()))
            .chain(target.fetch_args)
            .collect(),
        cwd: Some(target_dir.clone()),
    }];
    steps.push(conditional_package_json_step(
        npm_command,
        git_dependency_install_args(npm_command_configured),
        Some(target_dir),
    ));
    steps
}

pub fn git_update_target(target_dir: &Path, source: &GitSource) -> GitUpdateTarget {
    if let Some(reference) = &source.reference {
        return GitUpdateTarget {
            reset_ref: "FETCH_HEAD^{commit}".to_string(),
            fetch_args: vec![
                "fetch".to_string(),
                "origin".to_string(),
                reference.to_string(),
            ],
        };
    }

    if let Some(branch) = upstream_branch(target_dir) {
        return GitUpdateTarget {
            reset_ref: "@{upstream}^{commit}".to_string(),
            fetch_args: vec![
                "fetch".to_string(),
                "--prune".to_string(),
                "--no-tags".to_string(),
                "origin".to_string(),
                format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
            ],
        };
    }

    if let Some(branch) = origin_head_branch(target_dir) {
        return GitUpdateTarget {
            reset_ref: "origin/HEAD^{commit}".to_string(),
            fetch_args: vec![
                "fetch".to_string(),
                "--prune".to_string(),
                "--no-tags".to_string(),
                "origin".to_string(),
                format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
            ],
        };
    }

    GitUpdateTarget {
        reset_ref: "origin/HEAD^{commit}".to_string(),
        fetch_args: vec![
            "fetch".to_string(),
            "--prune".to_string(),
            "--no-tags".to_string(),
            "origin".to_string(),
            "+HEAD:refs/remotes/origin/HEAD".to_string(),
        ],
    }
}

fn upstream_branch(target_dir: &Path) -> Option<String> {
    let head_ref = head_branch_ref(target_dir)?;
    let config = fs::read_to_string(target_dir.join(".git").join("config")).ok()?;
    let branch = head_ref.strip_prefix("refs/heads/")?;
    let section = format!(r#"[branch "{branch}"]"#);
    let block = config_section(&config, &section)?;
    let remote = config_value(&block, "remote")?;
    if remote != "origin" {
        return None;
    }
    config_value(&block, "merge")?
        .strip_prefix("refs/heads/")
        .map(ToString::to_string)
}

fn origin_head_branch(target_dir: &Path) -> Option<String> {
    let value = fs::read_to_string(
        target_dir
            .join(".git")
            .join("refs")
            .join("remotes")
            .join("origin")
            .join("HEAD"),
    )
    .ok()?;
    value
        .trim()
        .strip_prefix("ref: refs/remotes/origin/")
        .filter(|branch| !branch.is_empty())
        .map(ToString::to_string)
}

fn head_branch_ref(target_dir: &Path) -> Option<String> {
    fs::read_to_string(target_dir.join(".git").join("HEAD"))
        .ok()?
        .trim()
        .strip_prefix("ref: ")
        .map(ToString::to_string)
}

fn config_section(config: &str, section: &str) -> Option<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section {
                return Some(lines.join("\n"));
            }
            if trimmed == section {
                in_section = true;
                lines.clear();
            }
            continue;
        }
        if in_section {
            lines.push(line.to_string());
        }
    }
    in_section.then(|| lines.join("\n"))
}

fn config_value<'a>(section: &'a str, key: &str) -> Option<&'a str> {
    section.lines().find_map(|line| {
        let (left, right) = line.split_once('=')?;
        (left.trim() == key).then(|| right.trim())
    })
}

fn git_dependency_install_args(npm_command_configured: bool) -> Vec<String> {
    if npm_command_configured {
        vec!["install".to_string()]
    } else {
        vec!["install".to_string(), "--omit=dev".to_string()]
    }
}

fn source_string(source: &GitSource) -> String {
    if let Some(reference) = &source.reference {
        format!("git:{}@{}", source.repo, reference)
    } else {
        source.repo.clone()
    }
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_manager::source::parse_source;
    use crate::package_manager::types::ParsedSource;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_GIT_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn pinned_git_update_fetches_reference_to_fetch_head() {
        let dir = temp_git_dir();
        let source = git_source("git:https://github.com/user/repo@main");

        let target = git_update_target(&dir, &source);

        assert_eq!(target.fetch_args, vec!["fetch", "origin", "main"]);
        assert_eq!(target.reset_ref, "FETCH_HEAD^{commit}");
    }

    #[test]
    fn unpinned_git_update_uses_upstream_refspec_when_available() {
        let dir = temp_git_dir();
        fs::write(dir.join(".git").join("HEAD"), "ref: refs/heads/dev\n").expect("head write");
        fs::write(
            dir.join(".git").join("config"),
            "[branch \"dev\"]\n\tremote = origin\n\tmerge = refs/heads/dev\n",
        )
        .expect("config write");
        let source = git_source("git:https://github.com/user/repo");

        let target = git_update_target(&dir, &source);

        assert_eq!(target.reset_ref, "@{upstream}^{commit}");
        assert_eq!(
            target.fetch_args,
            vec![
                "fetch",
                "--prune",
                "--no-tags",
                "origin",
                "+refs/heads/dev:refs/remotes/origin/dev"
            ]
        );
    }

    #[test]
    fn unpinned_git_update_falls_back_to_origin_head_branch() {
        let dir = temp_git_dir();
        let origin = dir.join(".git").join("refs").join("remotes").join("origin");
        fs::create_dir_all(&origin).expect("origin refs dir");
        fs::write(origin.join("HEAD"), "ref: refs/remotes/origin/main\n").expect("head write");
        let source = git_source("git:https://github.com/user/repo");

        let target = git_update_target(&dir, &source);

        assert_eq!(target.reset_ref, "origin/HEAD^{commit}");
        assert_eq!(
            target.fetch_args,
            vec![
                "fetch",
                "--prune",
                "--no-tags",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main"
            ]
        );
    }

    #[test]
    fn unpinned_git_update_falls_back_to_remote_head_refspec() {
        let dir = temp_git_dir();
        let source = git_source("git:https://github.com/user/repo");

        let target = git_update_target(&dir, &source);

        assert_eq!(target.reset_ref, "origin/HEAD^{commit}");
        assert_eq!(
            target.fetch_args,
            vec![
                "fetch",
                "--prune",
                "--no-tags",
                "origin",
                "+HEAD:refs/remotes/origin/HEAD"
            ]
        );
    }

    fn git_source(source: &str) -> GitSource {
        match parse_source(source) {
            ParsedSource::Git(source) => source,
            other => panic!("expected git source, got {other:?}"),
        }
    }

    fn temp_git_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let sequence = TEMP_GIT_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pm-agent-git-update-test-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join(".git")).expect("git dir");
        dir
    }
}
