use super::executor::PackageCommandRunner;
use super::lifecycle::{install, update_with_scope, PackageLifecycleError};
use super::manifest::read_pi_manifest;
use super::paths::display_path;
use super::resolver::{merge_paths, resolve_source_at_path, sort_resolved_paths};
use super::source::{
    git_install_path, managed_npm_install_path, package_source_base_dir, parse_source,
};
use super::types::{
    MissingSourceAction, NpmCommandConfig, PackageFilter, ParsedSource, PathMetadata,
    ProgressEvent, ResolvedPaths, ResolvedResource, SourceOrigin, SourceScope,
};
use crate::utils::paths::resolve_path;
use std::fs;
use std::path::{Path, PathBuf};

pub trait MissingSourceHandler {
    fn on_missing(&mut self, source: &str) -> MissingSourceAction;
}

#[derive(Debug, Clone, Copy)]
pub struct StaticMissingSourceHandler {
    action: MissingSourceAction,
}

impl StaticMissingSourceHandler {
    pub fn new(action: MissingSourceAction) -> Self {
        Self { action }
    }
}

impl MissingSourceHandler for StaticMissingSourceHandler {
    fn on_missing(&mut self, _source: &str) -> MissingSourceAction {
        self.action
    }
}

pub fn resolve_package_sources<R: PackageCommandRunner + ?Sized, H: MissingSourceHandler>(
    runner: &R,
    handler: &mut H,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    sources: &[(String, SourceScope, Option<PackageFilter>)],
    npm_command: Option<NpmCommandConfig>,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<ResolvedPaths, PackageLifecycleError> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let mut resolved = ResolvedPaths::default();
    for (source, scope, filter) in sources {
        let Some(path) = ensure_source_installed(
            runner,
            handler,
            agent_dir,
            cwd,
            source,
            *scope,
            npm_command.clone(),
            &mut on_progress,
        )?
        else {
            continue;
        };
        let is_local = matches!(parse_source(source), ParsedSource::Local(_));
        if is_local && path.is_file() {
            resolved.extensions.push(ResolvedResource {
                path: display_path(&path),
                enabled: true,
                metadata: PathMetadata {
                    source: source.clone(),
                    scope: *scope,
                    origin: SourceOrigin::Package,
                    base_dir: path.parent().map(display_path),
                },
            });
            continue;
        }
        let next = resolve_source_at_path(
            source,
            &path,
            *scope,
            SourceOrigin::Package,
            filter.as_ref(),
        );
        if is_local && local_directory_should_fallback_to_extension(&path, filter.as_ref()) {
            resolved.extensions.push(ResolvedResource {
                path: display_path(&path),
                enabled: true,
                metadata: PathMetadata {
                    source: source.clone(),
                    scope: *scope,
                    origin: SourceOrigin::Package,
                    base_dir: Some(display_path(&path)),
                },
            });
            continue;
        }
        merge_paths(&mut resolved, next);
    }
    sort_resolved_paths(&mut resolved);
    Ok(resolved)
}

pub(super) fn local_directory_should_fallback_to_extension(
    path: &Path,
    filter: Option<&PackageFilter>,
) -> bool {
    if !path.is_dir() || filter.is_some() || read_pi_manifest(path).is_some() {
        return false;
    }
    !["extensions", "skills", "prompts", "themes"]
        .iter()
        .any(|resource_type| path.join(resource_type).exists())
}

fn ensure_source_installed<R: PackageCommandRunner + ?Sized>(
    runner: &R,
    handler: &mut impl MissingSourceHandler,
    agent_dir: &Path,
    cwd: &Path,
    source: &str,
    scope: SourceScope,
    npm_command: Option<NpmCommandConfig>,
    on_progress: &mut impl FnMut(ProgressEvent),
) -> Result<Option<PathBuf>, PackageLifecycleError> {
    match parse_source(source) {
        ParsedSource::Local(local) => {
            let base = package_source_base_dir(agent_dir, cwd, scope);
            let path = resolve_path(&local.path, base, None);
            Ok(path.exists().then_some(path))
        }
        ParsedSource::Npm(npm) => {
            let npm_command = npm_command.unwrap_or_default();
            let mut path = npm_install_path(agent_dir, cwd, &npm, scope, &npm_command);
            if path.exists() && installed_npm_matches_pin(&path, npm.version.as_deref()) {
                return Ok(Some(path));
            }
            install_missing_source(
                runner,
                handler,
                agent_dir,
                cwd,
                source,
                scope,
                Some(npm_command.clone()),
                on_progress,
            )?;
            path = npm_install_path(agent_dir, cwd, &npm, scope, &npm_command);
            Ok(path.exists().then_some(path))
        }
        ParsedSource::Git(git) => {
            let path = git_install_path(agent_dir, cwd, &git, scope);
            if path.exists() {
                if scope == SourceScope::Temporary && git.reference.is_none() {
                    let _ = update_with_scope(
                        runner,
                        agent_dir,
                        cwd,
                        source,
                        SourceScope::Temporary,
                        npm_command,
                        on_progress,
                    );
                }
                return Ok(Some(path));
            }
            install_missing_source(
                runner,
                handler,
                agent_dir,
                cwd,
                source,
                scope,
                npm_command,
                on_progress,
            )?;
            Ok(path.exists().then_some(path))
        }
    }
}

fn npm_install_path(
    agent_dir: &Path,
    cwd: &Path,
    source: &super::types::NpmSource,
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
) -> PathBuf {
    let managed_path = managed_npm_install_path(agent_dir, cwd, source, scope);
    if scope != SourceScope::User || managed_path.exists() {
        return managed_path;
    }
    super::legacy_global_npm_package_path(source, cwd, npm_command)
        .filter(|path| path.exists())
        .unwrap_or(managed_path)
}

fn installed_npm_matches_pin(path: &Path, pinned_version: Option<&str>) -> bool {
    let Some(pinned_version) = pinned_version else {
        return true;
    };
    installed_npm_version(path).as_deref() == Some(pinned_version)
}

fn installed_npm_version(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path.join("package.json")).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&value).ok()?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn install_missing_source<R: PackageCommandRunner + ?Sized>(
    runner: &R,
    handler: &mut impl MissingSourceHandler,
    agent_dir: &Path,
    cwd: &Path,
    source: &str,
    scope: SourceScope,
    npm_command: Option<NpmCommandConfig>,
    on_progress: &mut impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    match handler.on_missing(source) {
        MissingSourceAction::Skip => Ok(()),
        MissingSourceAction::Error => Err(PackageLifecycleError::CommandFailed(format!(
            "Missing source: {source}"
        ))),
        MissingSourceAction::Install => install(
            runner,
            agent_dir,
            cwd,
            source,
            scope == SourceScope::Project,
            npm_command,
            on_progress,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_manager::executor::CommandExecution;
    use crate::package_manager::source::git_install_path;
    use crate::package_manager::types::PackageCommandStep;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<PackageCommandStep>>,
        fail: bool,
    }

    impl PackageCommandRunner for FakeRunner {
        fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
            self.calls.borrow_mut().push(step.clone());
            if self.fail {
                return Err("refresh failed".to_string());
            }
            Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            })
        }
    }

    struct Handler {
        action: MissingSourceAction,
        seen: Vec<String>,
    }

    impl MissingSourceHandler for Handler {
        fn on_missing(&mut self, source: &str) -> MissingSourceAction {
            self.seen.push(source.to_string());
            self.action
        }
    }

    #[test]
    fn resolves_existing_local_package_resources() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("prompts")).expect("prompts dir");
        fs::write(dir.join("prompts").join("review.md"), "").expect("prompt write");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[(dir.to_string_lossy().to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert_eq!(resolved.prompts.len(), 1);
        assert!(handler.seen.is_empty());
    }

    #[test]
    fn local_single_file_package_source_is_extension_like_pi() {
        let dir = temp_dir();
        let file = dir.join("extension.md");
        fs::write(&file, "").expect("extension write");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[(file.to_string_lossy().to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        let expected_base_dir = file.parent().map(|path| path.to_string_lossy().to_string());
        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.extensions[0].path, file.to_string_lossy());
        assert_eq!(
            resolved.extensions[0].metadata.source,
            file.to_string_lossy()
        );
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::User);
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert_eq!(
            resolved.extensions[0].metadata.base_dir.as_deref(),
            expected_base_dir.as_deref()
        );
        assert!(resolved.prompts.is_empty());
        assert!(handler.seen.is_empty());
    }

    #[test]
    fn local_directory_package_without_resources_is_extension_like_pi() {
        let dir = temp_dir();
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[(
                dir.to_string_lossy().to_string(),
                SourceScope::Project,
                None,
            )],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.extensions[0].path, dir.to_string_lossy());
        assert_eq!(
            resolved.extensions[0].metadata.source,
            dir.to_string_lossy()
        );
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::Project);
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert_eq!(
            resolved.extensions[0].metadata.base_dir.as_deref(),
            Some(dir.to_string_lossy().as_ref())
        );
        assert!(resolved.prompts.is_empty());
        assert!(handler.seen.is_empty());
    }

    #[test]
    fn local_directory_package_resources_keep_configured_source_metadata_like_pi() {
        let cwd = temp_dir();
        let package_dir = cwd.join("package");
        let extension_file = package_dir.join("extensions").join("index.ts");
        fs::create_dir_all(extension_file.parent().expect("extension parent"))
            .expect("extensions dir");
        fs::write(&extension_file, "").expect("extension write");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            &cwd,
            &[("../package".to_string(), SourceScope::Project, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(
            resolved.extensions[0].path,
            extension_file.to_string_lossy()
        );
        assert_eq!(resolved.extensions[0].metadata.source, "../package");
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::Project);
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert_eq!(
            resolved.extensions[0].metadata.base_dir.as_deref(),
            Some(package_dir.to_string_lossy().as_ref())
        );
        assert!(handler.seen.is_empty());
    }

    #[test]
    fn project_local_package_source_resolves_from_project_config_dir_like_pi() {
        let cwd = temp_dir();
        let package_dir = cwd.join("packages").join("demo");
        let extension_file = package_dir.join("extensions").join("index.ts");
        fs::create_dir_all(extension_file.parent().expect("extension parent"))
            .expect("extensions dir");
        fs::write(&extension_file, "").expect("extension write");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            &cwd,
            &[("../packages/demo".to_string(), SourceScope::Project, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(
            resolved.extensions[0].path,
            extension_file.to_string_lossy()
        );
        assert_eq!(resolved.extensions[0].metadata.source, "../packages/demo");
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::Project);
        assert_eq!(
            resolved.extensions[0].metadata.base_dir.as_deref(),
            Some(package_dir.to_string_lossy().as_ref())
        );
        assert!(handler.seen.is_empty());
    }

    #[test]
    fn missing_remote_source_can_be_skipped() {
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Skip,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[("npm:pkg".to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("skip should not fail");

        assert!(resolved.prompts.is_empty());
        assert_eq!(handler.seen, vec!["npm:pkg"]);
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn user_npm_resolve_uses_legacy_global_install_path_like_pi() {
        let cwd = temp_dir();
        let agent_dir = temp_dir();
        let global_root = temp_dir().join("global-node-modules");
        let package_dir = global_root.join("pkg");
        fs::create_dir_all(package_dir.join("prompts")).expect("global package prompts dir");
        fs::write(package_dir.join("prompts").join("review.md"), "").expect("prompt write");
        let npm_command = fake_npm_root_command(&global_root);
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            &agent_dir,
            &cwd,
            &[("npm:pkg".to_string(), SourceScope::User, None)],
            Some(npm_command),
            |_| {},
        )
        .expect("resolve should use legacy global package");

        assert_eq!(resolved.prompts.len(), 1);
        assert_eq!(handler.seen, Vec::<String>::new());
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn missing_remote_source_can_error() {
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let error = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[("npm:pkg".to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .unwrap_err();

        assert_eq!(
            error,
            PackageLifecycleError::CommandFailed("Missing source: npm:pkg".to_string())
        );
    }

    #[test]
    fn missing_remote_source_can_install() {
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Install,
            seen: Vec::new(),
        };

        let _ = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            "/work",
            &[("npm:pkg".to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("install plan should execute");

        assert_eq!(handler.seen, vec!["npm:pkg"]);
        assert_eq!(runner.calls.borrow().len(), 2);
    }

    #[test]
    fn pinned_npm_matching_installed_version_does_not_reinstall() {
        let agent_dir = temp_dir();
        let installed = agent_dir.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(&installed).expect("installed dir");
        fs::write(installed.join("package.json"), r#"{"version":"1.0.0"}"#).expect("package json");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Install,
            seen: Vec::new(),
        };

        let _ = resolve_package_sources(
            &runner,
            &mut handler,
            &agent_dir,
            "/work",
            &[("npm:pkg@1.0.0".to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert!(handler.seen.is_empty());
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn pinned_npm_mismatched_installed_version_reinstalls() {
        let agent_dir = temp_dir();
        let installed = agent_dir.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(&installed).expect("installed dir");
        fs::write(installed.join("package.json"), r#"{"version":"1.0.0"}"#).expect("package json");
        let runner = FakeRunner::default();
        let mut handler = Handler {
            action: MissingSourceAction::Install,
            seen: Vec::new(),
        };

        let _ = resolve_package_sources(
            &runner,
            &mut handler,
            &agent_dir,
            "/work",
            &[("npm:pkg@2.0.0".to_string(), SourceScope::User, None)],
            None,
            |_| {},
        )
        .expect("resolve should succeed");

        assert_eq!(handler.seen, vec!["npm:pkg@2.0.0"]);
        assert_eq!(runner.calls.borrow().len(), 2);
    }

    #[test]
    fn temporary_unpinned_git_refresh_failure_keeps_cached_checkout() {
        let cwd = temp_dir();
        let repo_name = format!(
            "repo-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        );
        let target = match parse_source(&format!("git:https://github.com/user/{repo_name}")) {
            ParsedSource::Git(source) => {
                git_install_path("/agent", &cwd, &source, SourceScope::Temporary)
            }
            other => panic!("expected git source, got {other:?}"),
        };
        fs::create_dir_all(target.join("prompts")).expect("git target prompts");
        fs::write(target.join("prompts").join("review.md"), "").expect("resource write");
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            fail: true,
        };
        let mut handler = Handler {
            action: MissingSourceAction::Error,
            seen: Vec::new(),
        };

        let resolved = resolve_package_sources(
            &runner,
            &mut handler,
            "/agent",
            &cwd,
            &[(
                format!("git:https://github.com/user/{repo_name}"),
                SourceScope::Temporary,
                None,
            )],
            None,
            |_| {},
        )
        .expect("refresh failure should keep cached checkout");

        assert_eq!(resolved.prompts.len(), 1);
        assert_eq!(runner.calls.borrow().len(), 1);
        assert!(handler.seen.is_empty());
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-resolve-runtime-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn fake_npm_root_command(global_root: &Path) -> NpmCommandConfig {
        let bin_dir = temp_dir().join("bin");
        fs::create_dir_all(&bin_dir).expect("fake npm bin dir");
        let command = bin_dir.join("npm");
        fs::write(
            &command,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"root\" ] && [ \"$2\" = \"-g\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 1\n",
                global_root.to_string_lossy()
            ),
        )
        .expect("fake npm command");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&command)
                .expect("fake npm metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&command, permissions).expect("fake npm executable");
        }

        NpmCommandConfig {
            command: command.to_string_lossy().to_string(),
            args: Vec::new(),
        }
    }
}
