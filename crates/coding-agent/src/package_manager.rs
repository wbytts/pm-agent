mod cli;
mod configured;
mod executor;
mod git_update;
mod lifecycle;
mod manifest;
mod operations;
mod paths;
mod patterns;
mod resolve_runtime;
mod resolver;
mod settings;
mod source;
mod types;
mod update_checker;
mod updates;

use crate::exec::{exec_command, ExecOptions};
use crate::settings_manager::{SettingsManager, SettingsStorage, CONFIG_DIR_NAME};
use paths::{display_path, local_source_path};
use resolve_runtime::local_directory_should_fallback_to_extension;
use resolver::{merge_paths, sort_resolved_paths};
use std::path::{Path, PathBuf};

pub use cli::{
    detect_install_method_from_context, entrypoint_package_dir, format_package_list,
    global_package_roots_from_plan, global_package_roots_plan,
    infer_npm_global_install_from_package_dir, is_managed_by_global_package_manager,
    package_command_actions, package_command_error_messages, package_command_help,
    package_command_usage, parse_package_command, path_comparison_candidates, plan_self_update,
    run_self_update_prepare_actions, self_update_command_for_method,
    self_update_command_from_context, self_update_package_dir_candidates,
    self_update_path_is_writable, self_update_prepare_actions, self_update_unavailable_instruction,
    update_instruction, GlobalPackageRootOutputTransform, GlobalPackageRootsPlan, InstallMethod,
    NpmGlobalInstall, PackageCommand, PackageCommandAction, PackageCommandOptions,
    PackageCommandValidationError, SelfUpdateCommand, SelfUpdateCommandStep, SelfUpdatePlan,
    SelfUpdatePrepareAction, SelfUpdatePrepareRunner, SystemSelfUpdatePrepareRunner, UpdateTarget,
};
pub use configured::{
    configured_package_sources, configured_update_sources, list_configured_packages_from_settings,
    npm_command_from_settings,
};
pub use executor::{CommandExecution, PackageCommandExecutor, PackageCommandRunner};
pub use lifecycle::{
    install, install_and_persist, remove, remove_and_persist, update, update_configured_sources,
    update_from_settings, PackageLifecycleError,
};
pub use operations::{plan_install, plan_remove, progress_events_for_plan, progress_verb};
pub use resolve_runtime::{
    resolve_package_sources, MissingSourceHandler, StaticMissingSourceHandler,
};
pub use resolver::{resolve_auto_discovered_resources, resolve_source, resource_precedence_rank};
pub use settings::{
    add_source_to_settings, normalize_package_source_for_settings, package_source_string,
    package_sources_match, remove_source_from_settings,
};
pub use source::{
    git_install_path, installed_path_for_source, managed_npm_install_path, parse_source,
    source_identity,
};
pub use types::{
    ConfiguredPackage, LocalSource, MissingSourceAction, NpmCommandConfig, NpmSource,
    PackageCommandStep, PackageFilter, PackageKind, PackageOperationPlan, PackageUpdate,
    ParsedSource, PathMetadata, ProgressAction, ProgressEvent, ProgressEventKind, ResolvedPaths,
    ResolvedResource, SourceOrigin, SourceScope,
};
pub use update_checker::CommandUpdateChecker;
pub use updates::{check_configured_updates, plan_update, ConfiguredUpdateSource, UpdateCheck};

pub struct LocalPackageManager {
    user_sources: Vec<String>,
    project_sources: Vec<String>,
}

impl LocalPackageManager {
    pub fn new(user_sources: Vec<String>, project_sources: Vec<String>) -> Self {
        Self {
            user_sources,
            project_sources,
        }
    }

    pub fn resolve(&self) -> ResolvedPaths {
        let mut resolved = ResolvedPaths::default();
        for source in &self.user_sources {
            merge_paths(
                &mut resolved,
                resolve_source(source, SourceScope::User, SourceOrigin::TopLevel, None),
            );
        }
        for source in &self.project_sources {
            merge_paths(
                &mut resolved,
                resolve_source(source, SourceScope::Project, SourceOrigin::TopLevel, None),
            );
        }
        sort_resolved_paths(&mut resolved);
        resolved
    }

    pub fn resolve_extension_sources(
        sources: &[String],
        local: bool,
        temporary: bool,
    ) -> ResolvedPaths {
        let scope = if temporary {
            SourceScope::Temporary
        } else if local {
            SourceScope::Project
        } else {
            SourceScope::User
        };
        let mut resolved = ResolvedPaths::default();
        for source in sources {
            let local_path = local_source_path(source);
            if let Some(single_file) = local_source_path(source).filter(|path| path.is_file()) {
                resolved.extensions.push(ResolvedResource {
                    path: display_path(&single_file),
                    enabled: true,
                    metadata: PathMetadata {
                        source: source.clone(),
                        scope,
                        origin: SourceOrigin::Package,
                        base_dir: single_file.parent().map(display_path),
                    },
                });
                continue;
            }
            if let Some(dir) =
                local_path.filter(|path| local_directory_should_fallback_to_extension(path, None))
            {
                resolved.extensions.push(ResolvedResource {
                    path: display_path(&dir),
                    enabled: true,
                    metadata: PathMetadata {
                        source: source.clone(),
                        scope,
                        origin: SourceOrigin::Package,
                        base_dir: Some(display_path(&dir)),
                    },
                });
                continue;
            }
            merge_paths(
                &mut resolved,
                resolve_source(source, scope, SourceOrigin::Package, None),
            );
        }
        sort_resolved_paths(&mut resolved);
        resolved
    }

    pub fn resolve_extension_sources_with_runner<R: PackageCommandRunner>(
        runner: &R,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        sources: &[String],
        local: bool,
        temporary: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<ResolvedPaths, PackageLifecycleError> {
        let scope = if temporary {
            SourceScope::Temporary
        } else if local {
            SourceScope::Project
        } else {
            SourceScope::User
        };
        let package_sources = sources
            .iter()
            .map(|source| (source.clone(), scope, None))
            .collect::<Vec<_>>();
        let mut handler = StaticMissingSourceHandler::new(MissingSourceAction::Install);
        resolve_package_sources(
            runner,
            &mut handler,
            agent_dir,
            cwd,
            &package_sources,
            npm_command,
            on_progress,
        )
    }

    pub fn resolve_from_settings<S: SettingsStorage, R: PackageCommandRunner + ?Sized>(
        runner: &R,
        settings: &SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        user_agents_dir: Option<&std::path::Path>,
        npm_command: Option<NpmCommandConfig>,
        mut on_progress: impl FnMut(ProgressEvent),
    ) -> Result<ResolvedPaths, PackageLifecycleError> {
        let agent_dir = agent_dir.as_ref();
        let cwd = cwd.as_ref();
        let project_base_dir = cwd.join(CONFIG_DIR_NAME);
        let global_settings = settings.global_settings();
        let project_settings = settings.project_settings();
        let package_sources = configured_package_sources(settings, agent_dir, cwd);
        let npm_command = match npm_command {
            Some(command) => Some(command),
            None => {
                npm_command_from_settings(settings).map_err(PackageLifecycleError::CommandFailed)?
            }
        };
        let mut handler = StaticMissingSourceHandler::new(MissingSourceAction::Install);
        let mut resolved = resolve_package_sources(
            runner,
            &mut handler,
            agent_dir,
            cwd,
            &package_sources,
            npm_command,
            &mut on_progress,
        )?;

        merge_paths(
            &mut resolved,
            resolve_explicit_top_level_settings(
                project_settings.extensions.clone().unwrap_or_default(),
                project_settings.skills.clone().unwrap_or_default(),
                project_settings.prompts.clone().unwrap_or_default(),
                project_settings.themes.clone().unwrap_or_default(),
                SourceScope::Project,
                &project_base_dir,
            ),
        );
        merge_paths(
            &mut resolved,
            resolve_explicit_top_level_settings(
                global_settings.extensions.clone().unwrap_or_default(),
                global_settings.skills.clone().unwrap_or_default(),
                global_settings.prompts.clone().unwrap_or_default(),
                global_settings.themes.clone().unwrap_or_default(),
                SourceScope::User,
                agent_dir,
            ),
        );
        merge_paths(
            &mut resolved,
            resolve_auto_discovered_resources(
                agent_dir,
                &project_base_dir,
                cwd,
                user_agents_dir,
                Some(&PackageFilter {
                    extensions: Some(global_settings.extensions.unwrap_or_default()),
                    skills: Some(global_settings.skills.unwrap_or_default()),
                    prompts: Some(global_settings.prompts.unwrap_or_default()),
                    themes: Some(global_settings.themes.unwrap_or_default()),
                }),
                Some(&PackageFilter {
                    extensions: Some(project_settings.extensions.unwrap_or_default()),
                    skills: Some(project_settings.skills.unwrap_or_default()),
                    prompts: Some(project_settings.prompts.unwrap_or_default()),
                    themes: Some(project_settings.themes.unwrap_or_default()),
                }),
            ),
        );
        sort_resolved_paths(&mut resolved);
        Ok(resolved)
    }

    pub fn list_configured_packages(&self) -> Vec<ConfiguredPackage> {
        self.user_sources
            .iter()
            .map(|source| ConfiguredPackage {
                source: source.clone(),
                scope: SourceScope::User,
                filtered: false,
                installed_path: local_source_path(source).map(display_path),
            })
            .chain(self.project_sources.iter().map(|source| ConfiguredPackage {
                source: source.clone(),
                scope: SourceScope::Project,
                filtered: false,
                installed_path: local_source_path(source).map(display_path),
            }))
            .collect()
    }

    pub fn list_configured_packages_from_settings<S: crate::settings_manager::SettingsStorage>(
        settings: &crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        npm_command: Option<NpmCommandConfig>,
    ) -> Vec<ConfiguredPackage> {
        configured::list_configured_packages_from_settings(settings, agent_dir, cwd, npm_command)
    }

    pub fn get_installed_path(
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        scope: SourceScope,
    ) -> Option<String> {
        Self::get_installed_path_with_npm_command(agent_dir, cwd, source, scope, None)
    }

    pub fn get_installed_path_with_npm_command(
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        scope: SourceScope,
        npm_command: Option<NpmCommandConfig>,
    ) -> Option<String> {
        let npm_command = npm_command.unwrap_or_default();
        let cwd = cwd.as_ref();
        source::installed_path_for_source_with_npm_fallback(agent_dir, cwd, source, scope, |npm| {
            legacy_global_npm_package_path(npm, cwd, &npm_command)
        })
        .map(display_path)
    }

    pub fn add_source_to_settings<S: crate::settings_manager::SettingsStorage>(
        settings: &mut crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
    ) -> bool {
        add_source_to_settings(settings, agent_dir, cwd, source, local)
    }

    pub fn remove_source_from_settings<S: crate::settings_manager::SettingsStorage>(
        settings: &mut crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
    ) -> bool {
        remove_source_from_settings(settings, agent_dir, cwd, source, local)
    }

    pub fn plan_install(
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        scope: SourceScope,
        npm_command: Option<NpmCommandConfig>,
    ) -> PackageOperationPlan {
        plan_install(agent_dir, cwd, source, scope, npm_command)
    }

    pub fn plan_remove(
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        scope: SourceScope,
        npm_command: Option<NpmCommandConfig>,
    ) -> PackageOperationPlan {
        plan_remove(agent_dir, cwd, source, scope, npm_command)
    }

    pub fn install<R: PackageCommandRunner>(
        runner: &R,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        install(
            runner,
            agent_dir,
            cwd,
            source,
            local,
            npm_command,
            on_progress,
        )
    }

    pub fn remove<R: PackageCommandRunner>(
        runner: &R,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        remove(
            runner,
            agent_dir,
            cwd,
            source,
            local,
            npm_command,
            on_progress,
        )
    }

    pub fn update<R: PackageCommandRunner>(
        runner: &R,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        update(
            runner,
            agent_dir,
            cwd,
            source,
            local,
            npm_command,
            on_progress,
        )
    }

    pub fn update_configured_sources<R: PackageCommandRunner>(
        runner: &R,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        sources: &[ConfiguredUpdateSource],
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        update_configured_sources(runner, agent_dir, cwd, sources, npm_command, on_progress)
    }

    pub fn update_from_settings<
        S: crate::settings_manager::SettingsStorage,
        R: PackageCommandRunner,
    >(
        runner: &R,
        settings: &crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source_filter: Option<&str>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        update_from_settings(runner, settings, agent_dir, cwd, source_filter, on_progress)
    }

    pub fn check_available_updates_from_settings<
        S: crate::settings_manager::SettingsStorage,
        C: UpdateCheck,
    >(
        checker: &C,
        settings: &crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
    ) -> Result<Vec<PackageUpdate>, PackageLifecycleError> {
        if lifecycle::is_offline_mode_enabled() {
            return Ok(Vec::new());
        }
        let agent_dir = agent_dir.as_ref();
        let cwd = cwd.as_ref();
        let sources = configured_package_sources(settings, agent_dir, cwd)
            .into_iter()
            .map(|(source, scope, _)| ConfiguredUpdateSource { source, scope })
            .collect::<Vec<_>>();
        let npm_command =
            npm_command_from_settings(settings).map_err(PackageLifecycleError::CommandFailed)?;
        let npm_command = npm_command.unwrap_or_default();
        Ok(updates::check_configured_updates_with_npm_fallback(
            checker,
            agent_dir,
            cwd,
            &sources,
            |npm| legacy_global_npm_package_path(npm, cwd, &npm_command),
        ))
    }

    pub fn install_and_persist<
        S: crate::settings_manager::SettingsStorage,
        R: PackageCommandRunner,
    >(
        runner: &R,
        settings: &mut crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<(), PackageLifecycleError> {
        install_and_persist(
            runner,
            settings,
            agent_dir,
            cwd,
            source,
            local,
            npm_command,
            on_progress,
        )
    }

    pub fn remove_and_persist<
        S: crate::settings_manager::SettingsStorage,
        R: PackageCommandRunner,
    >(
        runner: &R,
        settings: &mut crate::settings_manager::SettingsManager<S>,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        local: bool,
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<bool, PackageLifecycleError> {
        remove_and_persist(
            runner,
            settings,
            agent_dir,
            cwd,
            source,
            local,
            npm_command,
            on_progress,
        )
    }

    pub fn plan_update(
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        source: &str,
        scope: SourceScope,
        npm_command: Option<NpmCommandConfig>,
    ) -> PackageOperationPlan {
        plan_update(agent_dir, cwd, source, scope, npm_command)
    }

    pub fn resolve_package_sources<R: PackageCommandRunner + ?Sized, H: MissingSourceHandler>(
        runner: &R,
        handler: &mut H,
        agent_dir: impl AsRef<std::path::Path>,
        cwd: impl AsRef<std::path::Path>,
        sources: &[(String, SourceScope, Option<PackageFilter>)],
        npm_command: Option<NpmCommandConfig>,
        on_progress: impl FnMut(ProgressEvent),
    ) -> Result<ResolvedPaths, PackageLifecycleError> {
        resolve_package_sources(
            runner,
            handler,
            agent_dir,
            cwd,
            sources,
            npm_command,
            on_progress,
        )
    }
}

fn legacy_global_npm_package_path(
    source: &NpmSource,
    cwd: &Path,
    npm_command: &NpmCommandConfig,
) -> Option<PathBuf> {
    if operations::package_manager_name(npm_command) == "pnpm" {
        if let Some(path) = pnpm_global_package_path(&source.name, cwd, npm_command) {
            return Some(path);
        }
    }
    Some(global_npm_root(cwd, npm_command)?.join(&source.name))
}

fn pnpm_global_package_path(
    package_name: &str,
    cwd: &Path,
    npm_command: &NpmCommandConfig,
) -> Option<PathBuf> {
    let output =
        run_npm_command_capture(cwd, npm_command, &["list", "-g", "--depth", "0", "--json"])?;
    let entries = serde_json::from_str::<serde_json::Value>(&output).ok()?;
    entries.as_array()?.iter().find_map(|entry| {
        entry
            .get("dependencies")?
            .get(package_name)?
            .get("path")?
            .as_str()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    })
}

fn global_npm_root(cwd: &Path, npm_command: &NpmCommandConfig) -> Option<PathBuf> {
    if operations::package_manager_name(npm_command) == "bun" {
        let bin_dir = run_npm_command_capture(cwd, npm_command, &["pm", "bin", "-g"])?;
        return Path::new(bin_dir.trim())
            .parent()
            .map(|path| path.join("install").join("global").join("node_modules"));
    }
    run_npm_command_capture(cwd, npm_command, &["root", "-g"])
        .map(|output| PathBuf::from(output.trim()))
}

fn run_npm_command_capture(
    cwd: &Path,
    npm_command: &NpmCommandConfig,
    args: &[&str],
) -> Option<String> {
    let mut full_args = npm_command.args.clone();
    full_args.extend(args.iter().map(|arg| arg.to_string()));
    let result = exec_command(
        &npm_command.command,
        &full_args,
        &display_path(cwd),
        Some(ExecOptions {
            cwd: Some(display_path(cwd)),
            timeout_ms: None,
            ..ExecOptions::default()
        }),
    )
    .ok()?;
    (result.code == 0).then_some(result.stdout)
}

fn resolve_explicit_top_level_settings(
    extensions: Vec<String>,
    skills: Vec<String>,
    prompts: Vec<String>,
    themes: Vec<String>,
    scope: SourceScope,
    base_dir: &Path,
) -> ResolvedPaths {
    let mut resolved = ResolvedPaths::default();
    merge_paths(
        &mut resolved,
        resolve_explicit_entries_for_type(extensions, scope, base_dir, ResourceTypeKind::Extension),
    );
    merge_paths(
        &mut resolved,
        resolve_explicit_entries_for_type(skills, scope, base_dir, ResourceTypeKind::Skill),
    );
    merge_paths(
        &mut resolved,
        resolve_explicit_entries_for_type(prompts, scope, base_dir, ResourceTypeKind::Prompt),
    );
    merge_paths(
        &mut resolved,
        resolve_explicit_entries_for_type(themes, scope, base_dir, ResourceTypeKind::Theme),
    );
    resolved
}

#[derive(Debug, Clone, Copy)]
enum ResourceTypeKind {
    Extension,
    Skill,
    Prompt,
    Theme,
}

fn resolve_explicit_entries_for_type(
    entries: Vec<String>,
    scope: SourceScope,
    base_dir: &Path,
    kind: ResourceTypeKind,
) -> ResolvedPaths {
    let mut resolved = ResolvedPaths::default();
    for entry in entries {
        if entry.starts_with('!') || entry.starts_with('+') || entry.starts_with('-') {
            continue;
        }
        let path = crate::utils::paths::resolve_path(&entry, base_dir, None);
        if !path.exists() {
            continue;
        }
        let next = match kind {
            ResourceTypeKind::Extension => resolver::resolve_source_at_path(
                "local",
                &path,
                scope,
                SourceOrigin::TopLevel,
                Some(&PackageFilter {
                    extensions: Some(vec![entry]),
                    ..PackageFilter::default()
                }),
            ),
            ResourceTypeKind::Skill => resolver::resolve_source_at_path(
                "local",
                &path,
                scope,
                SourceOrigin::TopLevel,
                Some(&PackageFilter {
                    skills: Some(vec![entry]),
                    ..PackageFilter::default()
                }),
            ),
            ResourceTypeKind::Prompt => resolver::resolve_source_at_path(
                "local",
                &path,
                scope,
                SourceOrigin::TopLevel,
                Some(&PackageFilter {
                    prompts: Some(vec![entry]),
                    ..PackageFilter::default()
                }),
            ),
            ResourceTypeKind::Theme => resolver::resolve_source_at_path(
                "local",
                &path,
                scope,
                SourceOrigin::TopLevel,
                Some(&PackageFilter {
                    themes: Some(vec![entry]),
                    ..PackageFilter::default()
                }),
            ),
        };
        merge_paths(&mut resolved, next);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_manager::SettingsStorage;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakeUpdateChecker {
        npm: HashMap<String, String>,
        git: HashMap<String, String>,
    }

    impl UpdateCheck for FakeUpdateChecker {
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

    #[derive(Default)]
    struct InstallingRunner {
        calls: std::cell::RefCell<Vec<PackageCommandStep>>,
    }

    impl PackageCommandRunner for InstallingRunner {
        fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
            self.calls.borrow_mut().push(step.clone());
            if step.command == "ensure_npm_project" {
                let root = PathBuf::from(
                    step.args
                        .first()
                        .expect("ensure_npm_project should have root"),
                );
                let prompt = root
                    .join("node_modules")
                    .join("pkg")
                    .join("prompts")
                    .join("review.md");
                fs::create_dir_all(prompt.parent().expect("prompt parent should exist"))
                    .expect("npm package prompts dir should be created");
                fs::write(prompt, "").expect("prompt should be written");
            }
            Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            })
        }
    }

    #[test]
    fn resolves_package_manifest_resources() {
        let dir = temp_dir();
        fs::write(
            dir.join("package.json"),
            r#"{"pi":{"extensions":["src/index.ts"],"skills":["skills/demo/SKILL.md"],"prompts":["prompts/review.md"],"themes":["themes/dark.json"]}}"#,
        )
        .expect("manifest should be written");
        write_file(&dir.join("src/index.ts"));
        write_file(&dir.join("skills/demo/SKILL.md"));
        write_file(&dir.join("prompts/review.md"));
        write_file(&dir.join("themes/dark.json"));

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::Package,
            None,
        );
        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.skills.len(), 1);
        assert_eq!(resolved.prompts.len(), 1);
        assert_eq!(resolved.themes.len(), 1);
    }

    #[test]
    fn resolve_extension_sources_with_runner_installs_missing_remote_packages_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let runner = InstallingRunner::default();
        let mut events = Vec::new();

        let resolved = LocalPackageManager::resolve_extension_sources_with_runner(
            &runner,
            &agent_dir,
            &cwd,
            &["npm:pkg".to_string()],
            false,
            false,
            None,
            |event| events.push(event),
        )
        .expect("missing npm package should install and resolve");

        assert_eq!(resolved.prompts.len(), 1);
        assert!(resolved.prompts[0].path.ends_with("pkg/prompts/review.md"));
        assert_eq!(resolved.prompts[0].metadata.source, "npm:pkg");
        assert_eq!(resolved.prompts[0].metadata.scope, SourceScope::User);
        assert!(runner
            .calls
            .borrow()
            .iter()
            .any(|step| step.command == "ensure_npm_project"));
        assert!(events
            .iter()
            .any(|event| event.action == ProgressAction::Install));
    }

    #[test]
    fn resolve_from_settings_combines_packages_top_level_and_auto_resources_like_pi() {
        let root = temp_dir();
        let agent_dir = root.join("agent");
        let cwd = root.join("project");
        let project_base_dir = cwd.join(crate::settings_manager::CONFIG_DIR_NAME);
        fs::create_dir_all(&agent_dir).expect("agent dir should exist");
        fs::create_dir_all(&project_base_dir).expect("project base dir should exist");

        let global_pkg = agent_dir.join("npm").join("node_modules").join("pkg");
        let project_pkg = project_base_dir
            .join("npm")
            .join("node_modules")
            .join("pkg");
        write_file(&global_pkg.join("prompts").join("global-package.md"));
        write_file(&project_pkg.join("prompts").join("project-package.md"));
        write_file(&project_base_dir.join("project-explicit.md"));
        write_file(&project_base_dir.join("prompts").join("project-auto.md"));
        write_file(&agent_dir.join("skills").join("auto").join("SKILL.md"));

        let mut storage = crate::settings_manager::InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                r#"{"packages":["npm:pkg"]}"#.to_string(),
            )
            .expect("global settings should write");
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":["npm:pkg"],"prompts":["project-explicit.md"]}"#.to_string(),
            )
            .expect("project settings should write");
        let settings = crate::settings_manager::SettingsManager::from_storage(storage);
        let runner = InstallingRunner::default();

        let resolved = LocalPackageManager::resolve_from_settings(
            &runner,
            &settings,
            &agent_dir,
            &cwd,
            None,
            None,
            |_| {},
        )
        .expect("settings resources should resolve");

        let prompt_names = resolved
            .prompts
            .iter()
            .map(|resource| {
                (
                    Path::new(&resource.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.metadata.source.clone(),
                    resource.metadata.scope,
                    resource.metadata.origin,
                )
            })
            .collect::<Vec<_>>();
        assert!(prompt_names.contains(&(
            "project-package.md".to_string(),
            "npm:pkg".to_string(),
            SourceScope::Project,
            SourceOrigin::Package,
        )));
        assert!(prompt_names.contains(&(
            "project-explicit.md".to_string(),
            "local".to_string(),
            SourceScope::Project,
            SourceOrigin::TopLevel,
        )));
        assert!(!prompt_names
            .iter()
            .any(|(name, _, _, _)| name == "global-package.md"));

        assert_eq!(resolved.skills.len(), 1);
        assert!(resolved.skills[0].path.ends_with("auto/SKILL.md"));
        assert_eq!(resolved.skills[0].metadata.source, "auto");
        assert_eq!(resolved.skills[0].metadata.scope, SourceScope::User);
    }

    #[test]
    fn package_manifest_globs_and_overrides_match_pi() {
        let dir = temp_dir();
        fs::write(
            dir.join("package.json"),
            r#"{"pi":{"prompts":["prompts/*.md","!prompts/draft.md","-prompts/remove.md"]}}"#,
        )
        .expect("manifest should be written");
        write_file(&dir.join("prompts").join("keep.md"));
        write_file(&dir.join("prompts").join("draft.md"));
        write_file(&dir.join("prompts").join("remove.md"));

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::Package,
            None,
        );
        let mut prompts = resolved
            .prompts
            .iter()
            .map(|resource| {
                (
                    Path::new(&resource.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.enabled,
                )
            })
            .collect::<Vec<_>>();
        prompts.sort_by(|left, right| left.0.cmp(&right.0));

        assert_eq!(prompts, vec![("keep.md".to_string(), true)]);
    }

    #[test]
    fn resolves_top_level_extension_and_skill_entries() {
        let dir = temp_dir();
        write_file(&dir.join("plugin/index.ts"));
        write_file(&dir.join("skills/build/SKILL.md"));
        write_file(&dir.join("prompts/fix.md"));
        write_file(&dir.join("themes/work.json"));

        let manager = LocalPackageManager::new(vec![display_path(&dir)], Vec::new());
        let resolved = manager.resolve();
        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.skills.len(), 1);
        assert_eq!(resolved.prompts.len(), 2);
        assert_eq!(resolved.themes.len(), 1);
    }

    #[test]
    fn resolve_extension_sources_treats_local_single_file_as_extension_like_pi() {
        let dir = temp_dir();
        let extension_file = dir.join("extension.md");
        write_file(&extension_file);

        let resolved = LocalPackageManager::resolve_extension_sources(
            &[display_path(&extension_file)],
            true,
            false,
        );

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.extensions[0].path, display_path(&extension_file));
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert!(resolved.prompts.is_empty());
    }

    #[test]
    fn resolve_extension_sources_treats_local_directory_without_resources_as_extension_like_pi() {
        let dir = temp_dir();

        let resolved =
            LocalPackageManager::resolve_extension_sources(&[display_path(&dir)], true, false);

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.extensions[0].path, display_path(&dir));
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::Project);
        assert!(resolved.prompts.is_empty());
    }

    #[test]
    fn resolve_extension_sources_uses_package_metadata_for_local_directory_resources_like_pi() {
        let dir = temp_dir();
        let extension_file = dir.join("extensions").join("index.ts");
        write_file(&extension_file);

        let resolved =
            LocalPackageManager::resolve_extension_sources(&[display_path(&dir)], true, false);

        assert_eq!(resolved.extensions.len(), 1);
        assert_eq!(resolved.extensions[0].path, display_path(&extension_file));
        assert_eq!(resolved.extensions[0].metadata.source, display_path(&dir));
        assert_eq!(
            resolved.extensions[0].metadata.origin,
            SourceOrigin::Package
        );
        assert_eq!(resolved.extensions[0].metadata.scope, SourceScope::Project);
        assert_eq!(
            resolved.extensions[0].metadata.base_dir.as_deref(),
            Some(display_path(&dir).as_str())
        );
    }

    #[test]
    fn project_sources_sort_before_user_sources() {
        let user = PathMetadata {
            source: "local".to_string(),
            scope: SourceScope::User,
            origin: SourceOrigin::TopLevel,
            base_dir: None,
        };
        let project = PathMetadata {
            source: "local".to_string(),
            scope: SourceScope::Project,
            origin: SourceOrigin::TopLevel,
            base_dir: None,
        };
        assert!(resource_precedence_rank(&project) < resource_precedence_rank(&user));
    }

    #[test]
    fn reports_installed_paths_for_npm_git_and_local_sources() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let npm_path = agent_dir
            .join("npm")
            .join("node_modules")
            .join("@scope")
            .join("pkg");
        fs::create_dir_all(&npm_path).expect("npm path should be created");
        let git_path = agent_dir
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        fs::create_dir_all(&git_path).expect("git path should be created");
        let local_path = cwd.join("local-plugin");
        fs::create_dir_all(&local_path).expect("local path should be created");

        assert_eq!(
            LocalPackageManager::get_installed_path(
                &agent_dir,
                &cwd,
                "npm:@scope/pkg",
                SourceScope::User
            )
            .as_deref(),
            Some(display_path(&npm_path).as_str())
        );
        assert_eq!(
            LocalPackageManager::get_installed_path(
                &agent_dir,
                &cwd,
                "https://github.com/user/repo.git",
                SourceScope::User
            )
            .as_deref(),
            Some(display_path(&git_path).as_str())
        );
        assert_eq!(
            LocalPackageManager::get_installed_path(
                &agent_dir,
                &cwd,
                "../local-plugin",
                SourceScope::Project
            )
            .as_deref(),
            Some(display_path(&local_path).as_str())
        );
    }

    #[test]
    fn installed_path_uses_legacy_global_npm_root_for_user_scope_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let global_root = temp_dir().join("global-node-modules");
        let package_path = global_root.join("pkg");
        fs::create_dir_all(&package_path).expect("legacy package path should be created");
        let npm = fake_npm_command(&global_root);

        let installed = LocalPackageManager::get_installed_path_with_npm_command(
            &agent_dir,
            &cwd,
            "npm:pkg",
            SourceScope::User,
            Some(npm),
        );

        assert_eq!(
            installed.as_deref(),
            Some(display_path(&package_path).as_str())
        );
    }

    #[test]
    fn installed_path_uses_wrapped_pnpm_global_list_path_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let pnpm_root = temp_dir().join("pnpm").join("global").join("v11");
        let package_path = pnpm_root
            .join("20-hash")
            .join("node_modules")
            .join("pnpm-pkg");
        fs::create_dir_all(&package_path).expect("legacy pnpm package path should be created");
        let command = fake_pnpm_list_command(&pnpm_root, &package_path);

        let installed = LocalPackageManager::get_installed_path_with_npm_command(
            &agent_dir,
            &cwd,
            "npm:pnpm-pkg",
            SourceScope::User,
            Some(command),
        );

        assert_eq!(
            installed.as_deref(),
            Some(display_path(&package_path).as_str())
        );
    }

    #[test]
    fn installed_path_ignores_malformed_pnpm_global_list_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let command = fake_malformed_pnpm_list_command();

        let installed = LocalPackageManager::get_installed_path_with_npm_command(
            &agent_dir,
            &cwd,
            "npm:pnpm-pkg",
            SourceScope::User,
            Some(command),
        );

        assert!(installed.is_none());
    }

    #[test]
    fn list_configured_packages_from_settings_preserves_filtered_entries_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let user_package_path = agent_dir.join("npm").join("node_modules").join("pkg");
        let project_package_path = cwd
            .join(crate::settings_manager::CONFIG_DIR_NAME)
            .join("git")
            .join("github.com")
            .join("owner")
            .join("repo");
        fs::create_dir_all(&user_package_path).expect("user package should exist");
        fs::create_dir_all(&project_package_path).expect("project package should exist");

        let mut storage = crate::settings_manager::InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                r#"{"packages":[{"source":"npm:pkg","prompts":["review.md"]},"./missing"]}"#
                    .to_string(),
            )
            .expect("global settings should write");
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":[{"source":"https://github.com/owner/repo.git","extensions":["src/index.ts"]}]}"#
                    .to_string(),
            )
            .expect("project settings should write");
        let settings = crate::settings_manager::SettingsManager::from_storage(storage);

        let packages = LocalPackageManager::list_configured_packages_from_settings(
            &settings, &agent_dir, &cwd, None,
        );

        assert_eq!(
            packages,
            vec![
                ConfiguredPackage {
                    source: "npm:pkg".to_string(),
                    scope: SourceScope::User,
                    filtered: true,
                    installed_path: Some(display_path(&user_package_path)),
                },
                ConfiguredPackage {
                    source: "./missing".to_string(),
                    scope: SourceScope::User,
                    filtered: false,
                    installed_path: None,
                },
                ConfiguredPackage {
                    source: "https://github.com/owner/repo.git".to_string(),
                    scope: SourceScope::Project,
                    filtered: true,
                    installed_path: Some(display_path(&project_package_path)),
                },
            ]
        );
    }

    #[test]
    fn check_available_updates_from_settings_dedupes_and_skips_local_or_pinned_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let npm_path = cwd
            .join(crate::settings_manager::CONFIG_DIR_NAME)
            .join("npm")
            .join("node_modules")
            .join("pkg");
        fs::create_dir_all(&npm_path).expect("npm path should exist");
        fs::write(npm_path.join("package.json"), r#"{"version":"1.0.0"}"#)
            .expect("package json should be written");

        let git_path = agent_dir
            .join("git")
            .join("github.com")
            .join("owner")
            .join("repo");
        fs::create_dir_all(git_path.join(".git").join("refs").join("heads"))
            .expect("git path should exist");
        fs::write(git_path.join(".git").join("HEAD"), "ref: refs/heads/main\n")
            .expect("head should be written");
        fs::write(
            git_path
                .join(".git")
                .join("refs")
                .join("heads")
                .join("main"),
            "local\n",
        )
        .expect("branch head should be written");

        let mut storage = crate::settings_manager::InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                r#"{"packages":["npm:pkg@0.9.0","git:https://github.com/owner/repo","./local"]}"#
                    .to_string(),
            )
            .expect("global settings should write");
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":["npm:pkg","git:https://github.com/owner/pinned@main"]}"#
                    .to_string(),
            )
            .expect("project settings should write");
        let settings = crate::settings_manager::SettingsManager::from_storage(storage);

        let mut checker = FakeUpdateChecker::default();
        checker.npm.insert("pkg".to_string(), "2.0.0".to_string());
        checker
            .git
            .insert(git_path.to_string_lossy().to_string(), "remote".to_string());

        let updates = LocalPackageManager::check_available_updates_from_settings(
            &checker, &settings, &agent_dir, &cwd,
        )
        .expect("update check should succeed");

        assert_eq!(
            updates,
            vec![
                PackageUpdate {
                    source: "npm:pkg".to_string(),
                    display_name: "pkg".to_string(),
                    kind: PackageKind::Npm,
                    scope: SourceScope::Project,
                },
                PackageUpdate {
                    source: "git:https://github.com/owner/repo".to_string(),
                    display_name: "github.com/owner/repo".to_string(),
                    kind: PackageKind::Git,
                    scope: SourceScope::User,
                },
            ]
        );
    }

    #[test]
    fn check_available_updates_from_settings_uses_legacy_global_npm_path_like_pi() {
        let agent_dir = temp_dir();
        let cwd = temp_dir();
        let global_root = temp_dir().join("global-node-modules");
        let npm_path = global_root.join("pkg");
        fs::create_dir_all(&npm_path).expect("legacy npm path should exist");
        fs::write(npm_path.join("package.json"), r#"{"version":"1.0.0"}"#)
            .expect("package json should be written");

        let npm_command = fake_npm_command(&global_root);
        let settings = crate::settings_manager::SettingsManager::<
            crate::settings_manager::InMemorySettingsStorage,
        >::in_memory(json!({
            "packages": ["npm:pkg"],
            "npmCommand": [npm_command.command]
        }));

        let mut checker = FakeUpdateChecker::default();
        checker.npm.insert("pkg".to_string(), "2.0.0".to_string());

        let updates = LocalPackageManager::check_available_updates_from_settings(
            &checker, &settings, &agent_dir, &cwd,
        )
        .expect("update check should succeed");

        assert_eq!(
            updates,
            vec![PackageUpdate {
                source: "npm:pkg".to_string(),
                display_name: "pkg".to_string(),
                kind: PackageKind::Npm,
                scope: SourceScope::User,
            }]
        );
    }

    #[test]
    fn check_available_updates_from_settings_rejects_empty_npm_command_like_pi() {
        let settings = crate::settings_manager::SettingsManager::<
            crate::settings_manager::InMemorySettingsStorage,
        >::in_memory(json!({
            "packages": ["npm:pkg"],
            "npmCommand": [""]
        }));
        let checker = FakeUpdateChecker::default();

        let error = LocalPackageManager::check_available_updates_from_settings(
            &checker, &settings, "/agent", "/work",
        )
        .expect_err("empty npmCommand should fail fast");

        assert_eq!(
            error,
            PackageLifecycleError::CommandFailed(
                "Invalid npmCommand: first array entry must be a non-empty command".to_string()
            )
        );
    }

    #[test]
    fn auto_discovered_resources_dedupe_symlinked_user_skill_paths_like_pi() {
        let root = temp_dir();
        let agent_dir = root.join("agent");
        let project_base_dir = root.join(".pi");
        let user_agents_dir = root.join(".agents");
        let agents_skills_dir = user_agents_dir.join("skills");
        fs::create_dir_all(&agents_skills_dir).expect("agents skills dir should exist");
        fs::create_dir_all(&agent_dir).expect("agent dir should exist");
        fs::create_dir_all(&project_base_dir).expect("project base dir should exist");
        symlink_dir(&agents_skills_dir, &agent_dir.join("skills"));

        let skill_path = agents_skills_dir.join("foo").join("SKILL.md");
        write_file(&skill_path);

        let resolved = resolve_auto_discovered_resources(
            &agent_dir,
            &project_base_dir,
            &root,
            Some(user_agents_dir.as_path()),
            None,
            None,
        );
        let foo_skills = resolved
            .skills
            .iter()
            .filter(|resource| resource.path.ends_with("foo/SKILL.md"))
            .collect::<Vec<_>>();

        assert_eq!(foo_skills.len(), 1);
        assert_eq!(
            fs::canonicalize(&foo_skills[0].path).expect("resolved skill should exist"),
            fs::canonicalize(&skill_path).expect("source skill should exist")
        );
        assert_eq!(foo_skills[0].metadata.scope, SourceScope::User);
        assert_eq!(foo_skills[0].metadata.source, "auto");
    }

    #[test]
    fn auto_discovers_project_agents_skills_up_to_filesystem_root_without_git_like_pi() {
        let root = temp_dir();
        let agent_dir = temp_dir();
        let project_base_dir = root.join(".pi");
        let cwd = root.join("non-repo").join("a").join("b");
        fs::create_dir_all(&cwd).expect("cwd should exist");

        let root_skill = root
            .join("non-repo")
            .join(".agents")
            .join("skills")
            .join("root")
            .join("SKILL.md");
        let middle_skill = root
            .join("non-repo")
            .join("a")
            .join(".agents")
            .join("skills")
            .join("middle")
            .join("SKILL.md");
        write_file(&root_skill);
        write_file(&middle_skill);

        let resolved = resolve_auto_discovered_resources(
            &agent_dir,
            &project_base_dir,
            &cwd,
            None,
            None,
            None,
        );

        assert!(resolved
            .skills
            .iter()
            .any(|resource| same_existing_path(&resource.path, &root_skill) && resource.enabled));
        assert!(resolved
            .skills
            .iter()
            .any(|resource| same_existing_path(&resource.path, &middle_skill) && resource.enabled));
    }

    #[test]
    fn auto_discovers_project_agents_skills_only_to_git_root_like_pi() {
        let root = temp_dir();
        let agent_dir = temp_dir();
        let project_base_dir = root.join("repo").join(".pi");
        let repo_root = root.join("repo");
        let cwd = repo_root.join("packages").join("feature");
        fs::create_dir_all(cwd.as_path()).expect("cwd should exist");
        fs::create_dir_all(repo_root.join(".git")).expect("git dir should exist");

        let above_repo_skill = root
            .join(".agents")
            .join("skills")
            .join("above-repo")
            .join("SKILL.md");
        let repo_skill = repo_root
            .join(".agents")
            .join("skills")
            .join("repo-root")
            .join("SKILL.md");
        let nested_skill = repo_root
            .join("packages")
            .join(".agents")
            .join("skills")
            .join("nested")
            .join("SKILL.md");
        write_file(&above_repo_skill);
        write_file(&repo_skill);
        write_file(&nested_skill);

        let resolved = resolve_auto_discovered_resources(
            &agent_dir,
            &project_base_dir,
            &cwd,
            None,
            None,
            None,
        );

        assert!(resolved
            .skills
            .iter()
            .any(|resource| same_existing_path(&resource.path, &repo_skill) && resource.enabled));
        assert!(resolved
            .skills
            .iter()
            .any(|resource| same_existing_path(&resource.path, &nested_skill) && resource.enabled));
        assert!(!resolved
            .skills
            .iter()
            .any(|resource| same_existing_path(&resource.path, &above_repo_skill)));
    }

    #[test]
    fn auto_discovered_prompts_and_themes_only_scan_top_level_like_pi() {
        let agent_dir = temp_dir();
        let project_base_dir = temp_dir().join(".pi");
        let cwd = temp_dir();
        write_file(&project_base_dir.join("prompts").join("top.md"));
        write_file(
            &project_base_dir
                .join("prompts")
                .join("nested")
                .join("deep.md"),
        );
        write_file(&project_base_dir.join("themes").join("top.json"));
        write_file(
            &project_base_dir
                .join("themes")
                .join("nested")
                .join("deep.json"),
        );

        let resolved = resolve_auto_discovered_resources(
            &agent_dir,
            &project_base_dir,
            &cwd,
            None,
            None,
            None,
        );
        let prompt_names = resolved
            .prompts
            .iter()
            .map(|resource| {
                Path::new(&resource.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let theme_names = resolved
            .themes
            .iter()
            .map(|resource| {
                Path::new(&resource.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert_eq!(prompt_names, vec!["top.md"]);
        assert_eq!(theme_names, vec!["top.json"]);
    }

    #[test]
    fn auto_discovered_prompts_and_themes_follow_symlinked_files_like_pi() {
        let root = temp_dir();
        let agent_dir = root.join("agent");
        let project_base_dir = root.join(".pi");
        let cwd = temp_dir();
        let shared_prompt = root.join("shared").join("linked.md");
        let shared_theme = root.join("shared").join("linked.json");
        write_file(&shared_prompt);
        write_file(&shared_theme);
        fs::create_dir_all(project_base_dir.join("prompts")).expect("prompts dir should exist");
        fs::create_dir_all(project_base_dir.join("themes")).expect("themes dir should exist");
        symlink_file(
            &shared_prompt,
            &project_base_dir.join("prompts").join("linked.md"),
        );
        symlink_file(
            &shared_theme,
            &project_base_dir.join("themes").join("linked.json"),
        );

        let resolved = resolve_auto_discovered_resources(
            &agent_dir,
            &project_base_dir,
            &cwd,
            None,
            None,
            None,
        );

        assert_eq!(resolved.prompts.len(), 1);
        assert_eq!(resolved.themes.len(), 1);
        assert_eq!(
            fs::canonicalize(&resolved.prompts[0].path).expect("prompt should exist"),
            fs::canonicalize(&shared_prompt).expect("source prompt should exist")
        );
        assert_eq!(
            fs::canonicalize(&resolved.themes[0].path).expect("theme should exist"),
            fs::canonicalize(&shared_theme).expect("source theme should exist")
        );
    }

    #[test]
    fn top_level_resolution_respects_ignore_files() {
        let dir = temp_dir();
        fs::write(dir.join(".gitignore"), "ignored.md\nnested/\n").expect("ignore should write");
        write_file(&dir.join("visible.md"));
        write_file(&dir.join("ignored.md"));
        write_file(&dir.join("nested").join("hidden.md"));

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::TopLevel,
            None,
        );
        let prompts = resolved
            .prompts
            .iter()
            .map(|resource| resource.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].ends_with("visible.md"));
    }

    #[test]
    fn top_level_filters_support_glob_and_overrides() {
        let dir = temp_dir();
        write_file(&dir.join("keep.md"));
        write_file(&dir.join("draft.md"));
        write_file(&dir.join("force.md"));
        write_file(&dir.join("remove.md"));
        let filter = PackageFilter {
            prompts: Some(vec![
                "*.md".to_string(),
                "!draft.md".to_string(),
                "+force.md".to_string(),
                "-remove.md".to_string(),
            ]),
            ..PackageFilter::default()
        };

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::TopLevel,
            Some(&filter),
        );
        let mut prompts = resolved
            .prompts
            .iter()
            .map(|resource| {
                (
                    Path::new(&resource.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.enabled,
                )
            })
            .collect::<Vec<_>>();
        prompts.sort();

        assert_eq!(
            prompts,
            vec![
                ("force.md".to_string(), true),
                ("keep.md".to_string(), true)
            ]
        );
    }

    #[test]
    fn package_filters_keep_disabled_resources_visible_like_pi() {
        let dir = temp_dir();
        write_file(&dir.join("prompts").join("keep.md"));
        write_file(&dir.join("prompts").join("draft.md"));
        write_file(&dir.join("prompts").join("force.md"));
        write_file(&dir.join("prompts").join("remove.md"));
        let filter = PackageFilter {
            prompts: Some(vec![
                "prompts/*.md".to_string(),
                "!prompts/draft.md".to_string(),
                "+prompts/force.md".to_string(),
                "-prompts/remove.md".to_string(),
            ]),
            ..PackageFilter::default()
        };

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::Package,
            Some(&filter),
        );
        let mut prompts = resolved
            .prompts
            .iter()
            .map(|resource| {
                (
                    Path::new(&resource.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.enabled,
                    resource.metadata.origin,
                )
            })
            .collect::<Vec<_>>();
        prompts.sort_by(|left, right| left.0.cmp(&right.0));

        assert_eq!(
            prompts,
            vec![
                ("draft.md".to_string(), false, SourceOrigin::Package),
                ("force.md".to_string(), true, SourceOrigin::Package),
                ("keep.md".to_string(), true, SourceOrigin::Package),
                ("remove.md".to_string(), false, SourceOrigin::Package),
            ]
        );
    }

    #[test]
    fn manifest_package_filters_keep_disabled_resources_visible_like_pi() {
        let dir = temp_dir();
        fs::write(
            dir.join("package.json"),
            r#"{"pi":{"prompts":["prompts/keep.md","prompts/draft.md","prompts/force.md","prompts/remove.md"]}}"#,
        )
        .expect("manifest should be written");
        write_file(&dir.join("prompts").join("keep.md"));
        write_file(&dir.join("prompts").join("draft.md"));
        write_file(&dir.join("prompts").join("force.md"));
        write_file(&dir.join("prompts").join("remove.md"));
        let filter = PackageFilter {
            prompts: Some(vec![
                "prompts/*.md".to_string(),
                "!prompts/draft.md".to_string(),
                "+prompts/force.md".to_string(),
                "-prompts/remove.md".to_string(),
            ]),
            ..PackageFilter::default()
        };

        let resolved = resolve_source(
            &display_path(&dir),
            SourceScope::User,
            SourceOrigin::Package,
            Some(&filter),
        );
        let mut prompts = resolved
            .prompts
            .iter()
            .map(|resource| {
                (
                    Path::new(&resource.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.enabled,
                    resource.metadata.origin,
                )
            })
            .collect::<Vec<_>>();
        prompts.sort_by(|left, right| left.0.cmp(&right.0));

        assert_eq!(
            prompts,
            vec![
                ("draft.md".to_string(), false, SourceOrigin::Package),
                ("force.md".to_string(), true, SourceOrigin::Package),
                ("keep.md".to_string(), true, SourceOrigin::Package),
                ("remove.md".to_string(), false, SourceOrigin::Package),
            ]
        );
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("pm-agent-package-manager-test-{id}-{counter}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn fake_npm_command(global_root: &Path) -> NpmCommandConfig {
        let bin_dir = temp_dir().join("bin");
        fs::create_dir_all(&bin_dir).expect("fake npm bin dir should be created");
        let command = bin_dir.join("npm");
        fs::write(
            &command,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"root\" ] && [ \"$2\" = \"-g\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 1\n",
                global_root.to_string_lossy()
            ),
        )
        .expect("fake npm command should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&command)
                .expect("fake npm metadata should exist")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&command, permissions)
                .expect("fake npm command should be executable");
        }

        NpmCommandConfig {
            command: display_path(command),
            args: Vec::new(),
        }
    }

    fn fake_pnpm_list_command(pnpm_root: &Path, package_path: &Path) -> NpmCommandConfig {
        fake_pnpm_command(&format!(
            r#"[{{"path":"{}","dependencies":{{"pnpm-pkg":{{"path":"{}"}}}}}}]"#,
            pnpm_root.to_string_lossy(),
            package_path.to_string_lossy()
        ))
    }

    fn fake_malformed_pnpm_list_command() -> NpmCommandConfig {
        fake_pnpm_command("not json")
    }

    fn fake_pnpm_command(output: &str) -> NpmCommandConfig {
        let bin_dir = temp_dir().join("bin");
        fs::create_dir_all(&bin_dir).expect("fake pnpm bin dir should be created");
        let command = bin_dir.join("wrapper");
        fs::write(
            &command,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"node@20\" ] && [ \"$3\" = \"--\" ] && [ \"$4\" = \"pnpm\" ] && [ \"$5\" = \"list\" ] && [ \"$6\" = \"-g\" ] && [ \"$7\" = \"--depth\" ] && [ \"$8\" = \"0\" ] && [ \"$9\" = \"--json\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 1\n",
                output.replace('\'', "'\\''")
            ),
        )
        .expect("fake pnpm command should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&command)
                .expect("fake pnpm metadata should exist")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&command, permissions)
                .expect("fake pnpm command should be executable");
        }

        NpmCommandConfig {
            command: display_path(command),
            args: vec![
                "exec".to_string(),
                "node@20".to_string(),
                "--".to_string(),
                "pnpm".to_string(),
            ],
        }
    }

    #[cfg(unix)]
    fn symlink_dir(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).expect("directory symlink should be created");
    }

    #[cfg(windows)]
    fn symlink_dir(source: &Path, target: &Path) {
        std::os::windows::fs::symlink_dir(source, target)
            .expect("directory symlink should be created");
    }

    #[cfg(unix)]
    fn symlink_file(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).expect("file symlink should be created");
    }

    #[cfg(windows)]
    fn symlink_file(source: &Path, target: &Path) {
        std::os::windows::fs::symlink_file(source, target).expect("file symlink should be created");
    }

    fn write_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent should be created");
        }
        fs::write(path, "").expect("file should be written");
    }

    fn same_existing_path(left: impl AsRef<Path>, right: impl AsRef<Path>) -> bool {
        fs::canonicalize(left.as_ref()).ok() == fs::canonicalize(right.as_ref()).ok()
    }
}
