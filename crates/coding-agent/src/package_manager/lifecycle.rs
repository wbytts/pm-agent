use super::configured::{configured_update_sources, npm_command_from_settings};
use super::executor::PackageCommandRunner;
use super::operations::{plan_install, plan_npm_batch_update, plan_remove, progress_verb};
use super::settings::{add_source_to_settings, remove_source_from_settings};
use super::source::{managed_npm_install_path, parse_source};
use super::types::{
    NpmCommandConfig, NpmSource, ParsedSource, ProgressAction, ProgressEvent, ProgressEventKind,
    SourceScope,
};
use super::update_checker::CommandUpdateChecker;
use super::updates::{installed_npm_version, plan_update, ConfiguredUpdateSource, UpdateCheck};
use crate::settings_manager::{SettingsManager, SettingsStorage};
use crate::utils::paths::resolve_path;
use std::path::Path;
use thiserror::Error;

struct NpmUpdateEntry {
    source: String,
    spec: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackageLifecycleError {
    #[error("本地路径不存在：{0}")]
    MissingLocalPath(String),
    #[error("{0}")]
    CommandFailed(String),
}

pub fn install<R: PackageCommandRunner + ?Sized>(
    runner: &R,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let scope = scope_from_local(local);
    validate_local_install(cwd.as_ref(), source, scope)?;
    let plan = plan_install(agent_dir, cwd, source, scope, npm_command);
    execute_plan(runner, &plan, on_progress)
}

pub fn remove<R: PackageCommandRunner>(
    runner: &R,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let scope = scope_from_local(local);
    let plan = plan_remove(agent_dir, cwd, source, scope, npm_command);
    execute_plan(runner, &plan, on_progress)
}

pub fn install_and_persist<S: SettingsStorage, R: PackageCommandRunner>(
    runner: &R,
    settings: &mut SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    install(
        runner,
        agent_dir.as_ref(),
        cwd.as_ref(),
        source,
        local,
        npm_command,
        on_progress,
    )?;
    add_source_to_settings(settings, agent_dir, cwd, source, local);
    Ok(())
}

pub fn remove_and_persist<S: SettingsStorage, R: PackageCommandRunner>(
    runner: &R,
    settings: &mut SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<bool, PackageLifecycleError> {
    remove(
        runner,
        agent_dir.as_ref(),
        cwd.as_ref(),
        source,
        local,
        npm_command,
        on_progress,
    )?;
    Ok(remove_source_from_settings(
        settings, agent_dir, cwd, source, local,
    ))
}

pub fn update<R: PackageCommandRunner>(
    runner: &R,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let scope = scope_from_local(local);
    update_with_scope(
        runner,
        agent_dir,
        cwd,
        source,
        scope,
        npm_command,
        on_progress,
    )
}

pub fn update_with_scope<R: PackageCommandRunner + ?Sized>(
    runner: &R,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
    npm_command: Option<NpmCommandConfig>,
    on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let plan = plan_update(agent_dir, cwd, source, scope, npm_command);
    execute_plan(runner, &plan, on_progress)
}

pub fn update_configured_sources<R: PackageCommandRunner>(
    runner: &R,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    sources: &[ConfiguredUpdateSource],
    npm_command: Option<NpmCommandConfig>,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let npm_command = npm_command.unwrap_or_default();
    let mut user_npm_updates = Vec::new();
    let mut project_npm_updates = Vec::new();
    let mut git_sources = Vec::new();

    for entry in sources {
        if entry.scope == SourceScope::Temporary {
            continue;
        }
        match parse_source(&entry.source) {
            ParsedSource::Npm(npm) => {
                if npm.pinned {
                    continue;
                }
                let update = NpmUpdateEntry {
                    source: entry.source.clone(),
                    spec: format!("{}@latest", npm.name),
                };
                match entry.scope {
                    SourceScope::User => user_npm_updates.push(update),
                    SourceScope::Project => project_npm_updates.push(update),
                    SourceScope::Temporary => {}
                }
            }
            ParsedSource::Git(_) => git_sources.push(entry),
            ParsedSource::Local(_) => {}
        }
    }

    if !user_npm_updates.is_empty() {
        let specs = npm_update_specs(&user_npm_updates);
        let label = npm_batch_source_label(&user_npm_updates, SourceScope::User);
        let plan = plan_npm_batch_update(
            agent_dir.as_ref(),
            cwd.as_ref(),
            &specs,
            SourceScope::User,
            &npm_command,
            label,
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }
    if !project_npm_updates.is_empty() {
        let specs = npm_update_specs(&project_npm_updates);
        let label = npm_batch_source_label(&project_npm_updates, SourceScope::Project);
        let plan = plan_npm_batch_update(
            agent_dir.as_ref(),
            cwd.as_ref(),
            &specs,
            SourceScope::Project,
            &npm_command,
            label,
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }

    for entry in git_sources {
        let plan = plan_update(
            agent_dir.as_ref(),
            cwd.as_ref(),
            &entry.source,
            entry.scope,
            Some(npm_command.clone()),
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }
    Ok(())
}

fn update_configured_sources_with_checker<R: PackageCommandRunner, C: UpdateCheck>(
    runner: &R,
    checker: &C,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    sources: &[ConfiguredUpdateSource],
    npm_command: Option<NpmCommandConfig>,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let npm_command = npm_command.unwrap_or_default();
    let mut user_npm_updates = Vec::new();
    let mut project_npm_updates = Vec::new();
    let mut git_sources = Vec::new();

    for entry in sources {
        if entry.scope == SourceScope::Temporary {
            continue;
        }
        match parse_source(&entry.source) {
            ParsedSource::Npm(npm) => {
                if npm.pinned
                    || !should_update_npm_source(
                        checker,
                        agent_dir,
                        cwd,
                        &npm,
                        entry.scope,
                        |npm| super::legacy_global_npm_package_path(npm, cwd, &npm_command),
                    )
                {
                    continue;
                }
                let update = NpmUpdateEntry {
                    source: entry.source.clone(),
                    spec: format!("{}@latest", npm.name),
                };
                match entry.scope {
                    SourceScope::User => user_npm_updates.push(update),
                    SourceScope::Project => project_npm_updates.push(update),
                    SourceScope::Temporary => {}
                }
            }
            ParsedSource::Git(_) => git_sources.push(entry),
            ParsedSource::Local(_) => {}
        }
    }

    if !user_npm_updates.is_empty() {
        let specs = npm_update_specs(&user_npm_updates);
        let label = npm_batch_source_label(&user_npm_updates, SourceScope::User);
        let plan = plan_npm_batch_update(
            agent_dir,
            cwd,
            &specs,
            SourceScope::User,
            &npm_command,
            label,
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }
    if !project_npm_updates.is_empty() {
        let specs = npm_update_specs(&project_npm_updates);
        let label = npm_batch_source_label(&project_npm_updates, SourceScope::Project);
        let plan = plan_npm_batch_update(
            agent_dir,
            cwd,
            &specs,
            SourceScope::Project,
            &npm_command,
            label,
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }

    for entry in git_sources {
        let plan = plan_update(
            agent_dir,
            cwd,
            &entry.source,
            entry.scope,
            Some(npm_command.clone()),
        );
        execute_plan(runner, &plan, &mut on_progress)?;
    }
    Ok(())
}

fn should_update_npm_source<C, F>(
    checker: &C,
    agent_dir: &Path,
    cwd: &Path,
    source: &NpmSource,
    scope: SourceScope,
    legacy_npm_path: F,
) -> bool
where
    C: UpdateCheck,
    F: FnOnce(&NpmSource) -> Option<std::path::PathBuf>,
{
    let installed_path = npm_update_installed_path(agent_dir, cwd, source, scope, legacy_npm_path);
    let Some(installed_version) = installed_npm_version(&installed_path) else {
        return true;
    };

    match checker.latest_npm_version(&source.name) {
        Ok(Some(latest)) => latest != installed_version,
        Ok(None) | Err(_) => true,
    }
}

fn npm_update_installed_path<F>(
    agent_dir: &Path,
    cwd: &Path,
    source: &NpmSource,
    scope: SourceScope,
    legacy_npm_path: F,
) -> std::path::PathBuf
where
    F: FnOnce(&NpmSource) -> Option<std::path::PathBuf>,
{
    let managed_path = managed_npm_install_path(agent_dir, cwd, source, scope);
    if scope != SourceScope::User || managed_path.exists() {
        return managed_path;
    }
    legacy_npm_path(source)
        .filter(|path| path.exists())
        .unwrap_or(managed_path)
}

pub fn update_from_settings<S: SettingsStorage, R: PackageCommandRunner>(
    runner: &R,
    settings: &SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source_filter: Option<&str>,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    if is_offline_mode_enabled() {
        return Ok(());
    }
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let sources = configured_update_sources(settings, agent_dir, cwd, source_filter)
        .map_err(PackageLifecycleError::CommandFailed)?;
    let npm_command = npm_command_from_settings(settings);
    let checker = CommandUpdateChecker::new(cwd, npm_command.clone());
    update_configured_sources_with_checker(
        runner,
        &checker,
        agent_dir,
        cwd,
        &sources,
        npm_command,
        &mut on_progress,
    )
}

fn execute_plan<R: PackageCommandRunner + ?Sized>(
    runner: &R,
    plan: &super::types::PackageOperationPlan,
    mut on_progress: impl FnMut(ProgressEvent),
) -> Result<(), PackageLifecycleError> {
    emit(
        &mut on_progress,
        ProgressEventKind::Start,
        plan.action,
        &plan.source,
        Some(format!("{} {}...", progress_verb(plan.action), plan.source)),
    );

    let mut skip_remaining_steps = false;
    for step in &plan.steps {
        if skip_remaining_steps {
            break;
        }
        let execution = match runner.run(step) {
            Ok(execution) => execution,
            Err(error) => {
                emit(
                    &mut on_progress,
                    ProgressEventKind::Error,
                    plan.action,
                    &plan.source,
                    Some(error.clone()),
                );
                return Err(PackageLifecycleError::CommandFailed(error));
            }
        };
        if step.command == "git_ensure_ref" && execution.stdout.trim() == "unchanged" {
            skip_remaining_steps = true;
        }
        emit(
            &mut on_progress,
            ProgressEventKind::Progress,
            plan.action,
            &plan.source,
            Some(format!("已执行：{} {}", step.command, step.args.join(" "))),
        );
    }

    emit(
        &mut on_progress,
        ProgressEventKind::Complete,
        plan.action,
        &plan.source,
        None,
    );
    Ok(())
}

fn validate_local_install(
    cwd: &Path,
    source: &str,
    scope: SourceScope,
) -> Result<(), PackageLifecycleError> {
    let ParsedSource::Local(local) = parse_source(source) else {
        return Ok(());
    };
    let base = match scope {
        SourceScope::Project | SourceScope::Temporary | SourceScope::User => cwd,
    };
    let resolved = resolve_path(&local.path, base, None);
    if resolved.exists() {
        Ok(())
    } else {
        Err(PackageLifecycleError::MissingLocalPath(
            resolved.to_string_lossy().to_string(),
        ))
    }
}

fn scope_from_local(local: bool) -> SourceScope {
    if local {
        SourceScope::Project
    } else {
        SourceScope::User
    }
}

pub(super) fn is_offline_mode_enabled() -> bool {
    std::env::var("PI_OFFLINE")
        .map(|value| is_offline_value(&value))
        .unwrap_or(false)
}

fn is_offline_value(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

fn npm_update_specs(updates: &[NpmUpdateEntry]) -> Vec<String> {
    updates.iter().map(|entry| entry.spec.clone()).collect()
}

fn npm_batch_source_label(updates: &[NpmUpdateEntry], scope: SourceScope) -> String {
    if updates.len() == 1 {
        return updates[0].source.clone();
    }
    match scope {
        SourceScope::User => "user npm packages".to_string(),
        SourceScope::Project => "project npm packages".to_string(),
        SourceScope::Temporary => "temporary npm packages".to_string(),
    }
}

fn emit(
    on_progress: &mut impl FnMut(ProgressEvent),
    kind: ProgressEventKind,
    action: ProgressAction,
    source: &str,
    message: Option<String>,
) {
    on_progress(ProgressEvent {
        kind,
        action,
        source: source.to_string(),
        message,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_manager::executor::CommandExecution;
    use crate::package_manager::types::{PackageCommandStep, ProgressEventKind};
    use crate::settings_manager::{InMemorySettingsStorage, SettingsManager};
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<PackageCommandStep>>,
        fail: Option<String>,
        stdout: String,
    }

    impl PackageCommandRunner for FakeRunner {
        fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
            self.calls.borrow_mut().push(step.clone());
            if let Some(error) = &self.fail {
                return Err(error.clone());
            }
            Ok(CommandExecution {
                step: step.clone(),
                stdout: self.stdout.clone(),
                stderr: String::new(),
                code: 0,
            })
        }
    }

    struct FakeUpdateChecker {
        latest_npm_version: Result<Option<String>, String>,
    }

    impl UpdateCheck for FakeUpdateChecker {
        fn latest_npm_version(&self, _package_name: &str) -> Result<Option<String>, String> {
            self.latest_npm_version.clone()
        }

        fn remote_git_head(&self, _installed_path: &Path) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[test]
    fn install_executes_plan_and_emits_progress() {
        let runner = FakeRunner::default();
        let mut events = Vec::new();

        install(
            &runner,
            "/agent",
            "/work",
            "npm:@scope/pkg",
            false,
            None,
            |event| events.push(event),
        )
        .expect("install should succeed");

        assert_eq!(runner.calls.borrow().len(), 2);
        assert_eq!(
            events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<ProgressEventKind>>(),
            vec![
                ProgressEventKind::Start,
                ProgressEventKind::Progress,
                ProgressEventKind::Progress,
                ProgressEventKind::Complete
            ]
        );
    }

    #[test]
    fn install_reports_command_error_and_does_not_complete() {
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            fail: Some("boom".to_string()),
            stdout: String::new(),
        };
        let mut events = Vec::new();

        let error = install(
            &runner,
            "/agent",
            "/work",
            "npm:pkg",
            false,
            None,
            |event| events.push(event),
        )
        .unwrap_err();

        assert_eq!(
            error,
            PackageLifecycleError::CommandFailed("boom".to_string())
        );
        assert_eq!(
            events.last().map(|event| event.kind),
            Some(ProgressEventKind::Error)
        );
    }

    #[test]
    fn local_install_validates_path_without_running_commands() {
        let cwd = temp_dir();
        let local = cwd.join("local-extension");
        fs::create_dir_all(&local).expect("local dir should be created");
        let runner = FakeRunner::default();

        install(
            &runner,
            "/agent",
            &cwd,
            "local-extension",
            true,
            None,
            |_| {},
        )
        .expect("local install should succeed");

        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn user_local_install_resolves_relative_path_from_cwd_like_pi() {
        let cwd = temp_dir();
        let local = cwd.join("user-local-extension");
        fs::create_dir_all(&local).expect("local dir should be created");
        let runner = FakeRunner::default();

        install(
            &runner,
            "/agent",
            &cwd,
            "user-local-extension",
            false,
            None,
            |_| {},
        )
        .expect("user local install should resolve from cwd");

        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn install_and_persist_writes_settings_after_success() {
        let runner = FakeRunner::default();
        let mut settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));

        install_and_persist(
            &runner,
            &mut settings,
            "/agent",
            "/work",
            "npm:pkg",
            false,
            None,
            |_| {},
        )
        .expect("install should persist");

        assert_eq!(
            settings.get_global_packages(),
            vec![Value::String("npm:pkg".to_string())]
        );
    }

    #[test]
    fn install_and_persist_keeps_settings_unchanged_after_failure() {
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            fail: Some("boom".to_string()),
            stdout: String::new(),
        };
        let mut settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));

        let _ = install_and_persist(
            &runner,
            &mut settings,
            "/agent",
            "/work",
            "npm:pkg",
            false,
            None,
            |_| {},
        );

        assert!(settings.get_global_packages().is_empty());
    }

    #[test]
    fn remove_and_persist_updates_settings_after_success() {
        let runner = FakeRunner::default();
        let mut settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));

        let changed = remove_and_persist(
            &runner,
            &mut settings,
            "/agent",
            "/work",
            "npm:pkg",
            false,
            None,
            |_| {},
        )
        .expect("remove should succeed");

        assert!(changed);
        assert!(settings.get_global_packages().is_empty());
    }

    #[test]
    fn update_executes_update_plan() {
        let runner = FakeRunner::default();
        let mut events = Vec::new();

        update(
            &runner,
            "/agent",
            "/work",
            "npm:pkg",
            false,
            None,
            |event| events.push(event),
        )
        .expect("update should succeed");

        assert_eq!(runner.calls.borrow().len(), 2);
        assert_eq!(events[0].action, ProgressAction::Update);
    }

    #[test]
    fn unchanged_git_update_skips_following_dependency_install() {
        let root = temp_dir();
        let target = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(&target).expect("git target should exist");
        fs::write(target.join("package.json"), "{}").expect("package json should write");
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            fail: None,
            stdout: "unchanged".to_string(),
        };

        update(
            &runner,
            &root,
            "/work",
            "git:https://github.com/user/repo@main",
            false,
            None,
            |_| {},
        )
        .expect("update should succeed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].command, "git_ensure_ref");
    }

    #[test]
    fn changed_git_update_checks_package_json_after_ref_update_like_pi() {
        struct GitUpdateAddsPackageRunner {
            calls: RefCell<Vec<PackageCommandStep>>,
        }

        impl PackageCommandRunner for GitUpdateAddsPackageRunner {
            fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
                self.calls.borrow_mut().push(step.clone());
                if step.command == "git_ensure_ref" {
                    let cwd = step.cwd.as_deref().expect("git update should have cwd");
                    fs::write(Path::new(cwd).join("package.json"), "{}")
                        .expect("package json should be written after git update");
                    return Ok(CommandExecution {
                        step: step.clone(),
                        stdout: "changed".to_string(),
                        stderr: String::new(),
                        code: 0,
                    });
                }
                Ok(CommandExecution {
                    step: step.clone(),
                    stdout: String::new(),
                    stderr: String::new(),
                    code: 0,
                })
            }
        }

        let root = temp_dir();
        let target = root
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(&target).expect("git target should exist");
        let runner = GitUpdateAddsPackageRunner {
            calls: RefCell::new(Vec::new()),
        };

        update(
            &runner,
            &root,
            "/work",
            "git:https://github.com/user/repo@main",
            false,
            None,
            |_| {},
        )
        .expect("update should succeed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].command, "git_ensure_ref");
        assert_eq!(calls[1].command, "run_if_package_json");
        assert_eq!(calls[1].args, vec!["npm", "install", "--omit=dev"]);
        assert_eq!(
            calls[1].cwd.as_deref(),
            Some(display_path(&target).as_str())
        );
    }

    #[test]
    fn git_install_skips_dependency_install_when_cloned_repo_has_no_package_json_like_pi() {
        struct CloneWithoutPackageRunner {
            calls: RefCell<Vec<PackageCommandStep>>,
        }

        impl PackageCommandRunner for CloneWithoutPackageRunner {
            fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
                self.calls.borrow_mut().push(step.clone());
                if step.command == "git" && step.args.first().is_some_and(|arg| arg == "clone") {
                    let target = step.args.get(2).expect("clone target should exist");
                    fs::create_dir_all(target).expect("clone target should be created");
                }
                Ok(CommandExecution {
                    step: step.clone(),
                    stdout: String::new(),
                    stderr: String::new(),
                    code: 0,
                })
            }
        }

        let root = temp_dir();
        let runner = CloneWithoutPackageRunner {
            calls: RefCell::new(Vec::new()),
        };

        install(
            &runner,
            &root,
            "/work",
            "git:https://github.com/user/repo",
            false,
            None,
            |_| {},
        )
        .expect("git install should succeed");

        assert!(
            runner
                .calls
                .borrow()
                .iter()
                .all(|step| step.command != "npm"),
            "npm install should be skipped without package.json"
        );
    }

    #[test]
    fn update_configured_sources_runs_each_non_temporary_source() {
        let runner = FakeRunner::default();

        update_configured_sources(
            &runner,
            "/agent",
            "/work",
            &[
                ConfiguredUpdateSource {
                    source: "npm:a".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "npm:b".to_string(),
                    scope: SourceScope::Temporary,
                },
                ConfiguredUpdateSource {
                    source: "npm:c".to_string(),
                    scope: SourceScope::Project,
                },
            ],
            None,
            |_| {},
        )
        .expect("configured updates should succeed");

        assert_eq!(runner.calls.borrow().len(), 4);
    }

    #[test]
    fn update_configured_sources_batches_unpinned_npm_by_scope_like_pi() {
        let runner = FakeRunner::default();

        update_configured_sources(
            &runner,
            "/agent",
            "/work",
            &[
                ConfiguredUpdateSource {
                    source: "npm:a".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "npm:b".to_string(),
                    scope: SourceScope::User,
                },
            ],
            None,
            |_| {},
        )
        .expect("configured updates should succeed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].command, "ensure_npm_project");
        assert_eq!(calls[0].args, vec!["/agent/npm"]);
        assert_eq!(calls[1].command, "npm");
        assert_eq!(
            calls[1].args,
            vec![
                "install",
                "a@latest",
                "b@latest",
                "--prefix",
                "/agent/npm",
                "--legacy-peer-deps"
            ]
        );
    }

    #[test]
    fn update_configured_sources_keeps_single_npm_progress_source_like_pi() {
        let runner = FakeRunner::default();
        let mut events = Vec::new();

        update_configured_sources(
            &runner,
            "/agent",
            "/work",
            &[ConfiguredUpdateSource {
                source: "npm:pkg".to_string(),
                scope: SourceScope::User,
            }],
            None,
            |event| events.push(event),
        )
        .expect("configured updates should succeed");

        assert_eq!(events[0].source, "npm:pkg");
        assert_eq!(events[0].message.as_deref(), Some("Updating npm:pkg..."));
    }

    #[test]
    fn update_from_settings_reads_packages_and_npm_command() {
        let runner = FakeRunner::default();
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"],
            "npmCommand": ["corepack", "pnpm"]
        }));

        update_from_settings(&runner, &settings, "/agent", "/work", None, |_| {})
            .expect("settings update should succeed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].command, "corepack");
        assert_eq!(calls[1].args[0], "pnpm");
    }

    #[test]
    fn checked_configured_update_skips_unpinned_npm_when_installed_version_matches_latest_like_pi()
    {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let installed_path = agent_dir.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(&installed_path).expect("installed package should exist");
        fs::write(
            installed_path.join("package.json"),
            r#"{"version":"1.0.0"}"#,
        )
        .expect("package json should write");
        let checker = FakeUpdateChecker {
            latest_npm_version: Ok(Some("1.0.0".to_string())),
        };
        let runner = FakeRunner::default();

        update_configured_sources_with_checker(
            &runner,
            &checker,
            &agent_dir,
            &cwd,
            &[ConfiguredUpdateSource {
                source: "npm:pkg".to_string(),
                scope: SourceScope::User,
            }],
            None,
            |_| {},
        )
        .expect("configured update should succeed");

        assert!(
            runner.calls.borrow().is_empty(),
            "npm install should be skipped when installed version is already latest"
        );
    }

    #[test]
    fn update_from_settings_uses_legacy_global_npm_path_like_pi() {
        let root = temp_dir();
        let cwd = temp_dir();
        let global_root = root.join("global").join("node_modules");
        let installed_path = global_root.join("pkg");
        fs::create_dir_all(&installed_path).expect("installed package should exist");
        fs::write(
            installed_path.join("package.json"),
            r#"{"version":"1.0.0"}"#,
        )
        .expect("package json should write");
        let npm_command = fake_npm_root_and_view_command(&global_root, "1.0.0");
        let mut storage = InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                json!({
                    "packages": ["npm:pkg"],
                    "npmCommand": npm_command
                })
                .to_string(),
            )
            .expect("global settings write");
        let settings = SettingsManager::from_storage(storage);
        let runner = FakeRunner::default();

        update_from_settings(&runner, &settings, &root.join("agent"), &cwd, None, |_| {})
            .expect("settings update should succeed");

        assert!(
            runner.calls.borrow().is_empty(),
            "npm install should be skipped when legacy global install is already latest"
        );
    }

    #[test]
    fn checked_configured_update_updates_unpinned_npm_when_latest_lookup_fails_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let installed_path = agent_dir.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(&installed_path).expect("installed package should exist");
        fs::write(
            installed_path.join("package.json"),
            r#"{"version":"1.0.0"}"#,
        )
        .expect("package json should write");
        let checker = FakeUpdateChecker {
            latest_npm_version: Err("network".to_string()),
        };
        let runner = FakeRunner::default();

        update_configured_sources_with_checker(
            &runner,
            &checker,
            &agent_dir,
            &cwd,
            &[ConfiguredUpdateSource {
                source: "npm:pkg".to_string(),
                scope: SourceScope::User,
            }],
            None,
            |_| {},
        )
        .expect("configured update should succeed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].args[1], "pkg@latest");
    }

    #[test]
    fn update_from_settings_filters_project_local_package_by_resolved_path_like_pi() {
        let runner = FakeRunner::default();
        let mut storage = InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":["../packages/demo"]}"#.to_string(),
            )
            .expect("project settings write");
        let settings = SettingsManager::from_storage(storage);

        update_from_settings(
            &runner,
            &settings,
            "/agent",
            "/work",
            Some("/work/packages/demo"),
            |_| {},
        )
        .expect("resolved project local package filter should match");

        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn offline_value_matches_pi_env_values() {
        assert!(is_offline_value("1"));
        assert!(is_offline_value("true"));
        assert!(is_offline_value("YES"));
        assert!(!is_offline_value("0"));
        assert!(!is_offline_value(""));
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-package-lifecycle-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn fake_npm_root_and_view_command(global_root: &Path, latest_version: &str) -> Vec<String> {
        let script = format!(
            r#"case "$0" in
root) printf '{}\n' ;;
view) printf '"{}"\n' ;;
*) exit 1 ;;
esac"#,
            global_root.to_string_lossy(),
            latest_version
        );
        vec!["sh".to_string(), "-c".to_string(), script]
    }

    #[allow(dead_code)]
    fn display_path(path: impl AsRef<Path>) -> String {
        path.as_ref().to_string_lossy().to_string()
    }
}
