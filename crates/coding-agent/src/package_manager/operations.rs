use super::git_update::git_update_target;
use super::source::{
    git_install_path, git_install_root, git_storage_root, parse_source, temp_package_dir,
};
use super::types::{
    NpmCommandConfig, NpmSource, PackageCommandStep, PackageOperationPlan, ParsedSource,
    ProgressAction, SourceScope,
};
use crate::settings_manager::CONFIG_DIR_NAME;
use crate::utils::git::GitSource;
use std::path::Path;

pub fn plan_install(
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
        ParsedSource::Npm(npm) => install_npm_steps(agent_dir, cwd, npm, scope, &npm_command),
        ParsedSource::Git(git) => install_git_steps(
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
        action: ProgressAction::Install,
        source: source.to_string(),
        steps,
    }
}

pub fn plan_remove(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
    npm_command: Option<NpmCommandConfig>,
) -> PackageOperationPlan {
    let npm_command = npm_command.unwrap_or_default();
    let parsed = parse_source(source);
    let steps = match &parsed {
        ParsedSource::Npm(npm) => uninstall_npm_steps(agent_dir, cwd, npm, scope, &npm_command),
        ParsedSource::Git(git) => remove_git_steps(agent_dir, cwd, git, scope),
        ParsedSource::Local(_) => Vec::new(),
    };
    PackageOperationPlan {
        action: ProgressAction::Remove,
        source: source.to_string(),
        steps,
    }
}

pub fn progress_events_for_plan(plan: &PackageOperationPlan) -> Vec<super::types::ProgressEvent> {
    vec![
        super::types::ProgressEvent {
            kind: super::types::ProgressEventKind::Start,
            action: plan.action,
            source: plan.source.clone(),
            message: Some(format!("{} {}...", progress_verb(plan.action), plan.source)),
        },
        super::types::ProgressEvent {
            kind: super::types::ProgressEventKind::Complete,
            action: plan.action,
            source: plan.source.clone(),
            message: None,
        },
    ]
}

pub(super) fn plan_npm_batch_update(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    specs: &[String],
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
    source_label: impl Into<String>,
) -> PackageOperationPlan {
    let install_root = display_path(npm_install_root(agent_dir, cwd, scope));
    let steps = if specs.is_empty() {
        Vec::new()
    } else {
        vec![
            ensure_npm_project_step(&install_root),
            npm_step(
                npm_command,
                npm_install_args(specs, &install_root, npm_command),
                None,
            ),
        ]
    };
    PackageOperationPlan {
        action: ProgressAction::Update,
        source: source_label.into(),
        steps,
    }
}

fn install_npm_steps(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &NpmSource,
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
) -> Vec<PackageCommandStep> {
    let install_root = display_path(npm_install_root(agent_dir, cwd, scope));
    vec![
        ensure_npm_project_step(&install_root),
        npm_step(
            npm_command,
            npm_install_args(&[source.spec.clone()], &install_root, npm_command),
            None,
        ),
    ]
}

fn uninstall_npm_steps(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &NpmSource,
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
) -> Vec<PackageCommandStep> {
    let install_root = display_path(npm_install_root(agent_dir, cwd, scope));
    if !Path::new(&install_root).exists() {
        return Vec::new();
    }
    let args = if package_manager_name(npm_command) == "bun" {
        vec![
            "uninstall".to_string(),
            source.name.clone(),
            "--cwd".to_string(),
            install_root,
        ]
    } else {
        vec![
            "uninstall".to_string(),
            source.name.clone(),
            "--prefix".to_string(),
            install_root,
        ]
    };
    vec![npm_step(npm_command, args, None)]
}

fn install_git_steps(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &GitSource,
    scope: SourceScope,
    npm_command: &NpmCommandConfig,
    npm_command_configured: bool,
) -> Vec<PackageCommandStep> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let target_dir = display_path(git_install_path(agent_dir, cwd, source, scope));
    if Path::new(&target_dir).exists() {
        return reconcile_existing_git_steps(&target_dir, source);
    }
    let target_parent = Path::new(&target_dir)
        .parent()
        .map(display_path)
        .unwrap_or_default();
    let mut steps = Vec::new();
    if scope != SourceScope::Temporary {
        steps.push(PackageCommandStep {
            command: "ensure_git_root".to_string(),
            args: vec![display_path(git_install_root(
                agent_dir, cwd, source, scope,
            ))],
            cwd: None,
        });
    }
    steps.extend([
        PackageCommandStep {
            command: "ensure_dir".to_string(),
            args: vec![target_parent],
            cwd: None,
        },
        PackageCommandStep {
            command: "git".to_string(),
            args: vec!["clone".to_string(), source.repo.clone(), target_dir.clone()],
            cwd: None,
        },
    ]);
    if let Some(reference) = &source.reference {
        steps.push(PackageCommandStep {
            command: "git".to_string(),
            args: vec!["checkout".to_string(), reference.clone()],
            cwd: Some(target_dir.clone()),
        });
    }
    steps.push(conditional_package_json_step(
        npm_command,
        git_dependency_install_args(npm_command_configured),
        Some(target_dir),
    ));
    steps
}

fn reconcile_existing_git_steps(target_dir: &str, source: &GitSource) -> Vec<PackageCommandStep> {
    let target = if let Some(reference) = &source.reference {
        super::git_update::GitUpdateTarget {
            reset_ref: "FETCH_HEAD^{commit}".to_string(),
            fetch_args: vec![
                "fetch".to_string(),
                "origin".to_string(),
                reference.to_string(),
            ],
        }
    } else {
        git_update_target(Path::new(target_dir), source)
    };
    vec![PackageCommandStep {
        command: "git_ensure_ref".to_string(),
        args: std::iter::once(target.reset_ref)
            .chain(std::iter::once("--".to_string()))
            .chain(target.fetch_args)
            .collect(),
        cwd: Some(target_dir.to_string()),
    }]
}

fn remove_git_steps(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &GitSource,
    scope: SourceScope,
) -> Vec<PackageCommandStep> {
    let mut args = vec![display_path(git_install_path(
        &agent_dir, &cwd, source, scope,
    ))];
    if let Some(root) = git_storage_root(agent_dir, cwd, scope) {
        args.push(display_path(root));
    }
    vec![PackageCommandStep {
        command: "remove_dir".to_string(),
        args,
        cwd: None,
    }]
}

fn npm_step(
    npm_command: &NpmCommandConfig,
    args: Vec<String>,
    cwd: Option<String>,
) -> PackageCommandStep {
    let mut full_args = npm_command.args.clone();
    full_args.extend(args);
    PackageCommandStep {
        command: npm_command.command.clone(),
        args: full_args,
        cwd,
    }
}

pub(super) fn conditional_package_json_step(
    npm_command: &NpmCommandConfig,
    args: Vec<String>,
    cwd: Option<String>,
) -> PackageCommandStep {
    let mut full_args = vec![npm_command.command.clone()];
    full_args.extend(npm_command.args.clone());
    full_args.extend(args);
    PackageCommandStep {
        command: "run_if_package_json".to_string(),
        args: full_args,
        cwd,
    }
}

fn ensure_npm_project_step(install_root: &str) -> PackageCommandStep {
    PackageCommandStep {
        command: "ensure_npm_project".to_string(),
        args: vec![install_root.to_string()],
        cwd: None,
    }
}

fn npm_install_root(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    scope: SourceScope,
) -> std::path::PathBuf {
    match scope {
        SourceScope::Temporary => temp_package_dir("npm"),
        SourceScope::Project => cwd.as_ref().join(CONFIG_DIR_NAME).join("npm"),
        SourceScope::User => agent_dir.as_ref().join("npm"),
    }
}

fn npm_install_args(
    specs: &[String],
    install_root: &str,
    npm_command: &NpmCommandConfig,
) -> Vec<String> {
    match package_manager_name(npm_command).as_str() {
        "bun" => vec!["install".to_string()]
            .into_iter()
            .chain(specs.iter().cloned())
            .chain([
                "--cwd".to_string(),
                install_root.to_string(),
                "--omit=peer".to_string(),
            ])
            .collect(),
        "pnpm" => vec!["install".to_string()]
            .into_iter()
            .chain(specs.iter().cloned())
            .chain([
                "--prefix".to_string(),
                install_root.to_string(),
                "--config.auto-install-peers=false".to_string(),
                "--config.strict-peer-dependencies=false".to_string(),
                "--config.strict-dep-builds=false".to_string(),
            ])
            .collect(),
        _ => vec!["install".to_string()]
            .into_iter()
            .chain(specs.iter().cloned())
            .chain([
                "--prefix".to_string(),
                install_root.to_string(),
                "--legacy-peer-deps".to_string(),
            ])
            .collect(),
    }
}

fn git_dependency_install_args(npm_command_configured: bool) -> Vec<String> {
    if npm_command_configured {
        vec!["install".to_string()]
    } else {
        vec!["install".to_string(), "--omit=dev".to_string()]
    }
}

pub(super) fn package_manager_name(npm_command: &NpmCommandConfig) -> String {
    let parts = std::iter::once(npm_command.command.as_str())
        .chain(npm_command.args.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let command = if let Some(separator_index) = parts.iter().rposition(|part| *part == "--") {
        parts.get(separator_index + 1).copied().unwrap_or_default()
    } else {
        npm_command.command.as_str()
    };
    let command_name = std::path::Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    strip_windows_command_extension(command_name).to_string()
}

fn strip_windows_command_extension(command: &str) -> &str {
    let lower = command.to_ascii_lowercase();
    if lower.ends_with(".cmd") || lower.ends_with(".exe") {
        &command[..command.len() - 4]
    } else {
        command
    }
}

pub fn progress_verb(action: ProgressAction) -> &'static str {
    match action {
        ProgressAction::Install => "Installing",
        ProgressAction::Remove => "Removing",
        ProgressAction::Update => "Updating",
        ProgressAction::Clone => "Cloning",
        ProgressAction::Pull => "Refreshing",
    }
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_npm_install_args_for_default_npm() {
        let plan = plan_install(
            "/agent",
            "/work",
            "npm:@scope/pkg",
            SourceScope::Project,
            None,
        );
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].command, "ensure_npm_project");
        assert_eq!(plan.steps[0].args, vec!["/work/.pm-agent/npm"]);
        assert_eq!(plan.steps[1].command, "npm");
        assert_eq!(
            plan.steps[1].args,
            vec![
                "install",
                "@scope/pkg",
                "--prefix",
                "/work/.pm-agent/npm",
                "--legacy-peer-deps"
            ]
        );
    }

    #[test]
    fn plans_pnpm_install_with_peer_flags() {
        let plan = plan_install(
            "/agent",
            "/work",
            "npm:pkg",
            SourceScope::User,
            Some(NpmCommandConfig {
                command: "pnpm".to_string(),
                args: Vec::new(),
            }),
        );
        assert!(plan.steps[1]
            .args
            .contains(&"--config.auto-install-peers=false".to_string()));
    }

    #[test]
    fn temporary_npm_install_plan_uses_pi_extensions_hashed_root_like_pi() {
        let plan = plan_install("/agent", "/work", "npm:pkg", SourceScope::Temporary, None);

        assert!(std::path::Path::new(&plan.steps[0].args[0])
            .ends_with(std::path::Path::new("pi-extensions/npm/f35b2129")));
        assert!(plan.steps[1].args.iter().any(|arg| {
            std::path::Path::new(arg).ends_with(std::path::Path::new("pi-extensions/npm/f35b2129"))
        }));
    }

    #[test]
    fn temporary_git_remove_does_not_prune_parent_dirs_like_pi() {
        let plan = plan_remove(
            "/agent",
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::Temporary,
            None,
        );

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "remove_dir");
        assert_eq!(plan.steps[0].args.len(), 1);
        assert!(
            std::path::Path::new(&plan.steps[0].args[0]).ends_with(std::path::Path::new(
                "pi-extensions/git-github.com/338a1076/user/repo"
            ))
        );
    }

    #[test]
    fn temporary_git_clone_does_not_ensure_git_root_like_pi() {
        let plan = plan_install(
            "/agent",
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::Temporary,
            None,
        );

        assert_eq!(plan.steps[0].command, "ensure_dir");
        assert!(
            std::path::Path::new(&plan.steps[0].args[0]).ends_with(std::path::Path::new(
                "pi-extensions/git-github.com/338a1076/user"
            ))
        );
        assert_eq!(plan.steps[1].command, "git");
        assert_eq!(plan.steps[1].args[0], "clone");
        assert_eq!(plan.steps[2].command, "run_if_package_json");
        assert!(!plan
            .steps
            .iter()
            .any(|step| step.command == "ensure_git_root"));
    }

    #[test]
    fn package_manager_name_strips_windows_extensions_case_insensitively_like_pi() {
        assert_eq!(
            package_manager_name(&NpmCommandConfig {
                command: "PNPM.CMD".to_string(),
                args: Vec::new(),
            }),
            "PNPM"
        );
        assert_eq!(
            package_manager_name(&NpmCommandConfig {
                command: "/usr/local/bin/bun.EXE".to_string(),
                args: Vec::new(),
            }),
            "bun"
        );
    }

    #[test]
    fn package_manager_name_uses_command_after_separator_and_strips_extension_like_pi() {
        assert_eq!(
            package_manager_name(&NpmCommandConfig {
                command: "corepack".to_string(),
                args: vec!["--".to_string(), "pnpm.cmd".to_string()],
            }),
            "pnpm"
        );
        assert_eq!(
            package_manager_name(&NpmCommandConfig {
                command: "corepack".to_string(),
                args: vec![
                    "prepare".to_string(),
                    "pnpm@latest".to_string(),
                    "--".to_string(),
                    "/opt/bin/bun.EXE".to_string(),
                ],
            }),
            "bun"
        );
    }

    #[test]
    fn package_manager_name_returns_empty_when_separator_has_no_following_command_like_pi() {
        assert_eq!(
            package_manager_name(&NpmCommandConfig {
                command: "corepack".to_string(),
                args: vec![
                    "prepare".to_string(),
                    "pnpm@latest".to_string(),
                    "--".to_string()
                ],
            }),
            ""
        );
    }

    #[test]
    fn plans_git_clone_checkout_and_dependency_install() {
        let plan = plan_install(
            "/agent",
            "/work",
            "git:https://github.com/user/repo@main",
            SourceScope::User,
            None,
        );
        assert_eq!(plan.steps[0].command, "ensure_git_root");
        assert_eq!(plan.steps[0].args, vec!["/agent/git/github.com"]);
        assert_eq!(plan.steps[1].command, "ensure_dir");
        assert_eq!(plan.steps[1].args, vec!["/agent/git/github.com/user"]);
        assert_eq!(plan.steps[2].command, "git");
        assert_eq!(plan.steps[2].args[0], "clone");
        assert_eq!(plan.steps[3].args, vec!["checkout", "main"]);
        assert_eq!(plan.steps[4].command, "run_if_package_json");
        assert_eq!(plan.steps[4].args, vec!["npm", "install", "--omit=dev"]);
    }

    #[test]
    fn plans_git_remove_prunes_to_git_root_like_pi() {
        let plan = plan_remove(
            "/agent",
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::User,
            None,
        );

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "remove_dir");
        assert_eq!(
            plan.steps[0].args,
            vec!["/agent/git/github.com/user/repo", "/agent/git"]
        );
    }

    #[test]
    fn plans_git_clone_uses_plain_install_when_npm_command_is_configured_like_pi() {
        let agent_dir = temp_dir();
        let plan = plan_install(
            &agent_dir,
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::User,
            Some(NpmCommandConfig {
                command: "npm".to_string(),
                args: Vec::new(),
            }),
        );

        let install_step = plan.steps.last().expect("install step should exist");
        assert_eq!(install_step.command, "run_if_package_json");
        assert_eq!(install_step.args, vec!["npm", "install"]);
    }

    #[test]
    fn plans_existing_git_checkout_reconcile_for_pinned_install_like_pi() {
        let agent_dir = temp_dir();
        let target = agent_dir
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        std::fs::create_dir_all(&target).expect("target dir should exist");
        std::fs::write(target.join("package.json"), "{}").expect("package json should write");

        let plan = plan_install(
            &agent_dir,
            "/work",
            "git:https://github.com/user/repo@v2",
            SourceScope::User,
            None,
        );
        let target_path = display_path(&target);

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "git_ensure_ref");
        assert_eq!(
            plan.steps[0].args,
            vec!["FETCH_HEAD^{commit}", "--", "fetch", "origin", "v2"]
        );
        assert_eq!(plan.steps[0].cwd.as_deref(), Some(target_path.as_str()));
    }

    #[test]
    fn plans_existing_git_checkout_skips_dependency_install_when_npm_command_is_configured_like_pi()
    {
        let agent_dir = temp_dir();
        let target = agent_dir
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        std::fs::create_dir_all(&target).expect("target dir should exist");

        let plan = plan_install(
            &agent_dir,
            "/work",
            "git:https://github.com/user/repo@v2",
            SourceScope::User,
            Some(NpmCommandConfig {
                command: "npm".to_string(),
                args: Vec::new(),
            }),
        );

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "git_ensure_ref");
        assert_eq!(
            plan.steps[0].args,
            vec!["FETCH_HEAD^{commit}", "--", "fetch", "origin", "v2"]
        );
    }

    #[test]
    fn plans_existing_git_checkout_reconcile_from_upstream_for_unpinned_install_like_pi() {
        let agent_dir = temp_dir();
        let target = agent_dir
            .join("git")
            .join("github.com")
            .join("user")
            .join("repo");
        let git_dir = target.join(".git");
        std::fs::create_dir_all(&git_dir).expect("git dir should exist");
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("head should write");
        std::fs::write(
            git_dir.join("config"),
            r#"[branch "main"]
    remote = origin
    merge = refs/heads/main
"#,
        )
        .expect("config should write");

        let plan = plan_install(
            &agent_dir,
            "/work",
            "git:https://github.com/user/repo",
            SourceScope::User,
            None,
        );
        let target_path = display_path(&target);

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "git_ensure_ref");
        assert_eq!(
            plan.steps[0].args,
            vec![
                "@{upstream}^{commit}",
                "--",
                "fetch",
                "--prune",
                "--no-tags",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main"
            ]
        );
        assert_eq!(plan.steps[0].cwd.as_deref(), Some(target_path.as_str()));
    }

    fn temp_dir() -> std::path::PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-package-operations-test-{id}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
