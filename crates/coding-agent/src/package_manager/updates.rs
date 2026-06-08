use super::git_update::plan_git_update_steps;
use super::operations::plan_install;
use super::source::{
    git_install_path, managed_npm_install_path, parse_source, scoped_source_identity,
};
use super::types::{
    NpmCommandConfig, NpmSource, PackageKind, PackageOperationPlan, PackageUpdate, ParsedSource,
    ProgressAction, SourceScope,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredUpdateSource {
    pub source: String,
    pub scope: SourceScope,
}

pub trait UpdateCheck {
    fn latest_npm_version(&self, package_name: &str) -> Result<Option<String>, String>;
    fn remote_git_head(&self, installed_path: &Path) -> Result<Option<String>, String>;
}

pub fn plan_update(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
    npm_command: Option<NpmCommandConfig>,
) -> PackageOperationPlan {
    let npm_command_configured = npm_command.is_some();
    let npm_command = npm_command.unwrap_or_default();
    let parsed = parse_source(source);
    let steps = match &parsed {
        ParsedSource::Npm(npm) => {
            if npm.pinned {
                Vec::new()
            } else {
                plan_install(
                    agent_dir,
                    cwd,
                    &format!("npm:{}@latest", npm.name),
                    scope,
                    Some(npm_command),
                )
                .steps
            }
        }
        ParsedSource::Git(git) => plan_git_update_steps(
            agent_dir,
            cwd,
            git,
            scope,
            &npm_command,
            npm_command_configured,
        ),
        ParsedSource::Local(_) => Vec::new(),
    };
    PackageOperationPlan {
        action: ProgressAction::Update,
        source: source.to_string(),
        steps,
    }
}

pub fn check_configured_updates<C: UpdateCheck>(
    checker: &C,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    sources: &[ConfiguredUpdateSource],
) -> Vec<PackageUpdate> {
    check_configured_updates_with_npm_fallback(checker, agent_dir, cwd, sources, |_| None)
}

pub(super) fn check_configured_updates_with_npm_fallback<C, F>(
    checker: &C,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    sources: &[ConfiguredUpdateSource],
    mut legacy_npm_path: F,
) -> Vec<PackageUpdate>
where
    C: UpdateCheck,
    F: FnMut(&NpmSource) -> Option<PathBuf>,
{
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    dedupe_update_sources(agent_dir, cwd, sources)
        .iter()
        .filter_map(|entry| check_one_update(checker, agent_dir, cwd, entry, &mut legacy_npm_path))
        .collect()
}

fn dedupe_update_sources(
    agent_dir: &Path,
    cwd: &Path,
    sources: &[ConfiguredUpdateSource],
) -> Vec<ConfiguredUpdateSource> {
    let mut by_identity = HashMap::<String, ConfiguredUpdateSource>::new();
    let mut identities = Vec::<String>::new();

    for source in sources {
        let identity = scoped_source_identity(agent_dir, cwd, &source.source, source.scope);
        if !by_identity.contains_key(&identity) {
            identities.push(identity.clone());
        }
        if source.scope == SourceScope::Project {
            by_identity.insert(identity, source.clone());
        } else {
            by_identity
                .entry(identity)
                .or_insert_with(|| source.clone());
        }
    }

    identities
        .into_iter()
        .filter_map(|identity| by_identity.remove(&identity))
        .collect()
}

fn check_one_update<C, F>(
    checker: &C,
    agent_dir: &Path,
    cwd: &Path,
    entry: &ConfiguredUpdateSource,
    legacy_npm_path: &mut F,
) -> Option<PackageUpdate>
where
    C: UpdateCheck,
    F: FnMut(&NpmSource) -> Option<PathBuf>,
{
    if entry.scope == SourceScope::Temporary {
        return None;
    }
    match parse_source(&entry.source) {
        ParsedSource::Npm(npm) => {
            if npm.pinned {
                return None;
            }
            let installed_path =
                npm_install_path(agent_dir, cwd, &npm, entry.scope, legacy_npm_path);
            let installed_version = installed_npm_version(&installed_path)?;
            let latest = checker.latest_npm_version(&npm.name).ok().flatten()?;
            (installed_version != latest).then_some(PackageUpdate {
                source: entry.source.clone(),
                display_name: npm.name,
                kind: PackageKind::Npm,
                scope: entry.scope,
            })
        }
        ParsedSource::Git(git) => {
            if git.pinned {
                return None;
            }
            let installed_path = git_install_path(agent_dir, cwd, &git, entry.scope);
            let local = local_git_head(&installed_path)?;
            let remote = checker.remote_git_head(&installed_path).ok().flatten()?;
            (local.trim() != remote.trim()).then_some(PackageUpdate {
                source: entry.source.clone(),
                display_name: format!("{}/{}", git.host, git.path),
                kind: PackageKind::Git,
                scope: entry.scope,
            })
        }
        ParsedSource::Local(_) => None,
    }
}

fn npm_install_path<F>(
    agent_dir: &Path,
    cwd: &Path,
    source: &NpmSource,
    scope: SourceScope,
    legacy_npm_path: &mut F,
) -> PathBuf
where
    F: FnMut(&NpmSource) -> Option<PathBuf>,
{
    let managed_path = managed_npm_install_path(agent_dir, cwd, source, scope);
    if scope != SourceScope::User || managed_path.exists() {
        return managed_path;
    }
    legacy_npm_path(source)
        .filter(|path| path.exists())
        .unwrap_or(managed_path)
}

pub(super) fn installed_npm_version(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path.join("package.json")).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&value).ok()?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn local_git_head(path: &Path) -> Option<String> {
    let git_dir = git_dir_path(path)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let trimmed = head.trim();
    if let Some(reference) = trimmed.strip_prefix("ref: ") {
        return fs::read_to_string(git_dir.join(reference))
            .ok()
            .or_else(|| packed_ref_head(&git_dir, reference));
    }
    Some(trimmed.to_string())
}

fn git_dir_path(path: &Path) -> Option<PathBuf> {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let content = fs::read_to_string(&dot_git).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir:")?.trim();
    let gitdir_path = PathBuf::from(gitdir);
    Some(if gitdir_path.is_absolute() {
        gitdir_path
    } else {
        path.join(gitdir_path)
    })
}

fn packed_ref_head(git_dir: &Path, reference: &str) -> Option<String> {
    let packed_refs = fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    packed_refs.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            return None;
        }
        let (head, packed_reference) = line.split_once(char::is_whitespace)?;
        (packed_reference == reference).then(|| head.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeChecker {
        npm: HashMap<String, String>,
        git: HashMap<String, String>,
    }

    impl UpdateCheck for FakeChecker {
        fn latest_npm_version(&self, package_name: &str) -> Result<Option<String>, String> {
            Ok(self.npm.get(package_name).cloned())
        }

        fn remote_git_head(&self, installed_path: &Path) -> Result<Option<String>, String> {
            Ok(self
                .git
                .get(&installed_path.to_string_lossy().to_string())
                .cloned())
        }
    }

    #[test]
    fn plans_npm_update_as_latest_install_for_unpinned_source() {
        let plan = plan_update("/agent", "/work", "npm:pkg", SourceScope::User, None);

        assert_eq!(plan.action, ProgressAction::Update);
        assert_eq!(plan.steps[1].command, "npm");
        assert_eq!(plan.steps[1].args[1], "pkg@latest");
    }

    #[test]
    fn pinned_npm_update_has_no_steps() {
        let plan = plan_update("/agent", "/work", "npm:pkg@1.0.0", SourceScope::User, None);

        assert!(plan.steps.is_empty());
    }

    #[test]
    fn plans_git_update_for_existing_pinned_ref() {
        let root = temp_dir();
        let target = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(&target).expect("target should exist");

        let plan = plan_update(
            &root,
            "/work",
            "git:https://github.com/user/repo@main",
            SourceScope::User,
            None,
        );

        assert_eq!(plan.steps[0].command, "git_ensure_ref");
        assert_eq!(
            plan.steps[0].args,
            vec!["FETCH_HEAD^{commit}", "--", "fetch", "origin", "main"]
        );
    }

    #[test]
    fn plans_git_update_uses_plain_install_when_npm_command_is_configured_like_pi() {
        let root = temp_dir();
        let target = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(&target).expect("target should exist");

        let plan = plan_update(
            &root,
            "/work",
            "git:https://github.com/user/repo@main",
            SourceScope::User,
            Some(NpmCommandConfig {
                command: "npm".to_string(),
                args: Vec::new(),
            }),
        );

        assert_eq!(plan.steps[1].command, "run_if_package_json");
        assert_eq!(plan.steps[1].args, vec!["npm", "install"]);
    }

    #[test]
    fn plans_missing_git_update_keeps_default_npm_omit_dev_like_pi() {
        let root = temp_dir();

        let plan = plan_update(
            &root,
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::User,
            None,
        );

        let install_step = plan.steps.last().expect("install step should exist");
        assert_eq!(install_step.command, "run_if_package_json");
        assert_eq!(install_step.args, vec!["npm", "install", "--omit=dev"]);
    }

    #[test]
    fn detects_npm_and_git_updates_from_checker() {
        let root = temp_dir();
        let npm_path = root.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(&npm_path).expect("npm path should exist");
        fs::write(npm_path.join("package.json"), r#"{"version":"1.0.0"}"#)
            .expect("package json should be written");

        let git_path = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(git_path.join(".git")).expect("git path should exist");
        fs::write(git_path.join(".git").join("HEAD"), "local").expect("head should be written");

        let mut checker = FakeChecker::default();
        checker.npm.insert("pkg".to_string(), "2.0.0".to_string());
        checker
            .git
            .insert(git_path.to_string_lossy().to_string(), "remote".to_string());

        let updates = check_configured_updates(
            &checker,
            &root,
            "/work",
            &[
                ConfiguredUpdateSource {
                    source: "npm:pkg".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "git:https://github.com/user/repo".to_string(),
                    scope: SourceScope::User,
                },
            ],
        );

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].kind, PackageKind::Npm);
        assert_eq!(updates[1].kind, PackageKind::Git);
    }

    #[test]
    fn check_configured_updates_dedupes_sources_with_project_winning_like_pi() {
        let root = temp_dir();
        let cwd = temp_dir();
        let user_npm_path = root.join("npm").join("node_modules").join("pkg");
        let project_npm_path = cwd
            .join(crate::settings_manager::CONFIG_DIR_NAME)
            .join("npm")
            .join("node_modules")
            .join("pkg");
        fs::create_dir_all(&user_npm_path).expect("user npm path should exist");
        fs::create_dir_all(&project_npm_path).expect("project npm path should exist");
        fs::write(user_npm_path.join("package.json"), r#"{"version":"1.0.0"}"#)
            .expect("user package json should be written");
        fs::write(
            project_npm_path.join("package.json"),
            r#"{"version":"1.0.0"}"#,
        )
        .expect("project package json should be written");

        let mut checker = FakeChecker::default();
        checker.npm.insert("pkg".to_string(), "2.0.0".to_string());

        let updates = check_configured_updates(
            &checker,
            &root,
            &cwd,
            &[
                ConfiguredUpdateSource {
                    source: "npm:pkg".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "npm:pkg@1.0.0".to_string(),
                    scope: SourceScope::Project,
                },
            ],
        );

        assert!(
            updates.is_empty(),
            "project pinned package should win over duplicate user package and then be skipped"
        );
    }

    #[test]
    fn detects_git_update_when_local_branch_ref_is_packed_like_pi_rev_parse() {
        let root = temp_dir();
        let git_path = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(git_path.join(".git")).expect("git path should exist");
        fs::write(git_path.join(".git").join("HEAD"), "ref: refs/heads/main\n")
            .expect("head should be written");
        fs::write(
            git_path.join(".git").join("packed-refs"),
            "1111111111111111111111111111111111111111 refs/heads/main\n",
        )
        .expect("packed refs should be written");

        let mut checker = FakeChecker::default();
        checker.git.insert(
            git_path.to_string_lossy().to_string(),
            "2222222222222222222222222222222222222222".to_string(),
        );

        let updates = check_configured_updates(
            &checker,
            &root,
            "/work",
            &[ConfiguredUpdateSource {
                source: "git:https://github.com/user/repo".to_string(),
                scope: SourceScope::User,
            }],
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].kind, PackageKind::Git);
    }

    #[test]
    fn detects_git_update_when_git_metadata_is_file_like_pi_rev_parse() {
        let root = temp_dir();
        let git_path = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        let metadata_dir = root.join("metadata").join("repo.git");
        fs::create_dir_all(&git_path).expect("git path should exist");
        fs::create_dir_all(&metadata_dir).expect("metadata dir should exist");
        fs::write(
            git_path.join(".git"),
            format!("gitdir: {}\n", metadata_dir.to_string_lossy()),
        )
        .expect("git file should be written");
        fs::write(metadata_dir.join("HEAD"), "local").expect("head should be written");

        let mut checker = FakeChecker::default();
        checker
            .git
            .insert(git_path.to_string_lossy().to_string(), "remote".to_string());

        let updates = check_configured_updates(
            &checker,
            &root,
            "/work",
            &[ConfiguredUpdateSource {
                source: "git:https://github.com/user/repo".to_string(),
                scope: SourceScope::User,
            }],
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].kind, PackageKind::Git);
    }

    #[test]
    fn check_configured_updates_skips_pinned_git_sources_like_pi() {
        let root = temp_dir();
        let git_path = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(git_path.join(".git")).expect("git path should exist");
        fs::write(git_path.join(".git").join("HEAD"), "local").expect("head should be written");

        let mut checker = FakeChecker::default();
        checker
            .git
            .insert(git_path.to_string_lossy().to_string(), "remote".to_string());

        let updates = check_configured_updates(
            &checker,
            &root,
            "/work",
            &[ConfiguredUpdateSource {
                source: "git:https://github.com/user/repo@main".to_string(),
                scope: SourceScope::User,
            }],
        );

        assert!(updates.is_empty());
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-package-updates-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
