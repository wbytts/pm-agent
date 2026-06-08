mod context;
mod extension_resources;
mod paths;
mod prompts;
mod skills;
mod themes;

use crate::diagnostics::ResourceDiagnostic;
use crate::extensions::{
    create_extension_runtime, discover_and_load_extensions, load_extensions,
    DiscoveredExtensionResources, ExtensionFactory, ExtensionRuntime, LoadExtensionsResult,
};
use crate::package_manager::{
    LocalPackageManager, PackageCommandExecutor, PackageCommandRunner, PathMetadata, ResolvedPaths,
    ResolvedResource, SourceOrigin, SourceScope,
};
use crate::settings_manager::{SettingsManager, SettingsStorage};
use crate::slash_commands::{resource_slash_commands, SlashCommandInfo};
use crate::source_info::create_source_info;
use crate::utils::paths::is_local_path;
use agent::harness::{PromptTemplate, Skill};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

use context::{discover_first_file, load_project_context_files, resolve_prompt_input};
use extension_resources::discovered_resources_to_paths;
use paths::{display_path, enabled_paths, expand_home, merge_paths, normalize_path};
use prompts::load_prompts;
use skills::load_skills;
use themes::load_themes;

pub use context::AgentsFile;
pub use themes::Theme;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceExtensionPaths {
    pub skill_paths: Vec<ResourcePath>,
    pub prompt_paths: Vec<ResourcePath>,
    pub theme_paths: Vec<ResourcePath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePath {
    pub path: String,
    pub metadata: PathMetadata,
}

pub struct DefaultResourceLoader<S: SettingsStorage> {
    cwd: PathBuf,
    agent_dir: PathBuf,
    user_agents_dir: Option<PathBuf>,
    settings_manager: SettingsManager<S>,
    runtime: ExtensionRuntime,
    package_runner: Box<dyn PackageCommandRunner>,
    additional_extension_paths: Vec<String>,
    additional_skill_paths: Vec<String>,
    additional_prompt_paths: Vec<String>,
    additional_theme_paths: Vec<String>,
    extension_factories: Vec<(String, Box<dyn ExtensionFactory>)>,
    no_extensions: bool,
    no_skills: bool,
    no_prompt_templates: bool,
    no_themes: bool,
    no_context_files: bool,
    system_prompt_source: Option<String>,
    append_system_prompt_source: Vec<String>,
    extensions: LoadExtensionsResult,
    skills: Vec<Skill>,
    skill_diagnostics: Vec<ResourceDiagnostic>,
    prompts: Vec<PromptTemplate>,
    prompt_diagnostics: Vec<ResourceDiagnostic>,
    themes: Vec<Theme>,
    theme_diagnostics: Vec<ResourceDiagnostic>,
    agents_files: Vec<AgentsFile>,
    system_prompt: Option<String>,
    append_system_prompt: Vec<String>,
    last_skill_paths: Vec<String>,
    last_prompt_paths: Vec<String>,
    last_theme_paths: Vec<String>,
    metadata_by_path: BTreeMap<String, PathMetadata>,
}

pub struct DefaultResourceLoaderOptions<S: SettingsStorage> {
    pub cwd: PathBuf,
    pub agent_dir: PathBuf,
    pub user_agents_dir: Option<PathBuf>,
    pub settings_manager: SettingsManager<S>,
    pub package_runner: Option<Box<dyn PackageCommandRunner>>,
    pub additional_extension_paths: Vec<String>,
    pub additional_skill_paths: Vec<String>,
    pub additional_prompt_paths: Vec<String>,
    pub additional_theme_paths: Vec<String>,
    pub extension_factories: Vec<(String, Box<dyn ExtensionFactory>)>,
    pub no_extensions: bool,
    pub no_skills: bool,
    pub no_prompt_templates: bool,
    pub no_themes: bool,
    pub no_context_files: bool,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Vec<String>,
}

impl<S: SettingsStorage> DefaultResourceLoader<S> {
    pub fn new(options: DefaultResourceLoaderOptions<S>) -> Self {
        Self {
            cwd: normalize_path(options.cwd),
            agent_dir: normalize_path(options.agent_dir),
            user_agents_dir: options
                .user_agents_dir
                .or_else(default_user_agents_dir)
                .map(normalize_path),
            settings_manager: options.settings_manager,
            runtime: create_extension_runtime(),
            package_runner: options
                .package_runner
                .unwrap_or_else(|| Box::new(PackageCommandExecutor)),
            additional_extension_paths: options.additional_extension_paths,
            additional_skill_paths: options.additional_skill_paths,
            additional_prompt_paths: options.additional_prompt_paths,
            additional_theme_paths: options.additional_theme_paths,
            extension_factories: options.extension_factories,
            no_extensions: options.no_extensions,
            no_skills: options.no_skills,
            no_prompt_templates: options.no_prompt_templates,
            no_themes: options.no_themes,
            no_context_files: options.no_context_files,
            system_prompt_source: options.system_prompt,
            append_system_prompt_source: options.append_system_prompt,
            extensions: LoadExtensionsResult::default(),
            skills: Vec::new(),
            skill_diagnostics: Vec::new(),
            prompts: Vec::new(),
            prompt_diagnostics: Vec::new(),
            themes: Vec::new(),
            theme_diagnostics: Vec::new(),
            agents_files: Vec::new(),
            system_prompt: None,
            append_system_prompt: Vec::new(),
            last_skill_paths: Vec::new(),
            last_prompt_paths: Vec::new(),
            last_theme_paths: Vec::new(),
            metadata_by_path: BTreeMap::new(),
        }
    }

    pub fn reload(&mut self) -> Result<(), String> {
        self.settings_manager.reload();
        let resolved = LocalPackageManager::resolve_from_settings(
            self.package_runner.as_ref(),
            &self.settings_manager,
            &self.agent_dir,
            &self.cwd,
            self.user_agents_dir.as_deref(),
            None,
            |_| {},
        )
        .map_err(|error| error.to_string())?;
        let cli_resolved = LocalPackageManager::resolve_extension_sources(
            &self.additional_extension_paths,
            true,
            true,
        );
        self.metadata_by_path = metadata_by_path(&resolved, &cli_resolved);

        let extension_paths = if self.no_extensions {
            enabled_paths(&cli_resolved.extensions)
        } else {
            merge_paths(
                enabled_paths(&cli_resolved.extensions),
                enabled_paths(&resolved.extensions),
            )
        };
        self.extensions = discover_and_load_extensions(&extension_paths, &mut self.runtime);
        let mut factory_result = load_extensions(
            std::mem::take(&mut self.extension_factories),
            &mut self.runtime,
        );
        self.extensions
            .extensions
            .append(&mut factory_result.extensions);
        self.extensions.errors.append(&mut factory_result.errors);
        self.report_extension_conflicts();
        self.apply_extension_source_info();
        self.report_missing_additional_extension_paths();

        let skill_paths = if self.no_skills {
            self.additional_skill_paths.clone()
        } else {
            merge_paths(
                enabled_paths(&resolved.skills),
                self.settings_manager.get_skill_paths(),
            )
        };
        self.last_skill_paths = merge_paths(skill_paths, self.additional_skill_paths.clone());
        self.update_skills_from_paths(self.last_skill_paths.clone());
        self.report_missing_additional_skill_paths();

        let prompt_paths = if self.no_prompt_templates {
            self.additional_prompt_paths.clone()
        } else {
            merge_paths(
                enabled_paths(&resolved.prompts),
                self.settings_manager.get_prompt_template_paths(),
            )
        };
        self.last_prompt_paths = merge_paths(prompt_paths, self.additional_prompt_paths.clone());
        self.update_prompts_from_paths(self.last_prompt_paths.clone());
        self.report_missing_additional_prompt_paths();

        let theme_paths = if self.no_themes {
            self.additional_theme_paths.clone()
        } else {
            merge_paths(
                enabled_paths(&resolved.themes),
                self.settings_manager.get_theme_paths(),
            )
        };
        self.last_theme_paths = merge_paths(theme_paths, self.additional_theme_paths.clone());
        self.update_themes_from_paths(self.last_theme_paths.clone());

        self.agents_files = if self.no_context_files {
            Vec::new()
        } else {
            load_project_context_files(&self.cwd, &self.agent_dir)
        };
        let system_prompt_source = self
            .system_prompt_source
            .clone()
            .or_else(|| self.discover_system_prompt_file());
        self.system_prompt = resolve_prompt_input(system_prompt_source.as_deref());
        let append_sources = if self.append_system_prompt_source.is_empty() {
            self.discover_append_system_prompt_file()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            self.append_system_prompt_source.clone()
        };
        self.append_system_prompt = append_sources
            .iter()
            .filter_map(|source| resolve_prompt_input(Some(source)))
            .collect();
        Ok(())
    }

    pub fn extensions(&self) -> &LoadExtensionsResult {
        &self.extensions
    }

    pub fn skills(&self) -> (&[Skill], &[ResourceDiagnostic]) {
        (&self.skills, &self.skill_diagnostics)
    }

    pub fn prompts(&self) -> (&[PromptTemplate], &[ResourceDiagnostic]) {
        (&self.prompts, &self.prompt_diagnostics)
    }

    pub fn themes(&self) -> (&[Theme], &[ResourceDiagnostic]) {
        (&self.themes, &self.theme_diagnostics)
    }

    pub fn agents_files(&self) -> &[AgentsFile] {
        &self.agents_files
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn append_system_prompt(&self) -> &[String] {
        &self.append_system_prompt
    }

    pub fn resource_slash_commands(&self) -> Vec<SlashCommandInfo> {
        resource_slash_commands(&self.prompts, &self.skills)
    }

    pub fn extend_resources(&mut self, paths: ResourceExtensionPaths) {
        if !paths.skill_paths.is_empty() {
            extend_metadata_by_resource_paths(&mut self.metadata_by_path, &paths.skill_paths);
            self.last_skill_paths = merge_paths(
                self.last_skill_paths.clone(),
                paths
                    .skill_paths
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect(),
            );
            self.update_skills_from_paths(self.last_skill_paths.clone());
        }
        if !paths.prompt_paths.is_empty() {
            extend_metadata_by_resource_paths(&mut self.metadata_by_path, &paths.prompt_paths);
            self.last_prompt_paths = merge_paths(
                self.last_prompt_paths.clone(),
                paths
                    .prompt_paths
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect(),
            );
            self.update_prompts_from_paths(self.last_prompt_paths.clone());
        }
        if !paths.theme_paths.is_empty() {
            extend_metadata_by_resource_paths(&mut self.metadata_by_path, &paths.theme_paths);
            self.last_theme_paths = merge_paths(
                self.last_theme_paths.clone(),
                paths
                    .theme_paths
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect(),
            );
            self.update_themes_from_paths(self.last_theme_paths.clone());
        }
    }

    pub fn extend_resources_from_extensions(&mut self, resources: DiscoveredExtensionResources) {
        self.extend_resources(discovered_resources_to_paths(resources));
    }

    fn update_skills_from_paths(&mut self, paths: Vec<String>) {
        if self.no_skills && paths.is_empty() {
            self.skills.clear();
            self.skill_diagnostics.clear();
            return;
        }
        let (skills, diagnostics) = load_skills(
            paths
                .into_iter()
                .map(|path| self.resolve_resource_path(&path)),
        );
        self.skills = skills
            .into_iter()
            .map(|mut skill| {
                skill.source_info = self.source_info_value_for_path(&skill.file_path);
                skill
            })
            .collect();
        self.skill_diagnostics = diagnostics;
    }

    fn update_prompts_from_paths(&mut self, paths: Vec<String>) {
        if self.no_prompt_templates && paths.is_empty() {
            self.prompts.clear();
            self.prompt_diagnostics.clear();
            return;
        }
        let resolved = paths
            .iter()
            .map(|path| self.resolve_resource_path(path))
            .collect::<Vec<_>>();
        let (prompts, diagnostics) = load_prompts(&resolved);
        self.prompts = prompts
            .into_iter()
            .map(|mut prompt| {
                prompt.source_info = self.source_info_value_for_path(&prompt.file_path);
                prompt
            })
            .collect();
        self.prompt_diagnostics = diagnostics;
    }

    fn update_themes_from_paths(&mut self, paths: Vec<String>) {
        if self.no_themes && paths.is_empty() {
            self.themes.clear();
            self.theme_diagnostics.clear();
            return;
        }
        let resolved = paths
            .iter()
            .map(|path| self.resolve_resource_path(path))
            .collect::<Vec<_>>();
        let (themes, diagnostics) = load_themes(&resolved);
        self.themes = themes
            .into_iter()
            .map(|mut theme| {
                theme.source_info = self.source_info_value_for_path(&theme.path);
                theme
            })
            .collect();
        self.theme_diagnostics = diagnostics;
    }

    fn report_missing_additional_prompt_paths(&mut self) {
        let missing = self
            .additional_prompt_paths
            .iter()
            .map(|path| self.resolve_resource_path(path))
            .filter(|path| !path.exists())
            .map(display_path)
            .collect::<Vec<_>>();
        for path in missing {
            if self
                .prompt_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.as_deref() == Some(path.as_str()))
            {
                continue;
            }
            self.prompt_diagnostics.push(ResourceDiagnostic {
                kind: crate::diagnostics::ResourceDiagnosticKind::Error,
                message: "Prompt template path does not exist".to_string(),
                path: Some(path),
                collision: None,
            });
        }
    }

    fn report_missing_additional_skill_paths(&mut self) {
        let missing = self
            .additional_skill_paths
            .iter()
            .map(|path| self.resolve_resource_path(path))
            .filter(|path| !path.exists())
            .map(display_path)
            .collect::<Vec<_>>();
        for path in missing {
            if self.skill_diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::diagnostics::ResourceDiagnosticKind::Error
                    && diagnostic.path.as_deref() == Some(path.as_str())
            }) {
                continue;
            }
            self.skill_diagnostics.push(ResourceDiagnostic {
                kind: crate::diagnostics::ResourceDiagnosticKind::Error,
                message: "Skill path does not exist".to_string(),
                path: Some(path),
                collision: None,
            });
        }
    }

    fn report_missing_additional_extension_paths(&mut self) {
        let missing = self
            .additional_extension_paths
            .iter()
            .filter(|path| is_local_path(path))
            .map(|path| self.resolve_resource_path(path))
            .filter(|path| !path.exists())
            .map(display_path)
            .collect::<Vec<_>>();
        for path in missing {
            if self
                .extensions
                .errors
                .iter()
                .any(|error| error.extension_path == path)
            {
                continue;
            }
            self.extensions
                .errors
                .push(crate::extensions::ExtensionError {
                    extension_path: path.clone(),
                    event: None,
                    message: format!("Extension path does not exist: {path}"),
                });
        }
    }

    fn report_extension_conflicts(&mut self) {
        let mut tool_owners = BTreeMap::<String, String>::new();
        let mut flag_owners = BTreeMap::<String, String>::new();
        let mut conflicts = Vec::new();

        for extension in &self.extensions.extensions {
            for tool_name in extension.tools.keys() {
                if let Some(existing_owner) = tool_owners.get(tool_name) {
                    if existing_owner != &extension.path {
                        conflicts.push(crate::extensions::ExtensionError {
                            extension_path: extension.path.clone(),
                            event: None,
                            message: format!(
                                "Tool \"{tool_name}\" conflicts with {existing_owner}"
                            ),
                        });
                    }
                } else {
                    tool_owners.insert(tool_name.clone(), extension.path.clone());
                }
            }

            for flag_name in extension.flags.keys() {
                if let Some(existing_owner) = flag_owners.get(flag_name) {
                    if existing_owner != &extension.path {
                        conflicts.push(crate::extensions::ExtensionError {
                            extension_path: extension.path.clone(),
                            event: None,
                            message: format!(
                                "Flag \"--{flag_name}\" conflicts with {existing_owner}"
                            ),
                        });
                    }
                } else {
                    flag_owners.insert(flag_name.clone(), extension.path.clone());
                }
            }
        }

        self.extensions.errors.extend(conflicts);
    }

    fn resolve_resource_path(&self, path: &str) -> PathBuf {
        let expanded = expand_home(path);
        if expanded.is_absolute() {
            expanded
        } else {
            self.cwd.join(expanded)
        }
    }

    fn source_info_value_for_path(&self, path: &str) -> Option<Value> {
        let source_info = find_source_info_for_path(path, &self.metadata_by_path)
            .unwrap_or_else(|| self.default_source_info_for_path(path));
        serde_json::to_value(source_info).ok()
    }

    fn default_source_info_for_path(&self, path: &str) -> crate::source_info::SourceInfo {
        let resolved = self.resolve_resource_path(path);
        let normalized = display_path(&resolved);
        let user_roots = [
            self.agent_dir.join("skills"),
            self.agent_dir.join("prompts"),
            self.agent_dir.join("themes"),
            self.agent_dir.join("extensions"),
        ];
        let project_roots = [
            self.cwd
                .join(crate::settings_manager::CONFIG_DIR_NAME)
                .join("skills"),
            self.cwd
                .join(crate::settings_manager::CONFIG_DIR_NAME)
                .join("prompts"),
            self.cwd
                .join(crate::settings_manager::CONFIG_DIR_NAME)
                .join("themes"),
            self.cwd
                .join(crate::settings_manager::CONFIG_DIR_NAME)
                .join("extensions"),
        ];

        for root in user_roots {
            if is_under_path(&resolved, &root) {
                return create_source_info(
                    path,
                    &PathMetadata {
                        source: "local".to_string(),
                        scope: SourceScope::User,
                        origin: SourceOrigin::TopLevel,
                        base_dir: Some(display_path(root)),
                    },
                );
            }
        }
        for root in project_roots {
            if is_under_path(&resolved, &root) {
                return create_source_info(
                    path,
                    &PathMetadata {
                        source: "local".to_string(),
                        scope: SourceScope::Project,
                        origin: SourceOrigin::TopLevel,
                        base_dir: Some(display_path(root)),
                    },
                );
            }
        }

        create_source_info(
            path,
            &PathMetadata {
                source: "local".to_string(),
                scope: SourceScope::Temporary,
                origin: SourceOrigin::TopLevel,
                base_dir: resolved.metadata().ok().and_then(|metadata| {
                    if metadata.is_dir() {
                        Some(normalized)
                    } else {
                        resolved.parent().map(display_path)
                    }
                }),
            },
        )
    }

    fn apply_extension_source_info(&mut self) {
        let metadata_by_path = self.metadata_by_path.clone();
        for extension in &mut self.extensions.extensions {
            let source_info = find_source_info_for_path(&extension.path, &metadata_by_path)
                .unwrap_or_else(|| extension.source_info.clone());
            extension.source_info = source_info.clone();
            for command in extension.commands.values_mut() {
                command.source_info = source_info.clone();
            }
            for tool in extension.tools.values_mut() {
                tool.source_info = source_info.clone();
            }
        }
    }

    fn discover_system_prompt_file(&self) -> Option<String> {
        discover_first_file(&self.agent_dir, &["system-prompt.md", "system.md"])
    }

    fn discover_append_system_prompt_file(&self) -> Option<String> {
        discover_first_file(
            &self.agent_dir,
            &["append-system-prompt.md", "append-system.md"],
        )
    }
}

fn extend_metadata_by_resource_paths(
    metadata: &mut BTreeMap<String, PathMetadata>,
    resources: &[ResourcePath],
) {
    for resource in resources {
        insert_metadata(
            metadata,
            &ResolvedResource {
                path: resource.path.clone(),
                enabled: true,
                metadata: resource.metadata.clone(),
            },
        );
    }
}

fn metadata_by_path(
    resolved: &ResolvedPaths,
    cli_resolved: &ResolvedPaths,
) -> BTreeMap<String, PathMetadata> {
    let mut metadata = BTreeMap::new();
    for resource in all_resources(resolved) {
        insert_metadata(&mut metadata, resource);
    }
    for resource in all_resources(cli_resolved) {
        metadata
            .entry(resource.path.clone())
            .or_insert_with(|| PathMetadata {
                source: "cli".to_string(),
                scope: SourceScope::Temporary,
                origin: SourceOrigin::TopLevel,
                base_dir: resource.metadata.base_dir.clone(),
            });
    }
    metadata
}

fn all_resources(paths: &ResolvedPaths) -> impl Iterator<Item = &ResolvedResource> {
    paths
        .extensions
        .iter()
        .chain(paths.skills.iter())
        .chain(paths.prompts.iter())
        .chain(paths.themes.iter())
}

fn insert_metadata(metadata: &mut BTreeMap<String, PathMetadata>, resource: &ResolvedResource) {
    metadata
        .entry(resource.path.clone())
        .or_insert_with(|| resource.metadata.clone());
    if let Ok(canonical) = std::fs::canonicalize(&resource.path) {
        metadata
            .entry(display_path(canonical))
            .or_insert_with(|| resource.metadata.clone());
    }
    if resource.metadata.origin == SourceOrigin::Package {
        let skill_file = PathBuf::from(&resource.path).join("SKILL.md");
        if skill_file.exists() {
            metadata
                .entry(display_path(&skill_file))
                .or_insert_with(|| resource.metadata.clone());
            if let Ok(canonical) = std::fs::canonicalize(skill_file) {
                metadata
                    .entry(display_path(canonical))
                    .or_insert_with(|| resource.metadata.clone());
            }
        }
    }
}

fn find_source_info_for_path(
    resource_path: &str,
    metadata_by_path: &BTreeMap<String, PathMetadata>,
) -> Option<crate::source_info::SourceInfo> {
    if resource_path.is_empty() || resource_path.starts_with('<') {
        return None;
    }
    if let Some(metadata) = metadata_by_path.get(resource_path) {
        return Some(create_source_info(resource_path, metadata));
    }
    let normalized_resource_path =
        std::fs::canonicalize(resource_path).unwrap_or_else(|_| PathBuf::from(resource_path));
    for (source_path, metadata) in metadata_by_path {
        if is_under_path(&normalized_resource_path, source_path) {
            return Some(create_source_info(resource_path, metadata));
        }
    }
    None
}

fn is_under_path(path: impl AsRef<std::path::Path>, root: impl AsRef<std::path::Path>) -> bool {
    let path = path.as_ref();
    let root = root.as_ref();
    path == root || path.starts_with(root)
}

fn default_user_agents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".agents"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::ResourceDiagnosticKind;
    use crate::extensions::types::{CommandHandler, ExecutableToolDefinition, ToolExecutor};
    use crate::package_manager::{CommandExecution, PackageCommandStep};
    use crate::resource_loader::paths::display_path;
    use crate::settings_manager::{InMemorySettingsStorage, SettingsManager};
    use serde_json::json;
    use serde_json::Value;
    use std::cell::RefCell;
    use std::fs;
    use std::sync::Arc;
    use std::time::SystemTime;

    #[derive(Default)]
    struct InstallingPackageRunner {
        calls: RefCell<Vec<PackageCommandStep>>,
    }

    impl PackageCommandRunner for InstallingPackageRunner {
        fn run(&self, step: &PackageCommandStep) -> Result<CommandExecution, String> {
            self.calls.borrow_mut().push(step.clone());
            if step.command == "ensure_npm_project" {
                let root = PathBuf::from(
                    step.args
                        .first()
                        .expect("ensure_npm_project should have root"),
                );
                let package_dir = root.join("node_modules").join("pkg");
                fs::create_dir_all(package_dir.join("prompts"))
                    .expect("package prompts dir should be created");
                fs::write(
                    package_dir.join("package.json"),
                    r#"{"pi":{"prompts":["prompts/generated.md"]}}"#,
                )
                .expect("package manifest should be written");
                fs::write(
                    package_dir.join("prompts").join("generated.md"),
                    "# Generated\nBody",
                )
                .expect("package prompt should be written");
            }
            Ok(CommandExecution {
                step: step.clone(),
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            })
        }
    }

    struct SourceInfoExtensionFactory;

    impl ExtensionFactory for SourceInfoExtensionFactory {
        fn load(&self, api: &mut crate::extensions::ExtensionApi<'_>) -> Result<(), String> {
            let command: CommandHandler = Arc::new(|_| Ok(()));
            api.register_command("demo", Some("Demo command".to_string()), command)?;
            let execute: ToolExecutor = Arc::new(|input: Value, _| Ok(input));
            api.register_tool(ExecutableToolDefinition {
                definition: crate::extensions::ToolDefinition {
                    name: "demo_tool".to_string(),
                    label: None,
                    description: "Demo tool".to_string(),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute,
            })?;
            Ok(())
        }
    }

    struct ResourceDiscoverExtensionFactory {
        prompt_path: String,
    }

    impl ExtensionFactory for ResourceDiscoverExtensionFactory {
        fn load(&self, api: &mut crate::extensions::ExtensionApi<'_>) -> Result<(), String> {
            let prompt_path = self.prompt_path.clone();
            api.on(
                "resources_discover",
                Arc::new(move |_| {
                    Some(json!({
                        "promptPaths": [prompt_path],
                    }))
                }),
            )
        }
    }

    struct ConflictingExtensionFactory;

    impl ExtensionFactory for ConflictingExtensionFactory {
        fn load(&self, api: &mut crate::extensions::ExtensionApi<'_>) -> Result<(), String> {
            let execute: ToolExecutor = Arc::new(|input: Value, _| Ok(input));
            api.register_tool(ExecutableToolDefinition {
                definition: crate::extensions::ToolDefinition {
                    name: "demo_tool".to_string(),
                    label: None,
                    description: "Demo tool".to_string(),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute,
            })?;
            api.register_flag(crate::extensions::ExtensionFlag {
                name: "demo-flag".to_string(),
                flag_type: Default::default(),
                description: Some("Demo flag".to_string()),
            })?;
            Ok(())
        }
    }

    #[test]
    fn reload_loads_skill_prompt_and_theme_paths() {
        let dir = temp_dir();
        let skill_dir = dir.join("skill");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\nBody",
        )
        .expect("skill");
        fs::write(dir.join("prompt.md"), "# Fix\nPrompt").expect("prompt");
        fs::write(dir.join("theme.json"), r#"{"name":"work"}"#).expect("theme");

        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: vec![display_path(&skill_dir)],
            additional_prompt_paths: vec![display_path(dir.join("prompt.md"))],
            additional_theme_paths: vec![display_path(dir.join("theme.json"))],
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: false,
            no_themes: false,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        assert_eq!(loader.skills().0[0].name, "demo");
        assert_eq!(loader.prompts().0[0].name, "prompt");
        assert_eq!(loader.themes().0[0].name, "work");
        let commands = loader.resource_slash_commands();
        assert!(commands.iter().any(|command| command.name == "prompt"));
        assert!(commands.iter().any(|command| command.name == "skill:demo"));
    }

    #[test]
    fn reload_reports_package_manager_errors_like_pi() {
        let dir = temp_dir();
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"],
            "npmCommand": [""]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        let error = loader
            .reload()
            .expect_err("package-manager errors should propagate from reload");

        assert_eq!(
            error,
            "Invalid npmCommand: first array entry must be a non-empty command"
        );
    }

    #[test]
    fn reload_reports_missing_additional_prompt_path_like_pi() {
        let dir = temp_dir();
        let missing_prompt = dir.join("missing.md");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: vec![display_path(&missing_prompt)],
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        let diagnostics = loader.prompts().1;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResourceDiagnosticKind::Error
                && diagnostic.message == "Prompt template path does not exist"
                && diagnostic.path.as_deref() == Some(missing_prompt.to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn reload_reports_missing_additional_skill_path_like_pi() {
        let dir = temp_dir();
        let missing_skill = dir.join("missing-skill");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: vec![display_path(&missing_skill)],
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        let diagnostics = loader.skills().1;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResourceDiagnosticKind::Error
                && diagnostic.message == "Skill path does not exist"
                && diagnostic.path.as_deref() == Some(missing_skill.to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn reload_reports_missing_additional_extension_path_like_pi() {
        let dir = temp_dir();
        let missing_extension = dir.join("missing-extension.ts");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: vec![display_path(&missing_extension)],
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert!(loader.extensions().errors.iter().any(|error| {
            error.extension_path == missing_extension.to_string_lossy()
                && error.message
                    == format!(
                        "Extension path does not exist: {}",
                        missing_extension.to_string_lossy()
                    )
        }));
    }

    #[test]
    fn reload_reports_extension_tool_and_flag_conflicts_like_pi() {
        let dir = temp_dir();
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![
                (
                    "/extensions/one.ts".to_string(),
                    Box::new(ConflictingExtensionFactory) as Box<dyn ExtensionFactory>,
                ),
                (
                    "/extensions/two.ts".to_string(),
                    Box::new(ConflictingExtensionFactory) as Box<dyn ExtensionFactory>,
                ),
            ],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        let errors = &loader.extensions().errors;
        assert!(errors.iter().any(|error| {
            error.extension_path == "/extensions/two.ts"
                && error.message == r#"Tool "demo_tool" conflicts with /extensions/one.ts"#
        }));
        assert!(errors.iter().any(|error| {
            error.extension_path == "/extensions/two.ts"
                && error.message == r#"Flag "--demo-flag" conflicts with /extensions/one.ts"#
        }));
    }

    #[test]
    fn extends_resources_from_extension_discovery() {
        let dir = temp_dir();
        let skill_dir = dir.join("extension-skill");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: extension-skill\ndescription: Extension skill\n---\nBody",
        )
        .expect("skill");
        let prompt_path = dir.join("extension-prompt.md");
        fs::write(&prompt_path, "# Extension Prompt\nPrompt").expect("prompt");
        let theme_path = dir.join("extension-theme.json");
        fs::write(&theme_path, r#"{"name":"extension-theme"}"#).expect("theme");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: false,
            no_themes: false,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");

        loader.extend_resources_from_extensions(crate::extensions::DiscoveredExtensionResources {
            skill_paths: vec![crate::extensions::ExtensionResourcePath {
                path: display_path(&skill_dir),
                extension_path: "/extensions/resources.ts".to_string(),
                metadata: None,
            }],
            prompt_paths: vec![crate::extensions::ExtensionResourcePath {
                path: display_path(&prompt_path),
                extension_path: "/extensions/resources.ts".to_string(),
                metadata: None,
            }],
            theme_paths: vec![crate::extensions::ExtensionResourcePath {
                path: display_path(&theme_path),
                extension_path: "/extensions/resources.ts".to_string(),
                metadata: None,
            }],
        });

        assert_eq!(loader.skills().0[0].name, "extension-skill");
        assert_eq!(loader.prompts().0[0].name, "extension-prompt");
        assert_eq!(loader.themes().0[0].name, "extension-theme");
    }

    #[test]
    fn reload_resolves_prompt_resources_from_configured_packages_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let package_dir = agent_dir.join("npm").join("node_modules").join("pkg");
        fs::create_dir_all(package_dir.join("prompts")).expect("package prompts dir");
        fs::write(
            package_dir.join("package.json"),
            r#"{"pi":{"prompts":["prompts/review.md"]}}"#,
        )
        .expect("package manifest");
        fs::write(
            package_dir.join("prompts").join("review.md"),
            "# Review\nBody",
        )
        .expect("package prompt");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.prompts().0[0].name, "review");
    }

    #[test]
    fn reload_installs_missing_configured_package_resources_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let runner = InstallingPackageRunner::default();
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: Some(Box::new(runner)),
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.prompts().0[0].name, "generated");
    }

    #[test]
    fn reload_preserves_package_prompt_source_info_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir: agent_dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: Some(Box::new(InstallingPackageRunner::default())),
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        let commands =
            crate::slash_commands::slash_commands_to_rpc(&loader.resource_slash_commands());
        let command = commands
            .iter()
            .find(|command| command.name == "generated")
            .expect("generated prompt command should be exposed");
        assert_eq!(command.source_info["source"], "npm:pkg");
        assert_eq!(command.source_info["scope"], "user");
        assert_eq!(command.source_info["origin"], "package");
        let base_dir = command.source_info["baseDir"]
            .as_str()
            .expect("baseDir should be serialized");
        assert!(base_dir.ends_with("/npm/node_modules/pkg"));
    }

    #[test]
    fn reload_preserves_package_extension_source_info_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let package_dir = agent_dir.join("npm").join("node_modules").join("pkg");
        let extension_path = package_dir.join("extensions").join("demo.ts");
        fs::create_dir_all(extension_path.parent().expect("extension parent"))
            .expect("package extension dir");
        fs::write(
            package_dir.join("package.json"),
            r#"{"pi":{"extensions":["extensions/demo.ts"]}}"#,
        )
        .expect("package manifest");
        fs::write(&extension_path, "export default {}").expect("extension file");
        let extension_path = display_path(&extension_path);
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                extension_path.clone(),
                Box::new(SourceInfoExtensionFactory) as Box<dyn ExtensionFactory>,
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        let extension = loader
            .extensions()
            .extensions
            .iter()
            .find(|extension| extension.path == extension_path)
            .expect("package extension should be loaded");
        assert_eq!(extension.source_info.source, "npm:pkg");
        assert_eq!(extension.source_info.scope, SourceScope::User);
        assert_eq!(extension.source_info.origin, SourceOrigin::Package);
        let factory_extension = loader
            .extensions()
            .extensions
            .iter()
            .find(|extension| extension.commands.contains_key("demo"))
            .expect("factory extension should be loaded");
        let command = factory_extension
            .commands
            .get("demo")
            .expect("demo command");
        let tool = factory_extension.tools.get("demo_tool").expect("demo tool");
        assert_eq!(factory_extension.source_info.source, "npm:pkg");
        assert_eq!(command.source_info.source, "npm:pkg");
        assert_eq!(tool.source_info.source, "npm:pkg");
    }

    #[test]
    fn extension_discovered_resources_keep_package_source_info_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let package_dir = agent_dir.join("npm").join("node_modules").join("pkg");
        let extension_path = package_dir.join("extensions").join("resources.ts");
        let prompt_path = package_dir.join("prompts").join("review.md");
        fs::create_dir_all(extension_path.parent().expect("extension parent"))
            .expect("package extension dir");
        fs::create_dir_all(prompt_path.parent().expect("prompt parent")).expect("prompt dir");
        fs::write(
            package_dir.join("package.json"),
            r#"{"pi":{"extensions":["extensions/resources.ts"]}}"#,
        )
        .expect("package manifest");
        fs::write(&extension_path, "export default {}").expect("extension file");
        fs::write(&prompt_path, "# Review\nBody").expect("prompt file");
        let extension_path = display_path(&extension_path);
        let prompt_path = display_path(&prompt_path);
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg"]
        }));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                extension_path,
                Box::new(ResourceDiscoverExtensionFactory {
                    prompt_path: prompt_path.clone(),
                }) as Box<dyn ExtensionFactory>,
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");
        let resources = crate::extensions::emit_resources_discover(
            &loader.extensions().extensions,
            "",
            crate::extensions::ResourcesDiscoverReason::Reload,
            |_| {},
        );
        loader.extend_resources_from_extensions(resources);

        let commands =
            crate::slash_commands::slash_commands_to_rpc(&loader.resource_slash_commands());
        let command = commands
            .iter()
            .find(|command| command.name == "review")
            .expect("discovered prompt command should be exposed");
        assert_eq!(command.source_info["source"], "npm:pkg");
        assert_eq!(command.source_info["origin"], "package");
        assert_eq!(command.source_info["path"], prompt_path);
    }

    #[test]
    fn reload_auto_discovers_project_prompt_resources_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let project_prompt_dir = dir
            .join(crate::settings_manager::CONFIG_DIR_NAME)
            .join("prompts");
        fs::create_dir_all(&project_prompt_dir).expect("project prompts dir");
        fs::write(project_prompt_dir.join("review.md"), "# Review\nBody").expect("project prompt");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.prompts().0[0].name, "review");
    }

    #[test]
    fn reload_auto_discovers_project_pi_mode_root_markdown_skills_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let project_skill_dir = dir
            .join(crate::settings_manager::CONFIG_DIR_NAME)
            .join("skills");
        fs::create_dir_all(&project_skill_dir).expect("project skills dir");
        fs::write(
            project_skill_dir.join("review.md"),
            "---\nname: review\ndescription: Review skill\n---\nBody",
        )
        .expect("project skill");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.skills().0[0].name, "review");
    }

    #[test]
    fn reload_auto_discovers_project_agents_skills_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let cwd = dir.join("project").join("nested");
        let skill_dir = dir
            .join("project")
            .join(".agents")
            .join("skills")
            .join("review");
        fs::create_dir_all(&cwd).expect("cwd dir");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review skill\n---\nBody",
        )
        .expect("skill");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd,
            agent_dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.skills().0[0].name, "review");
    }

    #[test]
    fn reload_auto_discovers_user_agents_skills_like_pi() {
        let dir = temp_dir();
        let agent_dir = temp_dir();
        let user_agents_dir = temp_dir().join(".agents");
        let skill_dir = user_agents_dir.join("skills").join("personal");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: personal\ndescription: Personal skill\n---\nBody",
        )
        .expect("skill");
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir,
            agent_dir,
            user_agents_dir: Some(user_agents_dir),
            settings_manager: settings,
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: Vec::new(),
            no_extensions: true,
            no_skills: false,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });

        loader.reload().expect("reload should succeed");

        assert_eq!(loader.skills().0[0].name, "personal");
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-resource-loader-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
