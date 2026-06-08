use super::manifest::{read_pi_manifest, PiManifest};
use super::paths::{
    display_path, file_resource_type, local_source_path, matches_extension,
    resolve_extension_entries,
};
use super::patterns::{
    apply_patterns, filter_paths, is_enabled_by_overrides, is_ignored_by_rules, load_ignore_rules,
};
use super::types::{
    PackageFilter, PathMetadata, ResolvedPaths, ResolvedResource, ResourceType, SourceOrigin,
    SourceScope,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillDiscoveryMode {
    Pi,
    Agents,
}

pub fn resolve_source(
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    filter: Option<&PackageFilter>,
) -> ResolvedPaths {
    let Some(path) = local_source_path(source) else {
        return ResolvedPaths::default();
    };
    resolve_source_at_path(source, &path, scope, origin, filter)
}

pub(super) fn resolve_source_at_path(
    source: &str,
    path: &Path,
    scope: SourceScope,
    origin: SourceOrigin,
    filter: Option<&PackageFilter>,
) -> ResolvedPaths {
    if !path.exists() {
        return ResolvedPaths::default();
    }

    if path.is_file() {
        return resolve_single_file(source, scope, origin, path);
    }

    let manifest = read_pi_manifest(path);
    let mut paths = ResolvedPaths::default();
    if let Some(manifest) = manifest {
        collect_manifest_entries(source, scope, path, &manifest, filter, &mut paths);
    } else {
        collect_convention_entries(source, scope, origin, path, filter, &mut paths);
    }
    paths
}

pub fn resolve_auto_discovered_resources(
    user_base_dir: impl AsRef<Path>,
    project_base_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    user_agents_dir: Option<&Path>,
    user_filter: Option<&PackageFilter>,
    project_filter: Option<&PackageFilter>,
) -> ResolvedPaths {
    let mut paths = ResolvedPaths::default();
    let project_base_dir = project_base_dir.as_ref();
    let cwd = cwd.as_ref();
    let user_agents_skills_dir = user_agents_dir.map(|dir| dir.join("skills"));
    collect_auto_discovered_entries(
        &mut paths,
        SourceScope::Project,
        project_base_dir,
        project_filter,
    );
    collect_project_agents_skill_entries(
        &mut paths,
        cwd,
        user_agents_skills_dir.as_deref(),
        project_filter.and_then(|f| f.skills.as_deref()),
    );
    collect_auto_discovered_entries(
        &mut paths,
        SourceScope::User,
        user_base_dir.as_ref(),
        user_filter,
    );
    if let Some(user_agents_dir) = user_agents_dir {
        collect_user_agents_skill_entries(
            &mut paths,
            user_agents_dir,
            user_filter.and_then(|f| f.skills.as_deref()),
        );
    }
    sort_resolved_paths(&mut paths);
    paths
}

pub fn resource_precedence_rank(metadata: &PathMetadata) -> u8 {
    if metadata.origin == SourceOrigin::Package {
        return 4;
    }
    let scope_base = if metadata.scope == SourceScope::Project {
        0
    } else {
        2
    };
    scope_base + u8::from(metadata.source != "local")
}

pub(super) fn merge_paths(target: &mut ResolvedPaths, next: ResolvedPaths) {
    target.extensions.extend(next.extensions);
    target.skills.extend(next.skills);
    target.prompts.extend(next.prompts);
    target.themes.extend(next.themes);
}

pub(super) fn sort_resolved_paths(paths: &mut ResolvedPaths) {
    for resources in [
        &mut paths.extensions,
        &mut paths.skills,
        &mut paths.prompts,
        &mut paths.themes,
    ] {
        resources.sort_by_key(|resource| resource_precedence_rank(&resource.metadata));
        dedupe_canonical_resources(resources);
    }
}

fn dedupe_canonical_resources(resources: &mut Vec<ResolvedResource>) {
    let mut seen = HashSet::new();
    resources.retain(|resource| {
        let canonical = fs::canonicalize(&resource.path)
            .unwrap_or_else(|_| PathBuf::from(resource.path.as_str()));
        seen.insert(canonical)
    });
}

fn resolve_single_file(
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    path: &Path,
) -> ResolvedPaths {
    let metadata = metadata(source, scope, origin, path.parent());
    let resource = ResolvedResource {
        path: display_path(path),
        enabled: true,
        metadata,
    };
    let mut paths = ResolvedPaths::default();
    match file_resource_type(path) {
        Some(ResourceType::Extension) => paths.extensions.push(resource),
        Some(ResourceType::Skill) => paths.skills.push(resource),
        Some(ResourceType::Prompt) => paths.prompts.push(resource),
        Some(ResourceType::Theme) => paths.themes.push(resource),
        None => {}
    }
    paths
}

fn collect_manifest_entries(
    source: &str,
    scope: SourceScope,
    base_dir: &Path,
    manifest: &PiManifest,
    filter: Option<&PackageFilter>,
    paths: &mut ResolvedPaths,
) {
    let filter_mode = filter.is_some();
    collect_manifest_entries_for_type(
        paths,
        ResourceType::Extension,
        source,
        scope,
        base_dir,
        manifest.extensions.as_deref(),
        filter.and_then(|f| f.extensions.as_deref()),
        filter_mode,
    );
    collect_manifest_entries_for_type(
        paths,
        ResourceType::Skill,
        source,
        scope,
        base_dir,
        manifest.skills.as_deref(),
        filter.and_then(|f| f.skills.as_deref()),
        filter_mode,
    );
    collect_manifest_entries_for_type(
        paths,
        ResourceType::Prompt,
        source,
        scope,
        base_dir,
        manifest.prompts.as_deref(),
        filter.and_then(|f| f.prompts.as_deref()),
        filter_mode,
    );
    collect_manifest_entries_for_type(
        paths,
        ResourceType::Theme,
        source,
        scope,
        base_dir,
        manifest.themes.as_deref(),
        filter.and_then(|f| f.themes.as_deref()),
        filter_mode,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_manifest_entries_for_type(
    paths: &mut ResolvedPaths,
    resource_type: ResourceType,
    source: &str,
    scope: SourceScope,
    base_dir: &Path,
    manifest_entries: Option<&[String]>,
    filter: Option<&[String]>,
    filter_mode: bool,
) {
    if let Some(filter) = filter {
        let candidates = manifest_entries
            .map(|entries| collect_manifest_enabled_files(entries, base_dir, resource_type))
            .unwrap_or_else(|| {
                collect_convention_candidates(base_dir, SourceOrigin::Package, resource_type)
            });
        let enabled = if filter.is_empty() {
            Default::default()
        } else {
            apply_patterns(&candidates, filter, base_dir)
        };
        for path in candidates {
            push_resource_with_enabled(
                paths,
                resource_type,
                source,
                scope,
                SourceOrigin::Package,
                base_dir,
                path.clone(),
                enabled.contains(&path),
            );
        }
        return;
    }

    if let Some(entries) = manifest_entries {
        for path in collect_manifest_enabled_files(entries, base_dir, resource_type) {
            push_resource_with_enabled(
                paths,
                resource_type,
                source,
                scope,
                SourceOrigin::Package,
                base_dir,
                path,
                true,
            );
        }
    } else if filter_mode {
        for path in collect_convention_candidates(base_dir, SourceOrigin::Package, resource_type) {
            push_resource_with_enabled(
                paths,
                resource_type,
                source,
                scope,
                SourceOrigin::Package,
                base_dir,
                path,
                true,
            );
        }
    }
}

fn collect_manifest_enabled_files(
    entries: &[String],
    base_dir: &Path,
    resource_type: ResourceType,
) -> Vec<PathBuf> {
    let candidates = collect_manifest_files_from_entries(entries, base_dir, resource_type);
    let override_patterns = entries
        .iter()
        .filter(|entry| is_override_pattern(entry))
        .cloned()
        .collect::<Vec<_>>();
    let enabled = apply_patterns(&candidates, &override_patterns, base_dir);
    candidates
        .into_iter()
        .filter(|path| enabled.contains(path))
        .collect()
}

fn collect_manifest_files_from_entries(
    entries: &[String],
    base_dir: &Path,
    resource_type: ResourceType,
) -> Vec<PathBuf> {
    let mut resolved = Vec::new();
    for entry in entries.iter().filter(|entry| !is_override_pattern(entry)) {
        if has_glob_pattern(entry) {
            resolved.extend(collect_manifest_glob_matches(base_dir, entry));
        } else {
            resolved.push(base_dir.join(entry));
        }
    }
    collect_files_from_paths(resolved, resource_type)
}

fn collect_auto_discovered_entries(
    paths: &mut ResolvedPaths,
    scope: SourceScope,
    base_dir: &Path,
    filter: Option<&PackageFilter>,
) {
    collect_auto_discovered_entries_for_type(
        paths,
        ResourceType::Extension,
        scope,
        base_dir,
        filter.and_then(|f| f.extensions.as_deref()),
    );
    collect_auto_discovered_entries_for_type(
        paths,
        ResourceType::Skill,
        scope,
        base_dir,
        filter.and_then(|f| f.skills.as_deref()),
    );
    collect_auto_discovered_entries_for_type(
        paths,
        ResourceType::Prompt,
        scope,
        base_dir,
        filter.and_then(|f| f.prompts.as_deref()),
    );
    collect_auto_discovered_entries_for_type(
        paths,
        ResourceType::Theme,
        scope,
        base_dir,
        filter.and_then(|f| f.themes.as_deref()),
    );
}

fn collect_auto_discovered_entries_for_type(
    paths: &mut ResolvedPaths,
    resource_type: ResourceType,
    scope: SourceScope,
    base_dir: &Path,
    filter: Option<&[String]>,
) {
    for path in collect_auto_discovered_candidates(base_dir, resource_type) {
        let enabled = filter
            .map(|patterns| is_enabled_by_overrides(&path, patterns, base_dir))
            .unwrap_or(true);
        push_resource_with_enabled(
            paths,
            resource_type,
            "auto",
            scope,
            SourceOrigin::TopLevel,
            base_dir,
            path,
            enabled,
        );
    }
}

fn collect_auto_discovered_candidates(
    base_dir: &Path,
    resource_type: ResourceType,
) -> Vec<PathBuf> {
    let resource_dir = base_dir.join(resource_type_dir(resource_type));
    if !resource_dir.exists() {
        return Vec::new();
    }
    match resource_type {
        ResourceType::Extension => collect_extensions(&resource_dir),
        ResourceType::Skill => collect_skill_entries(&resource_dir, SkillDiscoveryMode::Pi),
        ResourceType::Prompt | ResourceType::Theme => {
            collect_top_level_files(&resource_dir, resource_type)
        }
    }
}

fn collect_project_agents_skill_entries(
    paths: &mut ResolvedPaths,
    cwd: &Path,
    user_agents_skills_dir: Option<&Path>,
    filter: Option<&[String]>,
) {
    for skills_dir in collect_ancestor_agents_skill_dirs(cwd) {
        if user_agents_skills_dir
            .map(|user_dir| paths_equal(&skills_dir, user_dir))
            .unwrap_or(false)
        {
            continue;
        }
        let Some(base_dir) = skills_dir.parent() else {
            continue;
        };
        for path in collect_skill_entries(&skills_dir, SkillDiscoveryMode::Agents) {
            let enabled = filter
                .map(|patterns| is_enabled_by_overrides(&path, patterns, base_dir))
                .unwrap_or(true);
            push_resource_with_enabled(
                paths,
                ResourceType::Skill,
                "auto",
                SourceScope::Project,
                SourceOrigin::TopLevel,
                base_dir,
                path,
                enabled,
            );
        }
    }
}

fn collect_user_agents_skill_entries(
    paths: &mut ResolvedPaths,
    user_agents_dir: &Path,
    filter: Option<&[String]>,
) {
    for path in collect_skill_entries(&user_agents_dir.join("skills"), SkillDiscoveryMode::Agents) {
        let enabled = filter
            .map(|patterns| is_enabled_by_overrides(&path, patterns, user_agents_dir))
            .unwrap_or(true);
        push_resource_with_enabled(
            paths,
            ResourceType::Skill,
            "auto",
            SourceScope::User,
            SourceOrigin::TopLevel,
            user_agents_dir,
            path,
            enabled,
        );
    }
}

fn collect_ancestor_agents_skill_dirs(start_dir: &Path) -> Vec<PathBuf> {
    let start_dir = fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
    let git_repo_root = find_git_repo_root(&start_dir);
    let mut result = Vec::new();
    let mut dir = start_dir;

    loop {
        result.push(dir.join(".agents").join("skills"));
        if git_repo_root.as_ref() == Some(&dir) {
            break;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }

    result
}

fn find_git_repo_root(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        let parent = dir.parent()?;
        if parent == dir {
            return None;
        }
        dir = parent.to_path_buf();
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn collect_convention_entries(
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    base_dir: &Path,
    filter: Option<&PackageFilter>,
    paths: &mut ResolvedPaths,
) {
    collect_convention_entries_for_type(
        paths,
        ResourceType::Extension,
        source,
        scope,
        origin,
        base_dir,
        filter.and_then(|f| f.extensions.as_deref()),
    );
    collect_convention_entries_for_type(
        paths,
        ResourceType::Skill,
        source,
        scope,
        origin,
        base_dir,
        filter.and_then(|f| f.skills.as_deref()),
    );
    collect_convention_entries_for_type(
        paths,
        ResourceType::Prompt,
        source,
        scope,
        origin,
        base_dir,
        filter.and_then(|f| f.prompts.as_deref()),
    );
    collect_convention_entries_for_type(
        paths,
        ResourceType::Theme,
        source,
        scope,
        origin,
        base_dir,
        filter.and_then(|f| f.themes.as_deref()),
    );
}

fn collect_convention_entries_for_type(
    paths: &mut ResolvedPaths,
    resource_type: ResourceType,
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    base_dir: &Path,
    filter: Option<&[String]>,
) {
    let candidates = collect_convention_candidates(base_dir, origin, resource_type);
    if origin == SourceOrigin::Package {
        if let Some(filter) = filter {
            let enabled = if filter.is_empty() {
                Default::default()
            } else {
                apply_patterns(&candidates, filter, base_dir)
            };
            for path in candidates {
                push_resource_with_enabled(
                    paths,
                    resource_type,
                    source,
                    scope,
                    origin,
                    base_dir,
                    path.clone(),
                    enabled.contains(&path),
                );
            }
            return;
        }
        for path in candidates {
            push_resource_with_enabled(
                paths,
                resource_type,
                source,
                scope,
                origin,
                base_dir,
                path,
                true,
            );
        }
        return;
    }

    for path in filter_paths(candidates, filter, base_dir) {
        push_resource(
            paths,
            resource_type,
            source,
            scope,
            origin,
            base_dir,
            path,
            filter,
        );
    }
}

fn collect_convention_candidates(
    base_dir: &Path,
    origin: SourceOrigin,
    resource_type: ResourceType,
) -> Vec<PathBuf> {
    let resource_dir = if origin == SourceOrigin::Package {
        base_dir.join(resource_type_dir(resource_type))
    } else {
        base_dir.to_path_buf()
    };
    if !resource_dir.exists() {
        return Vec::new();
    }
    match resource_type {
        ResourceType::Extension => collect_extensions(&resource_dir),
        ResourceType::Skill => collect_skill_entries(&resource_dir, SkillDiscoveryMode::Pi),
        ResourceType::Prompt | ResourceType::Theme => collect_files(&resource_dir, resource_type),
    }
}

fn resource_type_dir(resource_type: ResourceType) -> &'static str {
    match resource_type {
        ResourceType::Extension => "extensions",
        ResourceType::Skill => "skills",
        ResourceType::Prompt => "prompts",
        ResourceType::Theme => "themes",
    }
}

fn collect_extensions(base_dir: &Path) -> Vec<PathBuf> {
    if let Some(entries) = resolve_extension_entries(base_dir) {
        return entries;
    }
    let mut result = Vec::new();
    let rules = load_ignore_rules(base_dir, base_dir);
    let Ok(entries) = fs::read_dir(base_dir) else {
        return result;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if ignored_entry(&path) || is_ignored_by_rules(&path, path.is_dir(), base_dir, &rules) {
            continue;
        }
        if path.is_file() && matches_extension(&path, ResourceType::Extension) {
            result.push(path);
        } else if path.is_dir() {
            if let Some(entries) = resolve_extension_entries(&path) {
                result.extend(entries);
            }
        }
    }
    result
}

fn collect_skill_entries(base_dir: &Path, mode: SkillDiscoveryMode) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut rules = Vec::new();
    collect_skill_entries_inner(base_dir, base_dir, mode, &mut rules, &mut result);
    result
}

fn collect_skill_entries_inner(
    dir: &Path,
    root_dir: &Path,
    mode: SkillDiscoveryMode,
    rules: &mut Vec<String>,
    result: &mut Vec<PathBuf>,
) {
    let initial_rule_len = rules.len();
    rules.extend(load_ignore_rules(dir, root_dir));
    let Ok(entries) = fs::read_dir(dir) else {
        rules.truncate(initial_rule_len);
        return;
    };
    let entries = entries.filter_map(Result::ok).collect::<Vec<_>>();

    for entry in &entries {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
            continue;
        }
        if path_is_file(&path) && !is_ignored_by_rules(&path, false, root_dir, rules) {
            result.push(path);
            rules.truncate(initial_rule_len);
            return;
        }
    }

    for entry in entries {
        let path = entry.path();
        if ignored_entry(&path) {
            continue;
        }
        let is_dir = path_is_dir(&path);
        let is_file = path_is_file(&path);
        if is_ignored_by_rules(&path, is_dir, root_dir, rules) {
            continue;
        }
        if mode == SkillDiscoveryMode::Pi
            && dir == root_dir
            && is_file
            && matches_extension(&path, ResourceType::Skill)
        {
            result.push(path);
            continue;
        }
        if is_dir {
            collect_skill_entries_inner(&path, root_dir, mode, rules, result);
        }
    }
    rules.truncate(initial_rule_len);
}

fn collect_files(base_dir: &Path, resource_type: ResourceType) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut rules = Vec::new();
    collect_files_inner(base_dir, base_dir, resource_type, &mut rules, &mut result);
    result
}

fn collect_top_level_files(base_dir: &Path, resource_type: ResourceType) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let rules = load_ignore_rules(base_dir, base_dir);
    let Ok(entries) = fs::read_dir(base_dir) else {
        return result;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if ignored_entry(&path) || is_ignored_by_rules(&path, false, base_dir, &rules) {
            continue;
        }
        if path.is_file() && matches_extension(&path, resource_type) {
            result.push(path);
        }
    }
    result
}

fn collect_files_inner(
    dir: &Path,
    root_dir: &Path,
    resource_type: ResourceType,
    rules: &mut Vec<String>,
    result: &mut Vec<PathBuf>,
) {
    let initial_rule_len = rules.len();
    rules.extend(load_ignore_rules(dir, root_dir));
    let Ok(entries) = fs::read_dir(dir) else {
        rules.truncate(initial_rule_len);
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if ignored_entry(&path) || is_ignored_by_rules(&path, path.is_dir(), root_dir, rules) {
            continue;
        }
        if path.is_dir() {
            collect_files_inner(&path, root_dir, resource_type, rules, result);
        } else if matches_extension(&path, resource_type) {
            if resource_type == ResourceType::Skill {
                if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
                    result.push(path);
                }
            } else {
                result.push(path);
            }
        }
    }
    rules.truncate(initial_rule_len);
}

fn path_is_dir(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

fn path_is_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn collect_manifest_glob_matches(base_dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let candidates = collect_manifest_glob_candidates(base_dir);
    let matched = apply_patterns(&candidates, &[pattern.to_string()], base_dir);
    candidates
        .into_iter()
        .filter(|path| matched.contains(path))
        .collect()
}

fn collect_manifest_glob_candidates(base_dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_manifest_glob_candidates_inner(base_dir, base_dir, &mut result);
    result
}

fn collect_manifest_glob_candidates_inner(dir: &Path, base_dir: &Path, result: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if hidden_entry(&path) {
            continue;
        }
        result.push(path.clone());
        if path.is_dir() && path != base_dir {
            collect_manifest_glob_candidates_inner(&path, base_dir, result);
        }
    }
}

fn collect_files_from_paths(paths: Vec<PathBuf>, resource_type: ResourceType) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        if path.is_file() {
            push_unique_path(&mut result, path);
        } else if path.is_dir() {
            for entry in match resource_type {
                ResourceType::Extension => collect_extensions(&path),
                ResourceType::Skill => collect_skill_entries(&path, SkillDiscoveryMode::Pi),
                ResourceType::Prompt | ResourceType::Theme => collect_files(&path, resource_type),
            } {
                push_unique_path(&mut result, entry);
            }
        }
    }
    result
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn is_override_pattern(entry: &str) -> bool {
    entry.starts_with('!') || entry.starts_with('+') || entry.starts_with('-')
}

fn has_glob_pattern(entry: &str) -> bool {
    entry.contains('*') || entry.contains('?')
}

fn push_resource(
    paths: &mut ResolvedPaths,
    resource_type: ResourceType,
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    base_dir: &Path,
    path: PathBuf,
    filter: Option<&[String]>,
) {
    let enabled = filter
        .map(|patterns| is_enabled_by_overrides(&path, patterns, base_dir))
        .unwrap_or(true);
    push_resource_with_enabled(
        paths,
        resource_type,
        source,
        scope,
        origin,
        base_dir,
        path,
        enabled,
    );
}

fn push_resource_with_enabled(
    paths: &mut ResolvedPaths,
    resource_type: ResourceType,
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    base_dir: &Path,
    path: PathBuf,
    enabled: bool,
) {
    let resource = ResolvedResource {
        path: display_path(&path),
        enabled,
        metadata: metadata(source, scope, origin, Some(base_dir)),
    };
    match resource_type {
        ResourceType::Extension => paths.extensions.push(resource),
        ResourceType::Skill => paths.skills.push(resource),
        ResourceType::Prompt => paths.prompts.push(resource),
        ResourceType::Theme => paths.themes.push(resource),
    }
}

fn metadata(
    source: &str,
    scope: SourceScope,
    origin: SourceOrigin,
    base_dir: Option<&Path>,
) -> PathMetadata {
    PathMetadata {
        source: if origin == SourceOrigin::TopLevel && local_source_path(source).is_some() {
            "local".to_string()
        } else {
            source.to_string()
        },
        scope,
        origin,
        base_dir: base_dir.map(display_path),
    }
}

fn ignored_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') || name == "node_modules")
}

fn hidden_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn manifest_globs_can_collect_node_modules_entries_like_pi_glob_sync() {
        let package_dir = temp_dir();
        let prompt = package_dir.join("node_modules").join("review.md");
        fs::create_dir_all(prompt.parent().expect("prompt parent")).expect("prompt dir");
        fs::write(&prompt, "").expect("prompt write");
        fs::write(
            package_dir.join("package.json"),
            r#"{"pi":{"prompts":["node_modules/*.md"]}}"#,
        )
        .expect("package json write");

        let resolved = resolve_source_at_path(
            "local-package",
            &package_dir,
            SourceScope::User,
            SourceOrigin::Package,
            None,
        );

        assert_eq!(resolved.prompts.len(), 1);
        assert_eq!(resolved.prompts[0].path, prompt.to_string_lossy());
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-resolver-test-{id}-{count}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
