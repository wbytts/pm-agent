use super::types::{ConfiguredPackage, SourceScope};
use crate::utils::{
    cleanup_windows_self_update_quarantine, is_newer_package_version,
    quarantine_windows_native_dependencies, LatestPiRelease,
};
use std::fs::OpenOptions;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCommand {
    Install,
    Remove,
    Update,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateTarget {
    All,
    SelfOnly,
    Extensions { source: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCommandOptions {
    pub command: PackageCommand,
    pub source: Option<String>,
    pub update_target: Option<UpdateTarget>,
    pub local: bool,
    pub force: bool,
    pub help: bool,
    pub invalid_option: Option<String>,
    pub invalid_argument: Option<String>,
    pub missing_option_value: Option<String>,
    pub conflicting_options: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCommandAction {
    Help { command: PackageCommand },
    Install { source: String, local: bool },
    Remove { source: String, local: bool },
    List,
    UpdatePackages { source: Option<String> },
    UpdateSelf { force: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCommandValidationError {
    InvalidOption(String),
    MissingOptionValue(String),
    InvalidArgument(String),
    ConflictingOptions(String),
    MissingSource(PackageCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdatePlan {
    pub package_name: String,
    pub should_run: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    BunBinary,
    Npm,
    Pnpm,
    Yarn,
    Bun,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateCommandStep {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateCommand {
    pub command: String,
    pub args: Vec<String>,
    pub display: String,
    pub steps: Vec<SelfUpdateCommandStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfUpdatePrepareAction {
    WindowsNpmQuarantine { package_dir: String },
}

pub trait SelfUpdatePrepareRunner {
    fn cleanup_windows_quarantine(&mut self, package_dir: &str) -> Result<(), String>;
    fn quarantine_windows_native_dependencies(&mut self, package_dir: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Default)]
pub struct SystemSelfUpdatePrepareRunner {
    loaded_files: Vec<std::path::PathBuf>,
    run_id: String,
}

impl SystemSelfUpdatePrepareRunner {
    pub fn new(loaded_files: Vec<std::path::PathBuf>, run_id: impl Into<String>) -> Self {
        Self {
            loaded_files,
            run_id: run_id.into(),
        }
    }
}

impl SelfUpdatePrepareRunner for SystemSelfUpdatePrepareRunner {
    fn cleanup_windows_quarantine(&mut self, package_dir: &str) -> Result<(), String> {
        cleanup_windows_self_update_quarantine(package_dir);
        Ok(())
    }

    fn quarantine_windows_native_dependencies(&mut self, package_dir: &str) -> Result<(), String> {
        quarantine_windows_native_dependencies(package_dir, &self.loaded_files, &self.run_id)
            .map(|_| ())
            .map_err(|error| format!("隔离 Windows native 依赖失败：{error}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmGlobalInstall {
    pub root: String,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPackageRootsPlan {
    pub command: Option<SelfUpdateCommandStep>,
    pub fallback_roots: Vec<String>,
    pub command_output_transform: GlobalPackageRootOutputTransform,
    pub require_command_success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalPackageRootOutputTransform {
    RootOnly,
    RootAndParent,
    RootAndNodeModules,
    BunBinInstallRoot,
}

pub fn package_command_usage(command: &PackageCommand, app_name: &str) -> String {
    match command {
        PackageCommand::Install => format!("{app_name} install <source> [-l]"),
        PackageCommand::Remove => format!("{app_name} remove <source> [-l]"),
        PackageCommand::Update => format!(
            "{app_name} update [source|self|pi] [--self] [--extensions] [--extension <source>] [--force]"
        ),
        PackageCommand::List => format!("{app_name} list"),
    }
}

pub fn package_command_help(command: &PackageCommand, app_name: &str) -> String {
    let usage = package_command_usage(command, app_name);
    match command {
        PackageCommand::Install => format!(
            "Usage:\n  {usage}\n\nInstall a package and add it to settings.\n\nOptions:\n  -l, --local    Install project-locally (.pi/settings.json)\n\nExamples:\n  {app_name} install npm:@foo/bar\n  {app_name} install git:github.com/user/repo\n  {app_name} install git:git@github.com:user/repo\n  {app_name} install https://github.com/user/repo\n  {app_name} install ssh://git@github.com/user/repo\n  {app_name} install ./local/path\n"
        ),
        PackageCommand::Remove => format!(
            "Usage:\n  {usage}\n\nRemove a package and its source from settings.\nAlias: {app_name} uninstall <source> [-l]\n\nOptions:\n  -l, --local    Remove from project settings (.pi/settings.json)\n\nExamples:\n  {app_name} remove npm:@foo/bar\n  {app_name} uninstall npm:@foo/bar\n"
        ),
        PackageCommand::Update => format!(
            "Usage:\n  {usage}\n\nUpdate pi and installed packages.\n\nOptions:\n  --self                  Update pi only\n  --extensions            Update installed packages only\n  --extension <source>    Update one package only\n  --force                 Reinstall pi even if the current version is latest\n\nShort forms:\n  {app_name} update                Update pi and all extensions\n  {app_name} update <source>       Update one package\n  {app_name} update pi             Update pi only (self works as alias to pi)\n"
        ),
        PackageCommand::List => {
            format!("Usage:\n  {usage}\n\nList installed packages from user and project settings.\n")
        }
    }
}

pub fn package_command_error_messages(
    command: &PackageCommand,
    error: &PackageCommandValidationError,
    app_name: &str,
) -> Vec<String> {
    let usage = package_command_usage(command, app_name);
    match error {
        PackageCommandValidationError::InvalidOption(option) => vec![
            format!("Unknown option {option} for \"{}\".", command_name(command)),
            format!("Use \"{app_name} --help\" or \"{usage}\"."),
        ],
        PackageCommandValidationError::MissingOptionValue(option) => vec![
            format!("Missing value for {option}."),
            format!("Usage: {usage}"),
        ],
        PackageCommandValidationError::InvalidArgument(argument) => vec![
            format!("Unexpected argument {argument}."),
            format!("Usage: {usage}"),
        ],
        PackageCommandValidationError::ConflictingOptions(message) => {
            vec![message.clone(), format!("Usage: {usage}")]
        }
        PackageCommandValidationError::MissingSource(missing_command) => vec![
            format!("Missing {} source.", command_name(missing_command)),
            format!("Usage: {usage}"),
        ],
    }
}

pub fn format_package_list(packages: &[ConfiguredPackage]) -> Vec<String> {
    if packages.is_empty() {
        return vec!["No packages installed.".to_string()];
    }

    let user_packages = packages
        .iter()
        .filter(|package| package.scope == SourceScope::User)
        .collect::<Vec<_>>();
    let project_packages = packages
        .iter()
        .filter(|package| package.scope == SourceScope::Project)
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    append_package_group(&mut lines, "User packages:", &user_packages);
    if !user_packages.is_empty() && !project_packages.is_empty() {
        lines.push(String::new());
    }
    append_package_group(&mut lines, "Project packages:", &project_packages);
    lines
}

pub fn plan_self_update(
    current_version: &str,
    force: bool,
    latest_release: Option<LatestPiRelease>,
    installed_package_name: &str,
) -> SelfUpdatePlan {
    if force {
        return SelfUpdatePlan {
            package_name: installed_package_name.to_string(),
            should_run: true,
            note: None,
        };
    }

    let Some(release) = latest_release else {
        return SelfUpdatePlan {
            package_name: installed_package_name.to_string(),
            should_run: true,
            note: None,
        };
    };

    let package_name = release
        .package_name
        .unwrap_or_else(|| installed_package_name.to_string());
    if package_name != installed_package_name
        || is_newer_package_version(&release.version, current_version)
    {
        return SelfUpdatePlan {
            package_name,
            should_run: true,
            note: release.note,
        };
    }

    SelfUpdatePlan {
        package_name,
        should_run: false,
        note: None,
    }
}

pub fn self_update_command_for_method(
    method: InstallMethod,
    installed_package_name: &str,
    update_package_name: &str,
    npm_command: Option<Vec<String>>,
    inferred_npm_prefix: Option<String>,
) -> Option<SelfUpdateCommand> {
    match method {
        InstallMethod::BunBinary | InstallMethod::Unknown => None,
        InstallMethod::Pnpm => Some(make_self_update_command(
            make_self_update_command_step(
                "pnpm",
                ["install", "-g", "--ignore-scripts", update_package_name],
            ),
            uninstall_step_if_package_changed(
                "pnpm",
                ["remove", "-g", installed_package_name],
                installed_package_name,
                update_package_name,
            ),
        )),
        InstallMethod::Yarn => Some(make_self_update_command(
            make_self_update_command_step(
                "yarn",
                ["global", "add", "--ignore-scripts", update_package_name],
            ),
            uninstall_step_if_package_changed(
                "yarn",
                ["global", "remove", installed_package_name],
                installed_package_name,
                update_package_name,
            ),
        )),
        InstallMethod::Bun => Some(make_self_update_command(
            make_self_update_command_step(
                "bun",
                ["install", "-g", "--ignore-scripts", update_package_name],
            ),
            uninstall_step_if_package_changed(
                "bun",
                ["uninstall", "-g", installed_package_name],
                installed_package_name,
                update_package_name,
            ),
        )),
        InstallMethod::Npm => {
            let mut npm_parts = npm_command.unwrap_or_default();
            let command = if npm_parts.is_empty() {
                "npm".to_string()
            } else {
                npm_parts.remove(0)
            };
            if let Some(prefix) = inferred_npm_prefix {
                npm_parts.push("--prefix".to_string());
                npm_parts.push(prefix);
            }

            let mut install_args = npm_parts.clone();
            install_args.extend(
                ["install", "-g", "--ignore-scripts", update_package_name]
                    .into_iter()
                    .map(str::to_string),
            );

            let uninstall_step = if update_package_name == installed_package_name {
                None
            } else {
                let mut uninstall_args = npm_parts;
                uninstall_args.extend(
                    ["uninstall", "-g", installed_package_name]
                        .into_iter()
                        .map(str::to_string),
                );
                Some(make_self_update_command_step_owned(
                    command.clone(),
                    uninstall_args,
                ))
            };

            Some(make_self_update_command(
                make_self_update_command_step_owned(command, install_args),
                uninstall_step,
            ))
        }
    }
}

pub fn self_update_prepare_actions(
    platform: &str,
    method: InstallMethod,
    package_dir: impl Into<String>,
) -> Vec<SelfUpdatePrepareAction> {
    if platform == "win32" && method == InstallMethod::Npm {
        return vec![SelfUpdatePrepareAction::WindowsNpmQuarantine {
            package_dir: package_dir.into(),
        }];
    }
    Vec::new()
}

pub fn run_self_update_prepare_actions<R: SelfUpdatePrepareRunner>(
    runner: &mut R,
    actions: &[SelfUpdatePrepareAction],
) -> Result<(), String> {
    for action in actions {
        match action {
            SelfUpdatePrepareAction::WindowsNpmQuarantine { package_dir } => {
                runner.cleanup_windows_quarantine(package_dir)?;
                runner.quarantine_windows_native_dependencies(package_dir)?;
            }
        }
    }
    Ok(())
}

pub fn detect_install_method_from_context(
    is_bun_binary: bool,
    is_bun_runtime: bool,
    package_dir: &str,
    exec_path: &str,
) -> InstallMethod {
    if is_bun_binary {
        return InstallMethod::BunBinary;
    }

    let resolved_path = format!("{package_dir}\0{exec_path}")
        .to_lowercase()
        .replace('\\', "/");
    if resolved_path.contains("/pnpm/") || resolved_path.contains("/.pnpm/") {
        return InstallMethod::Pnpm;
    }
    if resolved_path.contains("/yarn/") || resolved_path.contains("/.yarn/") {
        return InstallMethod::Yarn;
    }
    if is_bun_runtime || resolved_path.contains("/install/global/node_modules/") {
        return InstallMethod::Bun;
    }
    if resolved_path.contains("/npm/") || resolved_path.contains("/node_modules/") {
        return InstallMethod::Npm;
    }
    InstallMethod::Unknown
}

pub fn infer_npm_global_install_from_package_dir(
    package_dir: &str,
    windows_path_shape: bool,
) -> Option<NpmGlobalInstall> {
    let path = if windows_path_shape {
        split_windows_path(package_dir)
    } else {
        split_path(package_dir)
    };
    let package_index = path.len().checked_sub(1)?;
    let package_parent_index = package_index.checked_sub(1)?;

    let root_index = if path.get(package_parent_index)?.starts_with('@')
        && path.get(package_parent_index.checked_sub(1)?)? == "node_modules"
    {
        package_parent_index.checked_sub(1)?
    } else if path.get(package_parent_index)? == "node_modules" {
        package_parent_index
    } else {
        return None;
    };

    let root_parent_index = root_index.checked_sub(1)?;
    if path.get(root_parent_index)? != "lib" {
        return None;
    }

    let root = join_path_segments(&path[..=root_index], windows_path_shape);
    let prefix = join_path_segments(&path[..root_parent_index], windows_path_shape);
    Some(NpmGlobalInstall { root, prefix })
}

pub fn self_update_command_from_context(
    method: InstallMethod,
    installed_package_name: &str,
    update_package_name: &str,
    npm_command: Option<Vec<String>>,
    package_dir_candidates: &[String],
    global_root_candidates: &[String],
    is_self_update_path_writable: bool,
    case_insensitive: bool,
) -> Option<SelfUpdateCommand> {
    let npm_command = npm_command.filter(|command| !command.is_empty());
    let inferred_npm_install = if method == InstallMethod::Npm && npm_command.is_none() {
        package_dir_candidates.iter().find_map(|package_dir| {
            infer_npm_global_install_from_package_dir(package_dir, case_insensitive)
        })
    } else {
        None
    };

    let global_roots = global_root_candidates
        .iter()
        .cloned()
        .chain(
            inferred_npm_install
                .iter()
                .map(|install| install.root.clone()),
        )
        .collect::<Vec<_>>();

    if !is_self_update_path_writable
        || !is_managed_by_global_package_manager(
            package_dir_candidates,
            &global_roots,
            case_insensitive,
        )
    {
        return None;
    }

    self_update_command_for_method(
        method,
        installed_package_name,
        update_package_name,
        npm_command,
        inferred_npm_install.map(|install| install.prefix),
    )
}

pub fn self_update_path_is_writable(package_dir: impl AsRef<Path>) -> bool {
    let package_dir = package_dir.as_ref();
    let Some(parent) = package_dir.parent() else {
        return false;
    };

    path_allows_writes(package_dir) && path_allows_writes(parent)
}

pub fn entrypoint_package_dir(entrypoint: impl AsRef<Path>) -> Option<PathBuf> {
    let mut dir = entrypoint.as_ref().parent()?.to_path_buf();
    while let Some(parent) = dir.parent() {
        if dir.join("package.json").exists() {
            return Some(dir);
        }
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    None
}

pub fn path_comparison_candidates(path: impl AsRef<Path>, case_insensitive: bool) -> Vec<String> {
    let path = path.as_ref();
    if !path.exists() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    push_unique_path_candidate(&mut candidates, absolute_path(path), case_insensitive);
    if let Ok(real_path) = path.canonicalize() {
        push_unique_path_candidate(&mut candidates, real_path, case_insensitive);
    }
    candidates
}

pub fn self_update_package_dir_candidates(
    package_dir: impl AsRef<Path>,
    entrypoint: Option<impl AsRef<Path>>,
    case_insensitive: bool,
) -> Vec<String> {
    let mut candidates = path_comparison_candidates(package_dir, case_insensitive);
    if let Some(entrypoint_package_dir) = entrypoint.and_then(entrypoint_package_dir) {
        for candidate in path_comparison_candidates(entrypoint_package_dir, case_insensitive) {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

pub fn global_package_roots_plan(
    method: InstallMethod,
    npm_command: Option<Vec<String>>,
    home_dir: &str,
    inferred_npm_install: Option<NpmGlobalInstall>,
) -> GlobalPackageRootsPlan {
    match method {
        InstallMethod::Npm => {
            let configured = npm_command
                .as_ref()
                .is_some_and(|command| !command.is_empty());
            let mut npm_parts = npm_command.unwrap_or_default();
            let command = if npm_parts.is_empty() {
                "npm".to_string()
            } else {
                npm_parts.remove(0)
            };

            if configured && command == "bun" {
                return GlobalPackageRootsPlan {
                    command: Some(make_self_update_command_step_owned(
                        command,
                        npm_parts
                            .into_iter()
                            .chain(["pm", "bin", "-g"].into_iter().map(str::to_string))
                            .collect(),
                    )),
                    fallback_roots: vec![bun_global_node_modules_root(home_dir)],
                    command_output_transform: GlobalPackageRootOutputTransform::BunBinInstallRoot,
                    require_command_success: true,
                };
            }

            GlobalPackageRootsPlan {
                command: Some(make_self_update_command_step_owned(
                    command,
                    npm_parts
                        .into_iter()
                        .chain(["root", "-g"].into_iter().map(str::to_string))
                        .collect(),
                )),
                fallback_roots: if configured {
                    Vec::new()
                } else {
                    inferred_npm_install
                        .map(|install| vec![install.root])
                        .unwrap_or_default()
                },
                command_output_transform: GlobalPackageRootOutputTransform::RootOnly,
                require_command_success: configured,
            }
        }
        InstallMethod::Pnpm => GlobalPackageRootsPlan {
            command: Some(make_self_update_command_step("pnpm", ["root", "-g"])),
            fallback_roots: Vec::new(),
            command_output_transform: GlobalPackageRootOutputTransform::RootAndParent,
            require_command_success: false,
        },
        InstallMethod::Yarn => GlobalPackageRootsPlan {
            command: Some(make_self_update_command_step("yarn", ["global", "dir"])),
            fallback_roots: Vec::new(),
            command_output_transform: GlobalPackageRootOutputTransform::RootAndNodeModules,
            require_command_success: false,
        },
        InstallMethod::Bun => GlobalPackageRootsPlan {
            command: Some(make_self_update_command_step("bun", ["pm", "bin", "-g"])),
            fallback_roots: vec![bun_global_node_modules_root(home_dir)],
            command_output_transform: GlobalPackageRootOutputTransform::BunBinInstallRoot,
            require_command_success: false,
        },
        InstallMethod::BunBinary | InstallMethod::Unknown => GlobalPackageRootsPlan {
            command: None,
            fallback_roots: Vec::new(),
            command_output_transform: GlobalPackageRootOutputTransform::RootOnly,
            require_command_success: false,
        },
    }
}

pub fn global_package_roots_from_plan(
    plan: &GlobalPackageRootsPlan,
    command_output: Option<&str>,
) -> Vec<String> {
    let mut roots = plan.fallback_roots.clone();
    let Some(output) = command_output
        .map(str::trim)
        .filter(|output| !output.is_empty())
    else {
        return roots;
    };

    match plan.command_output_transform {
        GlobalPackageRootOutputTransform::RootOnly => {
            roots.push(output.to_string());
        }
        GlobalPackageRootOutputTransform::RootAndParent => {
            roots.push(output.to_string());
            if let Some(parent) = parent_path(output) {
                roots.push(parent);
            }
        }
        GlobalPackageRootOutputTransform::RootAndNodeModules => {
            roots.push(output.to_string());
            roots.push(
                Path::new(output)
                    .join("node_modules")
                    .to_string_lossy()
                    .to_string(),
            );
        }
        GlobalPackageRootOutputTransform::BunBinInstallRoot => {
            if let Some(parent) = parent_path(output) {
                roots.push(
                    Path::new(&parent)
                        .join("install")
                        .join("global")
                        .join("node_modules")
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
    }
    roots
}

pub fn self_update_unavailable_instruction(
    method: InstallMethod,
    installed_package_name: &str,
    update_package_name: &str,
    npm_command: Option<Vec<String>>,
    inferred_npm_prefix: Option<String>,
    is_managed_by_global_package_manager: bool,
    is_self_update_path_writable: bool,
) -> String {
    if method == InstallMethod::BunBinary {
        return "Download from: https://github.com/earendil-works/pi-mono/releases/latest"
            .to_string();
    }

    if let Some(command) = self_update_command_for_method(
        method,
        installed_package_name,
        update_package_name,
        npm_command,
        inferred_npm_prefix,
    ) {
        if is_managed_by_global_package_manager && !is_self_update_path_writable {
            return format!(
                "This installation is managed by a global {} install, but the install path is not writable. Update it yourself with: {}",
                install_method_name(method),
                command.display
            );
        }
        return format!(
            "This installation is not managed by a global {} install. Update it with the package manager, wrapper, or source checkout that provides it.",
            install_method_name(method)
        );
    }

    format!(
        "Update {update_package_name} using the package manager, wrapper, or source checkout that provides this installation."
    )
}

pub fn update_instruction(
    method: InstallMethod,
    package_name: &str,
    npm_command: Option<Vec<String>>,
    inferred_npm_prefix: Option<String>,
) -> String {
    if let Some(command) = self_update_command_for_method(
        method,
        package_name,
        package_name,
        npm_command,
        inferred_npm_prefix,
    ) {
        format!("Run: {}", command.display)
    } else {
        self_update_unavailable_instruction(
            method,
            package_name,
            package_name,
            None,
            None,
            false,
            false,
        )
    }
}

pub fn is_managed_by_global_package_manager(
    package_dir_candidates: &[String],
    global_root_candidates: &[String],
    case_insensitive: bool,
) -> bool {
    let package_dirs = package_dir_candidates
        .iter()
        .map(|path| normalize_path_for_comparison(path, case_insensitive))
        .collect::<Vec<_>>();

    global_root_candidates.iter().any(|root| {
        let root = normalize_path_for_comparison(root, case_insensitive);
        let root_prefix = if root.ends_with('/') {
            root
        } else {
            format!("{root}/")
        };
        package_dirs
            .iter()
            .any(|package_dir| package_dir.starts_with(&root_prefix))
    })
}

pub fn parse_package_command(args: &[String]) -> Option<PackageCommandOptions> {
    let (raw_command, rest) = args.split_first()?;
    let command = match raw_command.as_str() {
        "install" => PackageCommand::Install,
        "remove" | "uninstall" => PackageCommand::Remove,
        "update" => PackageCommand::Update,
        "list" => PackageCommand::List,
        _ => return None,
    };

    let mut local = false;
    let mut force = false;
    let mut help = false;
    let mut invalid_option = None;
    let mut invalid_argument = None;
    let mut missing_option_value = None;
    let mut conflicting_options = None;
    let mut source = None;
    let mut self_flag = false;
    let mut extensions_flag = false;
    let mut extension_flag_source = None;

    let mut index = 0;
    while index < rest.len() {
        let arg = &rest[index];
        match arg.as_str() {
            "-h" | "--help" => {
                help = true;
                index += 1;
            }
            "-l" | "--local" => {
                if matches!(command, PackageCommand::Install | PackageCommand::Remove) {
                    local = true;
                } else {
                    invalid_option.get_or_insert_with(|| arg.clone());
                }
                index += 1;
            }
            "--self" => {
                if command == PackageCommand::Update {
                    self_flag = true;
                } else {
                    invalid_option.get_or_insert_with(|| arg.clone());
                }
                index += 1;
            }
            "--extensions" => {
                if command == PackageCommand::Update {
                    extensions_flag = true;
                } else {
                    invalid_option.get_or_insert_with(|| arg.clone());
                }
                index += 1;
            }
            "--force" => {
                if command == PackageCommand::Update {
                    force = true;
                } else {
                    invalid_option.get_or_insert_with(|| arg.clone());
                }
                index += 1;
            }
            "--extension" => {
                if command != PackageCommand::Update {
                    invalid_option.get_or_insert_with(|| arg.clone());
                    index += 1;
                    continue;
                }
                let value = rest.get(index + 1);
                if value.is_none_or(|value| value.starts_with('-')) {
                    missing_option_value.get_or_insert_with(|| arg.clone());
                    index += 1;
                } else if extension_flag_source.is_some() {
                    conflicting_options
                        .get_or_insert_with(|| "--extension can only be provided once".to_string());
                    index += 2;
                } else {
                    extension_flag_source = value.cloned();
                    index += 2;
                }
            }
            _ if arg.starts_with('-') => {
                invalid_option.get_or_insert_with(|| arg.clone());
                index += 1;
            }
            _ => {
                if source.is_none() {
                    source = Some(arg.clone());
                } else {
                    invalid_argument.get_or_insert_with(|| arg.clone());
                }
                index += 1;
            }
        }
    }

    let update_target = if command == PackageCommand::Update {
        Some(update_target(
            source.as_deref(),
            self_flag,
            extensions_flag,
            extension_flag_source.as_deref(),
            &mut conflicting_options,
        ))
    } else {
        None
    };

    Some(PackageCommandOptions {
        command,
        source,
        update_target,
        local,
        force,
        help,
        invalid_option,
        invalid_argument,
        missing_option_value,
        conflicting_options,
    })
}

pub fn package_command_actions(
    options: &PackageCommandOptions,
) -> Result<Vec<PackageCommandAction>, PackageCommandValidationError> {
    if options.help {
        return Ok(vec![PackageCommandAction::Help {
            command: options.command.clone(),
        }]);
    }
    if let Some(option) = &options.invalid_option {
        return Err(PackageCommandValidationError::InvalidOption(option.clone()));
    }
    if let Some(option) = &options.missing_option_value {
        return Err(PackageCommandValidationError::MissingOptionValue(
            option.clone(),
        ));
    }
    if let Some(argument) = &options.invalid_argument {
        return Err(PackageCommandValidationError::InvalidArgument(
            argument.clone(),
        ));
    }
    if let Some(conflict) = &options.conflicting_options {
        return Err(PackageCommandValidationError::ConflictingOptions(
            conflict.clone(),
        ));
    }

    match options.command {
        PackageCommand::Install => {
            let source = required_source(options)?;
            Ok(vec![PackageCommandAction::Install {
                source,
                local: options.local,
            }])
        }
        PackageCommand::Remove => {
            let source = required_source(options)?;
            Ok(vec![PackageCommandAction::Remove {
                source,
                local: options.local,
            }])
        }
        PackageCommand::List => Ok(vec![PackageCommandAction::List]),
        PackageCommand::Update => Ok(update_actions(
            options.update_target.as_ref().unwrap_or(&UpdateTarget::All),
            options.force,
        )),
    }
}

fn required_source(
    options: &PackageCommandOptions,
) -> Result<String, PackageCommandValidationError> {
    options
        .source
        .clone()
        .ok_or_else(|| PackageCommandValidationError::MissingSource(options.command.clone()))
}

fn update_actions(target: &UpdateTarget, force: bool) -> Vec<PackageCommandAction> {
    match target {
        UpdateTarget::All => vec![
            PackageCommandAction::UpdatePackages { source: None },
            PackageCommandAction::UpdateSelf { force },
        ],
        UpdateTarget::SelfOnly => vec![PackageCommandAction::UpdateSelf { force }],
        UpdateTarget::Extensions { source } => {
            vec![PackageCommandAction::UpdatePackages {
                source: source.clone(),
            }]
        }
    }
}

fn command_name(command: &PackageCommand) -> &'static str {
    match command {
        PackageCommand::Install => "install",
        PackageCommand::Remove => "remove",
        PackageCommand::Update => "update",
        PackageCommand::List => "list",
    }
}

fn append_package_group(lines: &mut Vec<String>, header: &str, packages: &[&ConfiguredPackage]) {
    if packages.is_empty() {
        return;
    }
    lines.push(header.to_string());
    for package in packages {
        let display = if package.filtered {
            format!("{} (filtered)", package.source)
        } else {
            package.source.clone()
        };
        lines.push(format!("  {display}"));
        if let Some(installed_path) = &package.installed_path {
            lines.push(format!("    {installed_path}"));
        }
    }
}

fn make_self_update_command(
    install_step: SelfUpdateCommandStep,
    uninstall_step: Option<SelfUpdateCommandStep>,
) -> SelfUpdateCommand {
    match uninstall_step {
        Some(uninstall_step) => SelfUpdateCommand {
            command: install_step.command.clone(),
            args: install_step.args.clone(),
            display: format!("{} && {}", uninstall_step.display, install_step.display),
            steps: vec![uninstall_step, install_step],
        },
        None => SelfUpdateCommand {
            command: install_step.command.clone(),
            args: install_step.args.clone(),
            display: install_step.display.clone(),
            steps: Vec::new(),
        },
    }
}

fn make_self_update_command_step<const N: usize>(
    command: &str,
    args: [&str; N],
) -> SelfUpdateCommandStep {
    make_self_update_command_step_owned(
        command.to_string(),
        args.into_iter().map(str::to_string).collect(),
    )
}

fn make_self_update_command_step_owned(
    command: String,
    args: Vec<String>,
) -> SelfUpdateCommandStep {
    let display = std::iter::once(command.as_str())
        .chain(args.iter().map(String::as_str))
        .map(shell_display_arg)
        .collect::<Vec<_>>()
        .join(" ");
    SelfUpdateCommandStep {
        command,
        args,
        display,
    }
}

fn uninstall_step_if_package_changed<const N: usize>(
    command: &str,
    args: [&str; N],
    installed_package_name: &str,
    update_package_name: &str,
) -> Option<SelfUpdateCommandStep> {
    (update_package_name != installed_package_name)
        .then(|| make_self_update_command_step(command, args))
}

fn shell_display_arg(arg: &str) -> String {
    if arg.chars().any(char::is_whitespace) {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

fn install_method_name(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::BunBinary => "bun-binary",
        InstallMethod::Npm => "npm",
        InstallMethod::Pnpm => "pnpm",
        InstallMethod::Yarn => "yarn",
        InstallMethod::Bun => "bun",
        InstallMethod::Unknown => "unknown",
    }
}

fn normalize_path_for_comparison(path: &str, case_insensitive: bool) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = collapse_repeated_slashes(&normalized);
    let normalized = normalized.trim_end_matches('/').to_string();
    if case_insensitive {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn collapse_repeated_slashes(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut previous_was_slash = false;
    for character in path.chars() {
        if character == '/' {
            if !previous_was_slash {
                result.push(character);
            }
            previous_was_slash = true;
        } else {
            result.push(character);
            previous_was_slash = false;
        }
    }
    result
}

fn split_path(path: &str) -> Vec<String> {
    Path::new(path)
        .components()
        .filter_map(|component| match component {
            Component::RootDir => Some("/".to_string()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            Component::CurDir | Component::ParentDir => None,
        })
        .collect()
}

fn split_windows_path(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn join_path_segments(segments: &[String], windows_path_shape: bool) -> String {
    if windows_path_shape {
        return segments.join("\\");
    }

    let mut path = PathBuf::new();
    for segment in segments {
        path.push(segment);
    }
    path.to_string_lossy().to_string()
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn push_unique_path_candidate(candidates: &mut Vec<String>, path: PathBuf, case_insensitive: bool) {
    let candidate = normalize_path_for_comparison(&path.to_string_lossy(), case_insensitive);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn bun_global_node_modules_root(home_dir: &str) -> String {
    Path::new(home_dir)
        .join(".bun")
        .join("install")
        .join("global")
        .join("node_modules")
        .to_string_lossy()
        .to_string()
}

fn parent_path(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
}

fn path_allows_writes(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let probe = path.join(format!(".pm-agent-write-test-{}", std::process::id()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn update_target(
    source: Option<&str>,
    self_flag: bool,
    extensions_flag: bool,
    extension_flag_source: Option<&str>,
    conflicting_options: &mut Option<String>,
) -> UpdateTarget {
    if let Some(extension_source) = extension_flag_source {
        if self_flag || extensions_flag {
            conflicting_options.get_or_insert_with(|| {
                "--extension cannot be combined with --self or --extensions".to_string()
            });
        }
        if source.is_some() {
            conflicting_options.get_or_insert_with(|| {
                "--extension cannot be combined with a positional source".to_string()
            });
        }
        return UpdateTarget::Extensions {
            source: Some(extension_source.to_string()),
        };
    }

    if let Some(source) = source {
        if source == "self" || source == "pi" {
            return if extensions_flag {
                UpdateTarget::All
            } else {
                UpdateTarget::SelfOnly
            };
        }
        if extensions_flag || self_flag {
            conflicting_options.get_or_insert_with(|| {
                "positional update targets cannot be combined with --self or --extensions"
                    .to_string()
            });
        }
        return UpdateTarget::Extensions {
            source: Some(source.to_string()),
        };
    }

    if self_flag && extensions_flag {
        UpdateTarget::All
    } else if self_flag {
        UpdateTarget::SelfOnly
    } else if extensions_flag {
        UpdateTarget::Extensions { source: None }
    } else {
        UpdateTarget::All
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_manager::{ConfiguredPackage, SourceScope};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_install_remove_and_uninstall_like_pi_package_cli() {
        assert_eq!(
            parse_package_command(&args(&["install", "npm:pkg", "--local"])),
            Some(PackageCommandOptions {
                command: PackageCommand::Install,
                source: Some("npm:pkg".to_string()),
                update_target: None,
                local: true,
                force: false,
                help: false,
                invalid_option: None,
                invalid_argument: None,
                missing_option_value: None,
                conflicting_options: None,
            })
        );

        assert_eq!(
            parse_package_command(&args(&["uninstall", "npm:pkg"])).map(|options| options.command),
            Some(PackageCommand::Remove)
        );
    }

    #[test]
    fn parses_update_targets_and_conflicts_like_pi_package_cli() {
        assert_eq!(
            parse_package_command(&args(&["update"])).and_then(|options| options.update_target),
            Some(UpdateTarget::All)
        );
        assert_eq!(
            parse_package_command(&args(&["update", "pi"]))
                .and_then(|options| options.update_target),
            Some(UpdateTarget::SelfOnly)
        );
        assert_eq!(
            parse_package_command(&args(&["update", "--extensions"]))
                .and_then(|options| options.update_target),
            Some(UpdateTarget::Extensions { source: None })
        );
        assert_eq!(
            parse_package_command(&args(&["update", "--extension", "npm:pkg"]))
                .and_then(|options| options.update_target),
            Some(UpdateTarget::Extensions {
                source: Some("npm:pkg".to_string())
            })
        );
        assert_eq!(
            parse_package_command(&args(&["update", "--extension", "npm:pkg", "--self"]))
                .and_then(|options| options.conflicting_options),
            Some("--extension cannot be combined with --self or --extensions".to_string())
        );
    }

    #[test]
    fn reports_invalid_arguments_and_missing_values_like_pi_package_cli() {
        assert_eq!(
            parse_package_command(&args(&["install", "npm:pkg", "extra"]))
                .and_then(|options| options.invalid_argument),
            Some("extra".to_string())
        );
        assert_eq!(
            parse_package_command(&args(&["update", "--extension"]))
                .and_then(|options| options.missing_option_value),
            Some("--extension".to_string())
        );
        assert_eq!(
            parse_package_command(&args(&["list", "--local"]))
                .and_then(|options| options.invalid_option),
            Some("--local".to_string())
        );
    }

    #[test]
    fn maps_valid_commands_to_package_actions_like_pi_handle_package_command() {
        assert_eq!(
            package_command_actions(
                &parse_package_command(&args(&["install", "npm:pkg", "--local"])).unwrap()
            ),
            Ok(vec![PackageCommandAction::Install {
                source: "npm:pkg".to_string(),
                local: true,
            }])
        );
        assert_eq!(
            package_command_actions(&parse_package_command(&args(&["remove", "npm:pkg"])).unwrap()),
            Ok(vec![PackageCommandAction::Remove {
                source: "npm:pkg".to_string(),
                local: false,
            }])
        );
        assert_eq!(
            package_command_actions(&parse_package_command(&args(&["list"])).unwrap()),
            Ok(vec![PackageCommandAction::List])
        );
    }

    #[test]
    fn maps_update_targets_to_ordered_actions_like_pi_handle_package_command() {
        assert_eq!(
            package_command_actions(&parse_package_command(&args(&["update"])).unwrap()),
            Ok(vec![
                PackageCommandAction::UpdatePackages { source: None },
                PackageCommandAction::UpdateSelf { force: false },
            ])
        );
        assert_eq!(
            package_command_actions(
                &parse_package_command(&args(&["update", "--extensions"])).unwrap()
            ),
            Ok(vec![PackageCommandAction::UpdatePackages { source: None }])
        );
        assert_eq!(
            package_command_actions(
                &parse_package_command(&args(&["update", "--extension", "npm:pkg"])).unwrap()
            ),
            Ok(vec![PackageCommandAction::UpdatePackages {
                source: Some("npm:pkg".to_string()),
            }])
        );
        assert_eq!(
            package_command_actions(
                &parse_package_command(&args(&["update", "--self", "--force"])).unwrap()
            ),
            Ok(vec![PackageCommandAction::UpdateSelf { force: true }])
        );
    }

    #[test]
    fn validates_package_command_options_like_pi_handle_package_command() {
        assert_eq!(
            package_command_actions(&parse_package_command(&args(&["install"])).unwrap()),
            Err(PackageCommandValidationError::MissingSource(
                PackageCommand::Install
            ))
        );
        assert_eq!(
            package_command_actions(&parse_package_command(&args(&["list", "--local"])).unwrap()),
            Err(PackageCommandValidationError::InvalidOption(
                "--local".to_string()
            ))
        );
        assert_eq!(
            package_command_actions(
                &parse_package_command(&args(&["update", "--extension", "npm:pkg", "--self"]))
                    .unwrap()
            ),
            Err(PackageCommandValidationError::ConflictingOptions(
                "--extension cannot be combined with --self or --extensions".to_string()
            ))
        );
    }

    #[test]
    fn formats_usage_help_and_validation_errors_like_pi_package_cli() {
        assert_eq!(
            package_command_usage(&PackageCommand::Install, "pi"),
            "pi install <source> [-l]"
        );
        assert_eq!(
            package_command_usage(&PackageCommand::Update, "pi"),
            "pi update [source|self|pi] [--self] [--extensions] [--extension <source>] [--force]"
        );

        let update_help = package_command_help(&PackageCommand::Update, "pi");
        assert!(update_help.contains("Usage:\n  pi update"));
        assert!(update_help.contains("Update pi and installed packages."));
        assert!(update_help.contains("--extension <source>    Update one package only"));

        assert_eq!(
            package_command_error_messages(
                &PackageCommand::List,
                &PackageCommandValidationError::InvalidOption("--local".to_string()),
                "pi",
            ),
            vec![
                "Unknown option --local for \"list\".".to_string(),
                "Use \"pi --help\" or \"pi list\".".to_string(),
            ]
        );
        assert_eq!(
            package_command_error_messages(
                &PackageCommand::Install,
                &PackageCommandValidationError::MissingSource(PackageCommand::Install),
                "pi",
            ),
            vec![
                "Missing install source.".to_string(),
                "Usage: pi install <source> [-l]".to_string(),
            ]
        );
    }

    #[test]
    fn formats_package_list_like_pi_handle_package_command() {
        assert_eq!(format_package_list(&[]), vec!["No packages installed."]);

        let lines = format_package_list(&[
            ConfiguredPackage {
                source: "npm:user-package".to_string(),
                scope: SourceScope::User,
                filtered: false,
                installed_path: Some("/agent/npm/node_modules/user-package".to_string()),
            },
            ConfiguredPackage {
                source: "git:github.com/project/repo".to_string(),
                scope: SourceScope::Project,
                filtered: true,
                installed_path: Some(".pi/git/github.com/project/repo".to_string()),
            },
        ]);

        assert_eq!(
            lines,
            vec![
                "User packages:".to_string(),
                "  npm:user-package".to_string(),
                "    /agent/npm/node_modules/user-package".to_string(),
                String::new(),
                "Project packages:".to_string(),
                "  git:github.com/project/repo (filtered)".to_string(),
                "    .pi/git/github.com/project/repo".to_string(),
            ]
        );
    }

    #[test]
    fn plans_self_update_like_pi_package_cli() {
        assert_eq!(
            plan_self_update("0.70.5", true, None, "pi"),
            SelfUpdatePlan {
                package_name: "pi".to_string(),
                should_run: true,
                note: None,
            }
        );
        assert_eq!(
            plan_self_update("0.70.5", false, None, "pi"),
            SelfUpdatePlan {
                package_name: "pi".to_string(),
                should_run: true,
                note: None,
            }
        );
        assert_eq!(
            plan_self_update(
                "0.70.5",
                false,
                Some(LatestPiRelease {
                    version: "0.70.5".to_string(),
                    package_name: None,
                    note: Some("ignored".to_string()),
                }),
                "pi",
            ),
            SelfUpdatePlan {
                package_name: "pi".to_string(),
                should_run: false,
                note: None,
            }
        );
        assert_eq!(
            plan_self_update(
                "0.70.5",
                false,
                Some(LatestPiRelease {
                    version: "0.70.5".to_string(),
                    package_name: Some("@scope/pi-next".to_string()),
                    note: Some("read this".to_string()),
                }),
                "pi",
            ),
            SelfUpdatePlan {
                package_name: "@scope/pi-next".to_string(),
                should_run: true,
                note: Some("read this".to_string()),
            }
        );
        assert_eq!(
            plan_self_update(
                "0.70.5",
                false,
                Some(LatestPiRelease {
                    version: "0.70.6".to_string(),
                    package_name: None,
                    note: Some("read this".to_string()),
                }),
                "pi",
            ),
            SelfUpdatePlan {
                package_name: "pi".to_string(),
                should_run: true,
                note: Some("read this".to_string()),
            }
        );
    }

    #[test]
    fn plans_self_update_commands_like_pi_install_methods() {
        assert_eq!(
            self_update_command_for_method(InstallMethod::Pnpm, "pi", "pi", None, None,),
            Some(SelfUpdateCommand {
                command: "pnpm".to_string(),
                args: vec![
                    "install".to_string(),
                    "-g".to_string(),
                    "--ignore-scripts".to_string(),
                    "pi".to_string(),
                ],
                display: "pnpm install -g --ignore-scripts pi".to_string(),
                steps: Vec::new(),
            })
        );

        assert_eq!(
            self_update_command_for_method(
                InstallMethod::Npm,
                "pi",
                "@scope/pi-next",
                Some(vec!["corepack npm".to_string(), "--registry".to_string(), "https://registry.example".to_string()]),
                None,
            ),
            Some(SelfUpdateCommand {
                command: "corepack npm".to_string(),
                args: vec![
                    "--registry".to_string(),
                    "https://registry.example".to_string(),
                    "install".to_string(),
                    "-g".to_string(),
                    "--ignore-scripts".to_string(),
                    "@scope/pi-next".to_string(),
                ],
                display: "\"corepack npm\" --registry https://registry.example uninstall -g pi && \"corepack npm\" --registry https://registry.example install -g --ignore-scripts @scope/pi-next".to_string(),
                steps: vec![
                    SelfUpdateCommandStep {
                        command: "corepack npm".to_string(),
                        args: vec![
                            "--registry".to_string(),
                            "https://registry.example".to_string(),
                            "uninstall".to_string(),
                            "-g".to_string(),
                            "pi".to_string(),
                        ],
                        display: "\"corepack npm\" --registry https://registry.example uninstall -g pi".to_string(),
                    },
                    SelfUpdateCommandStep {
                        command: "corepack npm".to_string(),
                        args: vec![
                            "--registry".to_string(),
                            "https://registry.example".to_string(),
                            "install".to_string(),
                            "-g".to_string(),
                            "--ignore-scripts".to_string(),
                            "@scope/pi-next".to_string(),
                        ],
                        display: "\"corepack npm\" --registry https://registry.example install -g --ignore-scripts @scope/pi-next".to_string(),
                    },
                ],
            })
        );

        assert_eq!(
            self_update_command_for_method(InstallMethod::Yarn, "pi", "next pi", None, None)
                .map(|command| command.display),
            Some(
                "yarn global remove pi && yarn global add --ignore-scripts \"next pi\"".to_string()
            )
        );
        assert_eq!(
            self_update_command_for_method(InstallMethod::Bun, "pi", "pi", None, None)
                .map(|command| command.display),
            Some("bun install -g --ignore-scripts pi".to_string())
        );
        assert_eq!(
            self_update_command_for_method(InstallMethod::Unknown, "pi", "pi", None, None),
            None
        );
        assert_eq!(
            self_update_command_for_method(InstallMethod::BunBinary, "pi", "pi", None, None),
            None
        );
    }

    #[test]
    fn plans_windows_npm_self_update_prepare_actions_like_pi_package_cli() {
        assert_eq!(
            self_update_prepare_actions("win32", InstallMethod::Npm, "C:/npm/node_modules/pi"),
            vec![SelfUpdatePrepareAction::WindowsNpmQuarantine {
                package_dir: "C:/npm/node_modules/pi".to_string(),
            }]
        );
        assert!(
            self_update_prepare_actions("darwin", InstallMethod::Npm, "/node_modules/pi")
                .is_empty()
        );
        assert!(self_update_prepare_actions("win32", InstallMethod::Pnpm, "C:/pnpm/pi").is_empty());
    }

    #[test]
    fn runs_windows_npm_self_update_prepare_actions_like_pi_package_cli() {
        #[derive(Default)]
        struct Runner {
            calls: Vec<String>,
        }

        impl SelfUpdatePrepareRunner for Runner {
            fn cleanup_windows_quarantine(&mut self, package_dir: &str) -> Result<(), String> {
                self.calls.push(format!("cleanup:{package_dir}"));
                Ok(())
            }

            fn quarantine_windows_native_dependencies(
                &mut self,
                package_dir: &str,
            ) -> Result<(), String> {
                self.calls.push(format!("quarantine:{package_dir}"));
                Ok(())
            }
        }

        let mut runner = Runner::default();
        run_self_update_prepare_actions(
            &mut runner,
            &[SelfUpdatePrepareAction::WindowsNpmQuarantine {
                package_dir: "C:/npm/node_modules/pi".to_string(),
            }],
        )
        .expect("prepare should run");

        assert_eq!(
            runner.calls,
            vec![
                "cleanup:C:/npm/node_modules/pi".to_string(),
                "quarantine:C:/npm/node_modules/pi".to_string(),
            ]
        );
    }

    #[test]
    fn system_prepare_runner_quarantines_native_dependencies_like_pi_package_cli() {
        let package_dir = temp_dir("self-update-prepare")
            .join("node_modules")
            .join("pi");
        let native_file = package_dir.join("native").join("addon.node");
        std::fs::create_dir_all(native_file.parent().expect("native parent"))
            .expect("native dir should be created");
        std::fs::write(&native_file, "binary").expect("native file should be written");

        let mut runner = SystemSelfUpdatePrepareRunner::new(vec![native_file.clone()], "run-1");
        run_self_update_prepare_actions(
            &mut runner,
            &[SelfUpdatePrepareAction::WindowsNpmQuarantine {
                package_dir: package_dir.to_string_lossy().to_string(),
            }],
        )
        .expect("system prepare should run");

        let quarantine_file = package_dir
            .parent()
            .expect("node_modules")
            .join(".pi-native-quarantine")
            .join("run-1")
            .join("native")
            .join("addon.node");
        assert_eq!(
            std::fs::read_to_string(&quarantine_file).expect("quarantine file should read"),
            "binary"
        );
        assert_eq!(
            std::fs::read_to_string(&native_file).expect("native file should be copied back"),
            "binary"
        );
    }

    #[test]
    fn detects_install_method_like_pi_config() {
        assert_eq!(
            detect_install_method_from_context(true, false, "/app/dist", "/usr/bin/node"),
            InstallMethod::BunBinary
        );
        assert_eq!(
            detect_install_method_from_context(
                false,
                false,
                "/opt/pnpm/global/pi",
                "/usr/bin/node"
            ),
            InstallMethod::Pnpm
        );
        assert_eq!(
            detect_install_method_from_context(
                false,
                false,
                "C:\\Users\\me\\.yarn\\pi",
                "node.exe"
            ),
            InstallMethod::Yarn
        );
        assert_eq!(
            detect_install_method_from_context(false, true, "/app/dist", "/usr/bin/node"),
            InstallMethod::Bun
        );
        assert_eq!(
            detect_install_method_from_context(
                false,
                false,
                "/home/me/.bun/install/global/node_modules/pi",
                "/usr/bin/node",
            ),
            InstallMethod::Bun
        );
        assert_eq!(
            detect_install_method_from_context(
                false,
                false,
                "/usr/local/lib/node_modules/pi",
                "/usr/bin/node",
            ),
            InstallMethod::Npm
        );
        assert_eq!(
            detect_install_method_from_context(false, false, "/opt/pi", "/usr/bin/node"),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn infers_npm_global_install_like_pi_config() {
        assert_eq!(
            infer_npm_global_install_from_package_dir("/usr/local/lib/node_modules/pi", false),
            Some(NpmGlobalInstall {
                root: "/usr/local/lib/node_modules".to_string(),
                prefix: "/usr/local".to_string(),
            })
        );
        assert_eq!(
            infer_npm_global_install_from_package_dir(
                "/usr/local/lib/node_modules/@scope/pi",
                false
            ),
            Some(NpmGlobalInstall {
                root: "/usr/local/lib/node_modules".to_string(),
                prefix: "/usr/local".to_string(),
            })
        );
        assert_eq!(
            infer_npm_global_install_from_package_dir(
                "C:\\Users\\me\\AppData\\Roaming\\npm\\node_modules\\pi",
                true
            ),
            None
        );
        assert_eq!(
            infer_npm_global_install_from_package_dir("/workspace/project/node_modules/pi", false),
            None
        );
    }

    #[test]
    fn plans_self_update_command_from_context_like_pi_config() {
        assert_eq!(
            self_update_command_from_context(
                InstallMethod::Npm,
                "pi",
                "pi",
                None,
                &["/usr/local/lib/node_modules/pi".to_string()],
                &[],
                true,
                false,
            )
            .map(|command| command.display),
            Some("npm --prefix /usr/local install -g --ignore-scripts pi".to_string())
        );
        assert_eq!(
            self_update_command_from_context(
                InstallMethod::Npm,
                "pi",
                "pi",
                Some(vec![
                    "npm".to_string(),
                    "--registry".to_string(),
                    "https://registry.example".to_string()
                ]),
                &["/opt/npm/node_modules/pi".to_string()],
                &["/opt/npm/node_modules".to_string()],
                true,
                false,
            )
            .map(|command| command.display),
            Some(
                "npm --registry https://registry.example install -g --ignore-scripts pi"
                    .to_string()
            )
        );
        assert_eq!(
            self_update_command_from_context(
                InstallMethod::Npm,
                "pi",
                "pi",
                Some(Vec::new()),
                &["/usr/local/lib/node_modules/pi".to_string()],
                &[],
                true,
                false,
            )
            .map(|command| command.display),
            Some("npm --prefix /usr/local install -g --ignore-scripts pi".to_string())
        );
        assert_eq!(
            self_update_command_from_context(
                InstallMethod::Npm,
                "pi",
                "pi",
                None,
                &["/usr/local/lib/node_modules/pi".to_string()],
                &[],
                false,
                false,
            ),
            None
        );
        assert_eq!(
            self_update_command_from_context(
                InstallMethod::Pnpm,
                "pi",
                "pi",
                None,
                &["/checkout/pi".to_string()],
                &["/usr/local/pnpm/global".to_string()],
                true,
                false,
            ),
            None
        );
    }

    #[test]
    fn plans_global_package_roots_probe_like_pi_config() {
        assert_eq!(
            global_package_roots_plan(InstallMethod::Npm, None, "/home/me", None),
            GlobalPackageRootsPlan {
                command: Some(SelfUpdateCommandStep {
                    command: "npm".to_string(),
                    args: vec!["root".to_string(), "-g".to_string()],
                    display: "npm root -g".to_string(),
                }),
                fallback_roots: Vec::new(),
                command_output_transform: GlobalPackageRootOutputTransform::RootOnly,
                require_command_success: false,
            }
        );
        assert_eq!(
            global_package_roots_plan(
                InstallMethod::Npm,
                Some(vec![
                    "npm".to_string(),
                    "--registry".to_string(),
                    "https://registry.example".to_string()
                ]),
                "/home/me",
                Some(NpmGlobalInstall {
                    root: "/usr/local/lib/node_modules".to_string(),
                    prefix: "/usr/local".to_string(),
                }),
            ),
            GlobalPackageRootsPlan {
                command: Some(SelfUpdateCommandStep {
                    command: "npm".to_string(),
                    args: vec![
                        "--registry".to_string(),
                        "https://registry.example".to_string(),
                        "root".to_string(),
                        "-g".to_string(),
                    ],
                    display: "npm --registry https://registry.example root -g".to_string(),
                }),
                fallback_roots: Vec::new(),
                command_output_transform: GlobalPackageRootOutputTransform::RootOnly,
                require_command_success: true,
            }
        );
        assert_eq!(
            global_package_roots_plan(
                InstallMethod::Npm,
                Some(vec!["bun".to_string()]),
                "/home/me",
                None,
            ),
            GlobalPackageRootsPlan {
                command: Some(SelfUpdateCommandStep {
                    command: "bun".to_string(),
                    args: vec!["pm".to_string(), "bin".to_string(), "-g".to_string()],
                    display: "bun pm bin -g".to_string(),
                }),
                fallback_roots: vec!["/home/me/.bun/install/global/node_modules".to_string()],
                command_output_transform: GlobalPackageRootOutputTransform::BunBinInstallRoot,
                require_command_success: true,
            }
        );
        assert_eq!(
            global_package_roots_plan(InstallMethod::Yarn, None, "/home/me", None),
            GlobalPackageRootsPlan {
                command: Some(SelfUpdateCommandStep {
                    command: "yarn".to_string(),
                    args: vec!["global".to_string(), "dir".to_string()],
                    display: "yarn global dir".to_string(),
                }),
                fallback_roots: Vec::new(),
                command_output_transform: GlobalPackageRootOutputTransform::RootAndNodeModules,
                require_command_success: false,
            }
        );
        assert_eq!(
            global_package_roots_plan(InstallMethod::Pnpm, None, "/home/me", None),
            GlobalPackageRootsPlan {
                command: Some(SelfUpdateCommandStep {
                    command: "pnpm".to_string(),
                    args: vec!["root".to_string(), "-g".to_string()],
                    display: "pnpm root -g".to_string(),
                }),
                fallback_roots: Vec::new(),
                command_output_transform: GlobalPackageRootOutputTransform::RootAndParent,
                require_command_success: false,
            }
        );
    }

    #[test]
    fn resolves_global_package_roots_from_probe_output_like_pi_config() {
        let npm_plan = global_package_roots_plan(
            InstallMethod::Npm,
            None,
            "/home/me",
            Some(NpmGlobalInstall {
                root: "/usr/local/lib/node_modules".to_string(),
                prefix: "/usr/local".to_string(),
            }),
        );
        assert_eq!(
            global_package_roots_from_plan(&npm_plan, Some("/opt/npm/node_modules")),
            vec![
                "/usr/local/lib/node_modules".to_string(),
                "/opt/npm/node_modules".to_string(),
            ]
        );

        let pnpm_plan = global_package_roots_plan(InstallMethod::Pnpm, None, "/home/me", None);
        assert_eq!(
            global_package_roots_from_plan(&pnpm_plan, Some("/pnpm/global/5/node_modules")),
            vec![
                "/pnpm/global/5/node_modules".to_string(),
                "/pnpm/global/5".to_string(),
            ]
        );

        let yarn_plan = global_package_roots_plan(InstallMethod::Yarn, None, "/home/me", None);
        assert_eq!(
            global_package_roots_from_plan(&yarn_plan, Some("/yarn/global")),
            vec![
                "/yarn/global".to_string(),
                "/yarn/global/node_modules".to_string(),
            ]
        );

        let bun_plan = global_package_roots_plan(InstallMethod::Bun, None, "/home/me", None);
        assert_eq!(
            global_package_roots_from_plan(&bun_plan, Some("/home/me/.bun/bin")),
            vec![
                "/home/me/.bun/install/global/node_modules".to_string(),
                "/home/me/.bun/install/global/node_modules".to_string(),
            ]
        );
    }

    #[test]
    fn checks_self_update_path_writability_like_pi_config() {
        let root = temp_dir("self-update-writable");
        let package_dir = root.join("node_modules").join("pi");
        std::fs::create_dir_all(&package_dir).expect("package dir should be created");
        assert!(self_update_path_is_writable(&package_dir));

        assert!(!self_update_path_is_writable(
            root.join("missing").join("pi")
        ));
    }

    #[test]
    fn finds_entrypoint_package_dir_like_pi_config() {
        let root = temp_dir("entrypoint-package");
        let package_dir = root.join("packages").join("pi");
        let bin_dir = package_dir.join("dist").join("bin");
        std::fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        std::fs::write(package_dir.join("package.json"), "{}").expect("package json should write");

        assert_eq!(
            entrypoint_package_dir(bin_dir.join("pi.js")),
            Some(package_dir)
        );
        assert_eq!(entrypoint_package_dir(root.join("loose.js")), None);
    }

    #[test]
    fn builds_path_comparison_candidates_like_pi_config() {
        let root = temp_dir("path-candidates");
        let package_dir = root.join("real").join("pi");
        std::fs::create_dir_all(&package_dir).expect("package dir should be created");
        let link_dir = root.join("link-pi");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&package_dir, &link_dir).expect("symlink should be created");

        let candidates =
            path_comparison_candidates(if cfg!(unix) { &link_dir } else { &package_dir }, false);
        let real_package_dir = package_dir
            .canonicalize()
            .expect("package dir should canonicalize")
            .to_string_lossy()
            .to_string();
        assert!(candidates.contains(&real_package_dir));
        assert!(path_comparison_candidates(root.join("missing"), false).is_empty());
    }

    #[test]
    fn builds_self_update_package_dir_candidates_like_pi_config() {
        let root = temp_dir("self-update-package-candidates");
        let package_dir = root.join("node_modules").join("pi");
        let entrypoint_package = root.join("wrapper");
        std::fs::create_dir_all(&package_dir).expect("package dir should be created");
        std::fs::create_dir_all(entrypoint_package.join("bin")).expect("bin dir should be created");
        std::fs::write(entrypoint_package.join("package.json"), "{}")
            .expect("package json should write");

        let candidates = self_update_package_dir_candidates(
            &package_dir,
            Some(entrypoint_package.join("bin").join("pi.js")),
            false,
        );

        assert!(candidates.contains(
            &package_dir
                .canonicalize()
                .expect("package dir canonicalize")
                .to_string_lossy()
                .to_string()
        ));
        assert!(candidates.contains(
            &entrypoint_package
                .canonicalize()
                .expect("entrypoint package canonicalize")
                .to_string_lossy()
                .to_string()
        ));
    }

    #[test]
    fn formats_self_update_instructions_like_pi_config() {
        assert_eq!(
            self_update_unavailable_instruction(
                InstallMethod::BunBinary,
                "pi",
                "pi",
                None,
                None,
                false,
                false,
            ),
            "Download from: https://github.com/earendil-works/pi-mono/releases/latest"
        );
        assert_eq!(
            self_update_unavailable_instruction(
                InstallMethod::Npm,
                "pi",
                "pi",
                None,
                None,
                true,
                false,
            ),
            "This installation is managed by a global npm install, but the install path is not writable. Update it yourself with: npm install -g --ignore-scripts pi"
        );
        assert_eq!(
            self_update_unavailable_instruction(
                InstallMethod::Pnpm,
                "pi",
                "pi",
                None,
                None,
                false,
                true,
            ),
            "This installation is not managed by a global pnpm install. Update it with the package manager, wrapper, or source checkout that provides it."
        );
        assert_eq!(
            self_update_unavailable_instruction(
                InstallMethod::Unknown,
                "pi",
                "@scope/pi-next",
                None,
                None,
                false,
                false,
            ),
            "Update @scope/pi-next using the package manager, wrapper, or source checkout that provides this installation."
        );
        assert_eq!(
            update_instruction(InstallMethod::Yarn, "pi", None, None),
            "Run: yarn global add --ignore-scripts pi"
        );
        assert_eq!(
            update_instruction(InstallMethod::BunBinary, "pi", None, None),
            "Download from: https://github.com/earendil-works/pi-mono/releases/latest"
        );
    }

    #[test]
    fn detects_global_package_manager_ownership_like_pi_config() {
        assert!(is_managed_by_global_package_manager(
            &[
                "/usr/local/lib/node_modules/pi".to_string(),
                "/private/var/folders/link/pi".to_string(),
            ],
            &["/usr/local/lib/node_modules".to_string()],
            false,
        ));
        assert!(is_managed_by_global_package_manager(
            &["C:\\Users\\ME\\AppData\\Roaming\\npm\\node_modules\\pi".to_string()],
            &["c:/users/me/appdata/roaming/npm/node_modules".to_string()],
            true,
        ));
        assert!(!is_managed_by_global_package_manager(
            &["/usr/local/lib/node_modules-other/pi".to_string()],
            &["/usr/local/lib/node_modules".to_string()],
            false,
        ));
        assert!(!is_managed_by_global_package_manager(
            &["/checkout/pi".to_string()],
            &[],
            false,
        ));
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = std::process::id();
        let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-cli-{label}-{id}-{count}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
