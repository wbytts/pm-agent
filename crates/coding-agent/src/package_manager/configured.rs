use super::settings::package_source_string;
use super::source::{
    installed_path_for_source_with_npm_fallback, parse_source, scoped_source_identity,
    source_identity_from_base,
};
use super::types::{ConfiguredPackage, NpmCommandConfig, PackageFilter, ParsedSource, SourceScope};
use super::updates::ConfiguredUpdateSource;
use crate::settings_manager::{SettingsManager, SettingsStorage};
use std::collections::HashMap;
use std::path::Path;

pub fn list_configured_packages_from_settings<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    npm_command: Option<NpmCommandConfig>,
) -> Vec<ConfiguredPackage> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let npm_command = npm_command.unwrap_or_default();
    let mut configured_packages = Vec::new();

    for (scope, packages) in [
        (SourceScope::User, settings.get_global_packages()),
        (SourceScope::Project, settings.get_project_packages()),
    ] {
        for package in packages {
            let Some(source) = package_source_string(&package) else {
                continue;
            };
            let installed_path = installed_path_for_source_with_npm_fallback(
                agent_dir,
                cwd,
                &source,
                scope,
                |npm| super::legacy_global_npm_package_path(npm, cwd, &npm_command),
            )
            .map(super::paths::display_path);
            configured_packages.push(ConfiguredPackage {
                source,
                scope,
                filtered: package.is_object(),
                installed_path,
            });
        }
    }

    configured_packages
}

pub fn configured_package_sources<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Vec<(String, SourceScope, Option<PackageFilter>)> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let mut by_identity = HashMap::<String, (String, SourceScope, Option<PackageFilter>)>::new();
    let mut identities = Vec::<String>::new();

    for (scope, packages) in [
        (SourceScope::Project, settings.get_project_packages()),
        (SourceScope::User, settings.get_global_packages()),
    ] {
        for package in packages {
            let Some(source) = package_source_string(&package) else {
                continue;
            };
            let entry = (source.clone(), scope, package_filter(&package));
            let identity = scoped_source_identity(agent_dir, cwd, &source, scope);
            if !by_identity.contains_key(&identity) {
                identities.push(identity.clone());
            }
            if scope == SourceScope::Project {
                by_identity.insert(identity, entry);
            } else {
                by_identity.entry(identity).or_insert(entry);
            }
        }
    }

    identities
        .into_iter()
        .filter_map(|identity| by_identity.remove(&identity))
        .collect()
}

pub fn configured_update_sources<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source_filter: Option<&str>,
) -> Result<Vec<ConfiguredUpdateSource>, String> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let filter_identity = source_filter.map(|source| source_identity_from_base(source, cwd));
    let mut matched = source_filter.is_none();
    let mut sources = Vec::new();

    for (scope, packages) in [
        (SourceScope::User, settings.get_global_packages()),
        (SourceScope::Project, settings.get_project_packages()),
    ] {
        for package in packages {
            let Some(source) = package_source_string(&package) else {
                continue;
            };
            let identity = scoped_source_identity(agent_dir, cwd, &source, scope);
            if filter_identity
                .as_ref()
                .is_some_and(|filter| filter != &identity)
            {
                continue;
            }
            matched = true;
            sources.push(ConfiguredUpdateSource { source, scope });
        }
    }

    if let Some(source_filter) = source_filter.filter(|_| !matched) {
        return Err(build_no_matching_package_message(
            source_filter,
            settings
                .get_global_packages()
                .into_iter()
                .chain(settings.get_project_packages())
                .collect(),
        ));
    }
    Ok(sources)
}

fn build_no_matching_package_message(
    source: &str,
    configured_packages: Vec<serde_json::Value>,
) -> String {
    if let Some(suggestion) = find_suggested_configured_source(source, configured_packages) {
        return format!("No matching package found for {source}. Did you mean {suggestion}?");
    }
    format!("No matching package found for {source}")
}

fn find_suggested_configured_source(
    source: &str,
    configured_packages: Vec<serde_json::Value>,
) -> Option<String> {
    let trimmed = source.trim();
    for package in configured_packages {
        let Some(source) = package_source_string(&package) else {
            continue;
        };
        match parse_source(&source) {
            ParsedSource::Npm(npm) => {
                if trimmed == npm.name || trimmed == npm.spec {
                    return Some(source);
                }
            }
            ParsedSource::Git(git) => {
                let shorthand = format!("{}/{}", git.host, git.path);
                let shorthand_with_ref = git
                    .reference
                    .as_ref()
                    .map(|reference| format!("{shorthand}@{reference}"));
                if trimmed == shorthand || shorthand_with_ref.as_deref() == Some(trimmed) {
                    return Some(source);
                }
            }
            ParsedSource::Local(_) => {}
        }
    }
    None
}

fn package_filter(value: &serde_json::Value) -> Option<PackageFilter> {
    let object = value.as_object()?;
    Some(PackageFilter {
        extensions: string_array_field(object, "extensions"),
        skills: string_array_field(object, "skills"),
        prompts: string_array_field(object, "prompts"),
        themes: string_array_field(object, "themes"),
    })
}

fn string_array_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<Vec<String>> {
    Some(
        object
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect(),
    )
}

pub fn npm_command_from_settings<S: SettingsStorage>(
    settings: &SettingsManager<S>,
) -> Result<Option<NpmCommandConfig>, String> {
    settings
        .settings()
        .npm_command
        .map(npm_command_from_value_array)
        .unwrap_or(Ok(None))
}

fn npm_command_from_value_array(values: Vec<String>) -> Result<Option<NpmCommandConfig>, String> {
    let mut values = values.into_iter();
    let Some(command) = values.next() else {
        return Ok(None);
    };
    if command.trim().is_empty() {
        return Err(
            "Invalid npmCommand: first array entry must be a non-empty command".to_string(),
        );
    }
    Ok(Some(NpmCommandConfig {
        command,
        args: values.collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_manager::InMemorySettingsStorage;
    use serde_json::json;

    #[test]
    fn configured_update_sources_preserves_global_then_project_duplicates_like_pi_update() {
        let mut storage = InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                r#"{"packages":["npm:pkg","npm:other"]}"#.to_string(),
            )
            .expect("global settings write");
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":["npm:pkg@2.0.0"]}"#.to_string(),
            )
            .expect("project settings write");
        let settings = SettingsManager::from_storage(storage);

        let sources =
            configured_update_sources(&settings, "/agent", "/work", None).expect("sources");

        assert_eq!(
            sources,
            vec![
                ConfiguredUpdateSource {
                    source: "npm:pkg".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "npm:other".to_string(),
                    scope: SourceScope::User,
                },
                ConfiguredUpdateSource {
                    source: "npm:pkg@2.0.0".to_string(),
                    scope: SourceScope::Project,
                },
            ]
        );
    }

    #[test]
    fn configured_update_sources_filters_by_identity() {
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg@1.0.0", "npm:other"]
        }));

        let sources = configured_update_sources(&settings, "/agent", "/work", Some("npm:pkg"))
            .expect("sources");

        assert_eq!(
            sources,
            vec![ConfiguredUpdateSource {
                source: "npm:pkg@1.0.0".to_string(),
                scope: SourceScope::User,
            }]
        );
    }

    #[test]
    fn configured_update_sources_suggests_matching_configured_npm_source_like_pi() {
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["npm:pkg@1.0.0", "npm:other"]
        }));

        let error = configured_update_sources(&settings, "/agent", "/work", Some("pkg"))
            .expect_err("unprefixed package name should suggest configured source");

        assert_eq!(
            error,
            "No matching package found for pkg. Did you mean npm:pkg@1.0.0?"
        );
    }

    #[test]
    fn configured_update_sources_suggests_matching_configured_git_source_like_pi() {
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["git:https://github.com/owner/repo@main"]
        }));

        let error = configured_update_sources(
            &settings,
            "/agent",
            "/work",
            Some("github.com/owner/repo@main"),
        )
        .expect_err("git shorthand with ref should suggest configured source");

        assert_eq!(
            error,
            "No matching package found for github.com/owner/repo@main. Did you mean git:https://github.com/owner/repo@main?"
        );
    }

    #[test]
    fn configured_update_sources_keeps_global_scope_for_targeted_git_update_like_pi() {
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": ["git:github.com/test/extension"]
        }));

        let sources = configured_update_sources(
            &settings,
            "/agent",
            "/work",
            Some("git:github.com/test/extension"),
        )
        .expect("global git source should match targeted update");

        assert_eq!(
            sources,
            vec![ConfiguredUpdateSource {
                source: "git:github.com/test/extension".to_string(),
                scope: SourceScope::User,
            }]
        );
    }

    #[test]
    fn configured_package_sources_dedupes_project_local_sources_by_resolved_scope_base_like_pi() {
        let mut storage = InMemorySettingsStorage::new();
        storage
            .write(
                crate::settings_manager::SettingsScope::Global,
                r#"{"packages":["../work/packages/demo"]}"#.to_string(),
            )
            .expect("global settings write");
        storage
            .write(
                crate::settings_manager::SettingsScope::Project,
                r#"{"packages":["../packages/demo"]}"#.to_string(),
            )
            .expect("project settings write");
        let settings = SettingsManager::from_storage(storage);

        let sources = configured_package_sources(&settings, "/agent", "/work");

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, "../packages/demo");
        assert_eq!(sources[0].1, SourceScope::Project);
        assert!(sources[0].2.is_none());
    }

    #[test]
    fn npm_command_reads_configured_command() {
        let settings = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "npmCommand": ["corepack", "pnpm"]
        }));

        assert_eq!(
            npm_command_from_settings(&settings),
            Ok(Some(NpmCommandConfig {
                command: "corepack".to_string(),
                args: vec!["pnpm".to_string()],
            }))
        );
    }
}
