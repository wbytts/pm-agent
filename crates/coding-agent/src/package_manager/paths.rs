use super::manifest::read_pi_manifest;
use super::types::ResourceType;
use std::path::{Path, PathBuf};

pub(super) fn local_source_path(source: &str) -> Option<PathBuf> {
    let path = PathBuf::from(source);
    if path.is_absolute() || source.starts_with('.') || source.starts_with('~') {
        return Some(expand_home(path));
    }
    None
}

fn expand_home(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path
}

pub(super) fn file_resource_type(path: &Path) -> Option<ResourceType> {
    if matches_extension(path, ResourceType::Extension) {
        return Some(ResourceType::Extension);
    }
    if matches_extension(path, ResourceType::Theme) {
        return Some(ResourceType::Theme);
    }
    if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
        return Some(ResourceType::Prompt);
    }
    None
}

pub(super) fn matches_extension(path: &Path, resource_type: ResourceType) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    match resource_type {
        ResourceType::Extension => matches!(extension, Some("ts" | "js")),
        ResourceType::Skill | ResourceType::Prompt => extension == Some("md"),
        ResourceType::Theme => extension == Some("json"),
    }
}

pub(super) fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

pub(super) fn resolve_extension_entries(dir: &Path) -> Option<Vec<PathBuf>> {
    if let Some(manifest) = read_pi_manifest(dir) {
        if let Some(entries) = manifest.extensions {
            let resolved = entries
                .into_iter()
                .map(|entry| dir.join(entry))
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            if !resolved.is_empty() {
                return Some(resolved);
            }
        }
    }
    for index in ["index.ts", "index.js"] {
        let path = dir.join(index);
        if path.exists() {
            return Some(vec![path]);
        }
    }
    None
}
