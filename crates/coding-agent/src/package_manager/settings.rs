use super::source::{package_source_base_dir, parse_source, source_identity};
use super::types::{ParsedSource, SourceScope};
use crate::settings_manager::{SettingsManager, SettingsStorage};
use crate::utils::paths::resolve_path;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub fn add_source_to_settings<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
) -> bool {
    let scope = if local {
        SourceScope::Project
    } else {
        SourceScope::User
    };
    let current = packages_for_scope(settings, scope);
    let normalized =
        normalize_package_source_for_settings(agent_dir.as_ref(), cwd.as_ref(), source, scope);
    let match_index = current.iter().position(|existing| {
        package_sources_match(agent_dir.as_ref(), cwd.as_ref(), existing, source, scope)
    });

    let mut next = current;
    if let Some(index) = match_index {
        if package_source_string(&next[index]).as_deref() == Some(normalized.as_str()) {
            return false;
        }
        next[index] = replace_package_source(next[index].clone(), normalized);
    } else {
        next.push(Value::String(normalized));
    }
    set_packages_for_scope(settings, scope, next);
    true
}

pub fn remove_source_from_settings<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    local: bool,
) -> bool {
    let scope = if local {
        SourceScope::Project
    } else {
        SourceScope::User
    };
    let current = packages_for_scope(settings, scope);
    let next = current
        .iter()
        .filter(|existing| {
            !package_sources_match(agent_dir.as_ref(), cwd.as_ref(), existing, source, scope)
        })
        .cloned()
        .collect::<Vec<_>>();
    if next.len() == current.len() {
        return false;
    }
    set_packages_for_scope(settings, scope, next);
    true
}

pub fn package_sources_match(
    agent_dir: &Path,
    cwd: &Path,
    existing: &Value,
    input_source: &str,
    scope: SourceScope,
) -> bool {
    let Some(existing_source) = package_source_string(existing) else {
        return false;
    };
    source_match_key_for_settings(agent_dir, cwd, &existing_source, scope)
        == source_match_key_for_input(cwd, input_source)
}

pub fn normalize_package_source_for_settings(
    agent_dir: &Path,
    cwd: &Path,
    source: &str,
    scope: SourceScope,
) -> String {
    match parse_source(source) {
        ParsedSource::Local(local) => {
            let base = package_source_base_dir(agent_dir, cwd, scope);
            let resolved = resolve_path(&local.path, cwd, None);
            relative_path(&base, &resolved)
        }
        _ => source.to_string(),
    }
}

pub fn package_source_string(value: &Value) -> Option<String> {
    if let Some(source) = value.as_str() {
        return Some(source.to_string());
    }
    value
        .as_object()
        .and_then(|object| object.get("source"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn replace_package_source(value: Value, source: String) -> Value {
    if value.is_string() {
        return Value::String(source);
    }
    let Some(mut object) = value.as_object().cloned() else {
        return Value::String(source);
    };
    object.insert("source".to_string(), Value::String(source));
    Value::Object(object)
}

fn source_match_key_for_input(cwd: &Path, source: &str) -> String {
    match parse_source(source) {
        ParsedSource::Local(local) => format!(
            "local:{}",
            display_path(&resolve_path(&local.path, cwd, None))
        ),
        _ => source_identity(source),
    }
}

fn source_match_key_for_settings(
    agent_dir: &Path,
    cwd: &Path,
    source: &str,
    scope: SourceScope,
) -> String {
    match parse_source(source) {
        ParsedSource::Local(local) => {
            let base = package_source_base_dir(agent_dir, cwd, scope);
            format!(
                "local:{}",
                display_path(&resolve_path(&local.path, base, None))
            )
        }
        _ => source_identity(source),
    }
}

fn packages_for_scope<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    scope: SourceScope,
) -> Vec<Value> {
    match scope {
        SourceScope::Project => settings.get_project_packages(),
        SourceScope::User | SourceScope::Temporary => settings.get_global_packages(),
    }
}

fn set_packages_for_scope<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    scope: SourceScope,
    packages: Vec<Value>,
) {
    match scope {
        SourceScope::Project => settings.set_project_packages(packages),
        SourceScope::User | SourceScope::Temporary => settings.set_packages(packages),
    }
}

fn relative_path(base: &Path, path: &Path) -> String {
    let base_components = path_components(base);
    let path_components = path_components(path);
    let mut common = 0;
    while common < base_components.len()
        && common < path_components.len()
        && base_components[common] == path_components[common]
    {
        common += 1;
    }

    if common == 0
        && base_components
            .first()
            .is_some_and(|component| component == "/")
        && path_components
            .first()
            .is_some_and(|component| component == "/")
    {
        common = 1;
    }

    let mut relative = PathBuf::new();
    for _ in common..base_components.len() {
        relative.push("..");
    }
    for component in &path_components[common..] {
        relative.push(component);
    }

    if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        explicit_relative_path(display_path(relative))
    }
}

fn explicit_relative_path(path: String) -> String {
    if path == "." || path.starts_with("./") || path.starts_with("../") {
        path
    } else {
        format!("./{path}")
    }
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::RootDir => Some("/".to_string()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            Component::ParentDir => Some("..".to_string()),
        })
        .collect()
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_manager::{InMemorySettingsStorage, SettingsManager};
    use serde_json::json;

    #[test]
    fn adds_and_removes_global_package_sources() {
        let mut manager = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "npm:@scope/pkg",
            false
        ));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "npm:@scope/pkg@1.0.0",
            false
        ));
        assert_eq!(
            manager.get_global_packages(),
            vec![Value::String("npm:@scope/pkg@1.0.0".to_string())]
        );
        assert!(remove_source_from_settings(
            &mut manager,
            "/agent",
            "/work",
            "npm:@scope/pkg",
            false
        ));
        assert!(manager.get_global_packages().is_empty());
    }

    #[test]
    fn updates_existing_object_package_source_without_losing_filters() {
        let mut manager = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": [{ "source": "npm:pkg@1.0.0", "prompts": ["review.md"] }]
        }));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "npm:pkg@2.0.0",
            false
        ));
        assert_eq!(
            manager.get_global_packages(),
            vec![json!({ "source": "npm:pkg@2.0.0", "prompts": ["review.md"] })]
        );
    }

    #[test]
    fn git_sources_match_by_normalized_identity_like_pi() {
        let mut manager = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({
            "packages": [{ "source": "git:https://github.com/owner/repo@main", "extensions": ["src/index.ts"] }]
        }));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "git:git@github.com:owner/repo@v2",
            false,
        ));
        assert_eq!(
            manager.get_global_packages(),
            vec![
                json!({ "source": "git:git@github.com:owner/repo@v2", "extensions": ["src/index.ts"] })
            ]
        );
    }

    #[test]
    fn local_project_sources_are_normalized_relative_to_project_config_dir_like_pi() {
        let mut manager = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "/work/packages/demo",
            true,
        ));
        assert_eq!(
            manager.get_project_packages(),
            vec![Value::String("../packages/demo".to_string())]
        );
    }

    #[test]
    fn local_sources_under_settings_base_keep_dot_slash_prefix_like_pi() {
        let mut manager = SettingsManager::<InMemorySettingsStorage>::in_memory(json!({}));
        assert!(add_source_to_settings(
            &mut manager,
            "/agent",
            "/work",
            "/agent/packages/demo",
            false,
        ));

        assert_eq!(
            manager.get_global_packages(),
            vec![Value::String("./packages/demo".to_string())]
        );
    }
}
