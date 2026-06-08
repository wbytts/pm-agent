use crate::package_manager::ResolvedResource;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn enabled_paths(resources: &[ResolvedResource]) -> Vec<String> {
    resources
        .iter()
        .filter(|resource| resource.enabled)
        .map(|resource| resource.path.clone())
        .collect()
}

pub fn merge_paths(first: Vec<String>, second: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    first
        .into_iter()
        .chain(second)
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

pub fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

pub fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

pub fn basename_without_extension(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}
