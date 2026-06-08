use crate::package_manager::{
    PathMetadata, ResolvedPaths, ResolvedResource, SourceOrigin, SourceScope,
};
use crate::settings_manager::{SettingsManager, SettingsStorage, CONFIG_DIR_NAME};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigResourceType {
    Extension,
    Skill,
    Prompt,
    Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigResourceItem {
    pub path: String,
    pub enabled: bool,
    pub metadata: PathMetadata,
    pub resource_type: ConfigResourceType,
    pub display_name: String,
    pub group_key: String,
    pub subgroup_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigResourceSubgroup {
    pub resource_type: ConfigResourceType,
    pub label: String,
    pub items: Vec<ConfigResourceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigResourceGroup {
    pub key: String,
    pub label: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    pub source: String,
    pub subgroups: Vec<ConfigResourceSubgroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResourceEntry {
    Group {
        key: String,
        label: String,
    },
    Subgroup {
        group_key: String,
        subgroup_key: String,
        label: String,
        resource_type: ConfigResourceType,
    },
    Item(ConfigResourceItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigResourceListState {
    all_entries: Vec<ConfigResourceEntry>,
    filtered_entries: Vec<ConfigResourceEntry>,
    selected_index: usize,
}

impl ConfigResourceListState {
    pub fn new(groups: Vec<ConfigResourceGroup>) -> Self {
        let all_entries = flatten_config_resource_groups(&groups);
        let selected_index = first_item_index(&all_entries);
        Self {
            filtered_entries: all_entries.clone(),
            all_entries,
            selected_index,
        }
    }

    pub fn filtered_entries(&self) -> &[ConfigResourceEntry] {
        &self.filtered_entries
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_item(&self) -> Option<&ConfigResourceItem> {
        match self.filtered_entries.get(self.selected_index) {
            Some(ConfigResourceEntry::Item(item)) => Some(item),
            _ => None,
        }
    }

    pub fn filter(&mut self, query: &str) {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            self.filtered_entries = self.all_entries.clone();
            self.selected_index = first_item_index(&self.filtered_entries);
            return;
        }

        let mut matching_group_keys = Vec::<String>::new();
        let mut matching_subgroup_keys = Vec::<String>::new();
        let mut matching_item_keys = Vec::<(String, ConfigResourceType)>::new();
        for entry in &self.all_entries {
            let ConfigResourceEntry::Item(item) = entry else {
                continue;
            };
            if item.display_name.to_lowercase().contains(&query)
                || resource_type_key(item.resource_type).contains(&query)
                || item.path.to_lowercase().contains(&query)
            {
                push_unique(&mut matching_group_keys, item.group_key.clone());
                push_unique(&mut matching_subgroup_keys, item.subgroup_key.clone());
                push_unique(
                    &mut matching_item_keys,
                    (item.path.clone(), item.resource_type),
                );
            }
        }

        self.filtered_entries = self
            .all_entries
            .iter()
            .filter(|entry| match entry {
                ConfigResourceEntry::Group { key, .. } => matching_group_keys.contains(key),
                ConfigResourceEntry::Subgroup { subgroup_key, .. } => {
                    matching_subgroup_keys.contains(subgroup_key)
                }
                ConfigResourceEntry::Item(item) => {
                    matching_item_keys.contains(&(item.path.clone(), item.resource_type))
                }
            })
            .cloned()
            .collect();
        self.selected_index = first_item_index(&self.filtered_entries);
    }

    pub fn move_selection(&mut self, direction: isize) {
        if direction == 0 || self.filtered_entries.is_empty() {
            return;
        }

        let mut index = self.selected_index as isize + direction.signum();
        while index >= 0 && (index as usize) < self.filtered_entries.len() {
            if matches!(
                self.filtered_entries.get(index as usize),
                Some(ConfigResourceEntry::Item(_))
            ) {
                self.selected_index = index as usize;
                return;
            }
            index += direction.signum();
        }
    }

    pub fn page_selection(&mut self, amount: isize) {
        if amount == 0 || self.filtered_entries.is_empty() {
            return;
        }

        let mut target = (self.selected_index as isize + amount)
            .clamp(0, self.filtered_entries.len().saturating_sub(1) as isize)
            as usize;
        if amount > 0 {
            while target > 0
                && !matches!(
                    self.filtered_entries.get(target),
                    Some(ConfigResourceEntry::Item(_))
                )
            {
                target -= 1;
            }
        } else {
            while target < self.filtered_entries.len()
                && !matches!(
                    self.filtered_entries.get(target),
                    Some(ConfigResourceEntry::Item(_))
                )
            {
                target += 1;
            }
        }

        if matches!(
            self.filtered_entries.get(target),
            Some(ConfigResourceEntry::Item(_))
        ) {
            self.selected_index = target;
        }
    }

    pub fn set_selected_item_enabled(&mut self, enabled: bool) {
        let Some(selected_item) = self.selected_item() else {
            return;
        };
        let path = selected_item.path.clone();
        let resource_type = selected_item.resource_type;

        for entry in &mut self.all_entries {
            if let ConfigResourceEntry::Item(item) = entry {
                if item.path == path && item.resource_type == resource_type {
                    item.enabled = enabled;
                }
            }
        }
        for entry in &mut self.filtered_entries {
            if let ConfigResourceEntry::Item(item) = entry {
                if item.path == path && item.resource_type == resource_type {
                    item.enabled = enabled;
                }
            }
        }
    }

    pub fn render_lines(
        &self,
        search_value: &str,
        width: usize,
        max_visible: usize,
    ) -> Vec<String> {
        let mut lines = vec![truncate_ascii(search_value, width), String::new()];

        if self.filtered_entries.is_empty() {
            lines.push(truncate_ascii("  No resources found", width));
            return lines;
        }

        let max_visible = max_visible.max(1);
        let start_index = self.visible_start_index(max_visible);
        let end_index = (start_index + max_visible).min(self.filtered_entries.len());

        for index in start_index..end_index {
            let line = match &self.filtered_entries[index] {
                ConfigResourceEntry::Group { label, .. } => format!("  {label}"),
                ConfigResourceEntry::Subgroup { label, .. } => format!("    {label}"),
                ConfigResourceEntry::Item(item) => {
                    let cursor = if index == self.selected_index {
                        "> "
                    } else {
                        "  "
                    };
                    let checkbox = if item.enabled { "[x]" } else { "[ ]" };
                    format!("{cursor}    {checkbox} {}", item.display_name)
                }
            };
            lines.push(truncate_ascii(&line, width));
        }

        if start_index > 0 || end_index < self.filtered_entries.len() {
            let item_count = self
                .filtered_entries
                .iter()
                .filter(|entry| matches!(entry, ConfigResourceEntry::Item(_)))
                .count();
            let current_item_index = self
                .filtered_entries
                .iter()
                .take(self.selected_index)
                .filter(|entry| matches!(entry, ConfigResourceEntry::Item(_)))
                .count()
                + 1;
            lines.push(truncate_ascii(
                &format!("  ({current_item_index}/{item_count})"),
                width,
            ));
        }

        lines
    }

    fn visible_start_index(&self, max_visible: usize) -> usize {
        if self.filtered_entries.len() <= max_visible {
            return 0;
        }
        let centered = self.selected_index.saturating_sub(max_visible / 2);
        centered.min(self.filtered_entries.len() - max_visible)
    }
}

pub fn flatten_config_resource_groups(groups: &[ConfigResourceGroup]) -> Vec<ConfigResourceEntry> {
    let mut entries = Vec::new();
    for group in groups {
        entries.push(ConfigResourceEntry::Group {
            key: group.key.clone(),
            label: group.label.clone(),
        });
        for subgroup in &group.subgroups {
            let subgroup_key = format!(
                "{}:{}",
                group.key,
                resource_type_key(subgroup.resource_type)
            );
            entries.push(ConfigResourceEntry::Subgroup {
                group_key: group.key.clone(),
                subgroup_key,
                label: subgroup.label.clone(),
                resource_type: subgroup.resource_type,
            });
            for item in &subgroup.items {
                entries.push(ConfigResourceEntry::Item(item.clone()));
            }
        }
    }
    entries
}

pub fn config_resource_pattern(
    item: &ConfigResourceItem,
    cwd: impl AsRef<Path>,
    agent_dir: impl AsRef<Path>,
) -> String {
    let base_dir = match item.metadata.origin {
        SourceOrigin::TopLevel => item
            .metadata
            .base_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| top_level_base_dir(item.metadata.scope, cwd, agent_dir)),
        SourceOrigin::Package => item
            .metadata
            .base_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(&item.path)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default()
            }),
    };
    relative_pattern(&base_dir, Path::new(&item.path))
}

pub fn apply_config_resource_toggle_pattern(
    current: &[String],
    pattern: &str,
    enabled: bool,
) -> Vec<String> {
    let mut updated = current
        .iter()
        .filter(|value| strip_resource_pattern_prefix(value) != pattern)
        .cloned()
        .collect::<Vec<_>>();
    let prefix = if enabled { '+' } else { '-' };
    updated.push(format!("{prefix}{pattern}"));
    updated
}

pub fn apply_config_resource_toggle_to_settings<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    item: &ConfigResourceItem,
    enabled: bool,
    cwd: impl AsRef<Path>,
    agent_dir: impl AsRef<Path>,
) {
    let pattern = config_resource_pattern(item, cwd, agent_dir);
    match item.metadata.origin {
        SourceOrigin::TopLevel => {
            apply_top_level_resource_toggle(settings, item, &pattern, enabled);
        }
        SourceOrigin::Package => {
            apply_package_resource_toggle(settings, item, &pattern, enabled);
        }
    }
}

pub fn build_config_resource_groups(
    resolved: &ResolvedPaths,
    home_dir: Option<&str>,
) -> Vec<ConfigResourceGroup> {
    let mut groups = BTreeMap::<String, ConfigResourceGroup>::new();
    add_resources_to_groups(
        &mut groups,
        &resolved.extensions,
        ConfigResourceType::Extension,
        home_dir,
    );
    add_resources_to_groups(
        &mut groups,
        &resolved.skills,
        ConfigResourceType::Skill,
        home_dir,
    );
    add_resources_to_groups(
        &mut groups,
        &resolved.prompts,
        ConfigResourceType::Prompt,
        home_dir,
    );
    add_resources_to_groups(
        &mut groups,
        &resolved.themes,
        ConfigResourceType::Theme,
        home_dir,
    );

    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        group_origin_rank(left.origin)
            .cmp(&group_origin_rank(right.origin))
            .then(group_scope_rank(left.scope).cmp(&group_scope_rank(right.scope)))
            .then(left.source.cmp(&right.source))
    });

    for group in &mut groups {
        group
            .subgroups
            .sort_by_key(|subgroup| subgroup.resource_type);
        for subgroup in &mut group.subgroups {
            subgroup
                .items
                .sort_by(|left, right| left.display_name.cmp(&right.display_name));
        }
    }

    groups
}

fn add_resources_to_groups(
    groups: &mut BTreeMap<String, ConfigResourceGroup>,
    resources: &[ResolvedResource],
    resource_type: ConfigResourceType,
    home_dir: Option<&str>,
) {
    for resource in resources {
        let metadata = &resource.metadata;
        let group_key = group_key(metadata);
        let group = groups
            .entry(group_key.clone())
            .or_insert_with(|| ConfigResourceGroup {
                key: group_key.clone(),
                label: group_label(metadata, home_dir),
                scope: metadata.scope,
                origin: metadata.origin,
                source: metadata.source.clone(),
                subgroups: Vec::new(),
            });
        let subgroup_key = format!("{group_key}:{}", resource_type_key(resource_type));
        let subgroup_index = group
            .subgroups
            .iter()
            .position(|subgroup| subgroup.resource_type == resource_type)
            .unwrap_or_else(|| {
                group.subgroups.push(ConfigResourceSubgroup {
                    resource_type,
                    label: resource_type_label(resource_type).to_string(),
                    items: Vec::new(),
                });
                group.subgroups.len() - 1
            });
        group.subgroups[subgroup_index]
            .items
            .push(ConfigResourceItem {
                path: resource.path.clone(),
                enabled: resource.enabled,
                metadata: metadata.clone(),
                resource_type,
                display_name: display_name(&resource.path, resource_type),
                group_key: group_key.clone(),
                subgroup_key,
            });
    }
}

fn group_key(metadata: &PathMetadata) -> String {
    format!(
        "{}:{}:{}:{}",
        origin_key(metadata.origin),
        scope_key(metadata.scope),
        metadata.source,
        metadata.base_dir.as_deref().unwrap_or_default()
    )
}

fn group_label(metadata: &PathMetadata, home_dir: Option<&str>) -> String {
    if metadata.origin == SourceOrigin::Package {
        return format!("{} ({})", metadata.source, scope_key(metadata.scope));
    }

    if metadata.source == "auto" {
        if let Some(base_dir) = metadata.base_dir.as_deref() {
            return match metadata.scope {
                SourceScope::User => format!("User ({})", format_base_dir(base_dir, home_dir)),
                SourceScope::Project => {
                    format!("Project ({})", format_base_dir(base_dir, home_dir))
                }
                SourceScope::Temporary => {
                    format!("Temporary ({})", format_base_dir(base_dir, home_dir))
                }
            };
        }
        return match metadata.scope {
            SourceScope::User => "User (~/.pi/agent/)".to_string(),
            SourceScope::Project => "Project (.pi/)".to_string(),
            SourceScope::Temporary => "Temporary".to_string(),
        };
    }

    match metadata.scope {
        SourceScope::User => "User settings".to_string(),
        SourceScope::Project => "Project settings".to_string(),
        SourceScope::Temporary => "Temporary settings".to_string(),
    }
}

fn format_base_dir(base_dir: &str, home_dir: Option<&str>) -> String {
    let normalized = base_dir.replace('\\', "/");
    let display = if let Some(home_dir) = home_dir {
        let home_dir = home_dir.replace('\\', "/");
        if normalized == home_dir {
            "~".to_string()
        } else if let Some(rest) = normalized.strip_prefix(&home_dir) {
            format!("~{rest}")
        } else {
            normalized
        }
    } else {
        normalized
    };

    if display.ends_with('/') {
        display
    } else {
        format!("{display}/")
    }
}

fn display_name(path: &str, resource_type: ConfigResourceType) -> String {
    let path = Path::new(path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    match resource_type {
        ConfigResourceType::Extension if parent != "extensions" && !parent.is_empty() => {
            format!("{parent}/{file_name}")
        }
        ConfigResourceType::Skill if file_name == "SKILL.md" && !parent.is_empty() => parent,
        _ => file_name,
    }
}

fn resource_type_label(resource_type: ConfigResourceType) -> &'static str {
    match resource_type {
        ConfigResourceType::Extension => "Extensions",
        ConfigResourceType::Skill => "Skills",
        ConfigResourceType::Prompt => "Prompts",
        ConfigResourceType::Theme => "Themes",
    }
}

fn resource_type_key(resource_type: ConfigResourceType) -> &'static str {
    match resource_type {
        ConfigResourceType::Extension => "extensions",
        ConfigResourceType::Skill => "skills",
        ConfigResourceType::Prompt => "prompts",
        ConfigResourceType::Theme => "themes",
    }
}

fn scope_key(scope: SourceScope) -> &'static str {
    match scope {
        SourceScope::User => "user",
        SourceScope::Project => "project",
        SourceScope::Temporary => "temporary",
    }
}

fn origin_key(origin: SourceOrigin) -> &'static str {
    match origin {
        SourceOrigin::Package => "package",
        SourceOrigin::TopLevel => "top-level",
    }
}

fn group_origin_rank(origin: SourceOrigin) -> u8 {
    match origin {
        SourceOrigin::Package => 0,
        SourceOrigin::TopLevel => 1,
    }
}

fn group_scope_rank(scope: SourceScope) -> u8 {
    match scope {
        SourceScope::User => 0,
        SourceScope::Project => 1,
        SourceScope::Temporary => 2,
    }
}

fn apply_top_level_resource_toggle<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    item: &ConfigResourceItem,
    pattern: &str,
    enabled: bool,
) {
    let current = match item.metadata.scope {
        SourceScope::Project => project_paths(settings, item.resource_type),
        SourceScope::User | SourceScope::Temporary => global_paths(settings, item.resource_type),
    };
    let updated = apply_config_resource_toggle_pattern(&current, pattern, enabled);
    match (item.metadata.scope, item.resource_type) {
        (SourceScope::Project, ConfigResourceType::Extension) => {
            settings.set_project_extension_paths(updated)
        }
        (SourceScope::Project, ConfigResourceType::Skill) => {
            settings.set_project_skill_paths(updated)
        }
        (SourceScope::Project, ConfigResourceType::Prompt) => {
            settings.set_project_prompt_template_paths(updated)
        }
        (SourceScope::Project, ConfigResourceType::Theme) => {
            settings.set_project_theme_paths(updated)
        }
        (_, ConfigResourceType::Extension) => settings.set_extension_paths(updated),
        (_, ConfigResourceType::Skill) => settings.set_skill_paths(updated),
        (_, ConfigResourceType::Prompt) => settings.set_prompt_template_paths(updated),
        (_, ConfigResourceType::Theme) => settings.set_theme_paths(updated),
    }
}

fn apply_package_resource_toggle<S: SettingsStorage>(
    settings: &mut SettingsManager<S>,
    item: &ConfigResourceItem,
    pattern: &str,
    enabled: bool,
) {
    let mut packages = match item.metadata.scope {
        SourceScope::Project => settings.get_project_packages(),
        SourceScope::User | SourceScope::Temporary => settings.get_global_packages(),
    };
    let Some(index) = packages.iter().position(|package| {
        package_source(package).as_deref() == Some(item.metadata.source.as_str())
    }) else {
        return;
    };

    let mut object = match packages[index].clone() {
        Value::String(source) => {
            let mut object = Map::new();
            object.insert("source".to_string(), Value::String(source));
            object
        }
        Value::Object(object) => object,
        _ => return,
    };
    let key = resource_type_key(item.resource_type);
    let current = object
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let updated = apply_config_resource_toggle_pattern(&current, pattern, enabled);
    object.insert(
        key.to_string(),
        Value::Array(updated.into_iter().map(Value::String).collect()),
    );
    packages[index] = Value::Object(object);

    match item.metadata.scope {
        SourceScope::Project => settings.set_project_packages(packages),
        SourceScope::User | SourceScope::Temporary => settings.set_packages(packages),
    }
}

fn package_source(package: &Value) -> Option<String> {
    match package {
        Value::String(source) => Some(source.clone()),
        Value::Object(object) => object
            .get("source")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        _ => None,
    }
}

fn global_paths<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    resource_type: ConfigResourceType,
) -> Vec<String> {
    let scope = settings.global_settings();
    match resource_type {
        ConfigResourceType::Extension => scope.extensions.unwrap_or_default(),
        ConfigResourceType::Skill => scope.skills.unwrap_or_default(),
        ConfigResourceType::Prompt => scope.prompts.unwrap_or_default(),
        ConfigResourceType::Theme => scope.themes.unwrap_or_default(),
    }
}

fn project_paths<S: SettingsStorage>(
    settings: &SettingsManager<S>,
    resource_type: ConfigResourceType,
) -> Vec<String> {
    let scope = settings.project_settings();
    match resource_type {
        ConfigResourceType::Extension => scope.extensions.unwrap_or_default(),
        ConfigResourceType::Skill => scope.skills.unwrap_or_default(),
        ConfigResourceType::Prompt => scope.prompts.unwrap_or_default(),
        ConfigResourceType::Theme => scope.themes.unwrap_or_default(),
    }
}

fn top_level_base_dir(
    scope: SourceScope,
    cwd: impl AsRef<Path>,
    agent_dir: impl AsRef<Path>,
) -> PathBuf {
    match scope {
        SourceScope::Project => cwd.as_ref().join(CONFIG_DIR_NAME),
        SourceScope::User | SourceScope::Temporary => agent_dir.as_ref().to_path_buf(),
    }
}

fn relative_pattern(base_dir: &Path, path: &Path) -> String {
    path.strip_prefix(base_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn strip_resource_pattern_prefix(value: &str) -> &str {
    value
        .strip_prefix('!')
        .or_else(|| value.strip_prefix('+'))
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value)
}

fn first_item_index(entries: &[ConfigResourceEntry]) -> usize {
    entries
        .iter()
        .position(|entry| matches!(entry, ConfigResourceEntry::Item(_)))
        .unwrap_or(0)
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn truncate_ascii(value: &str, width: usize) -> String {
    if value.len() <= width {
        return value.to_string();
    }
    value.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use crate::config_selector::{
        apply_config_resource_toggle_pattern, apply_config_resource_toggle_to_settings,
        build_config_resource_groups, config_resource_pattern, ConfigResourceEntry,
        ConfigResourceItem, ConfigResourceListState, ConfigResourceType,
    };
    use crate::package_manager::{
        PathMetadata, ResolvedPaths, ResolvedResource, SourceOrigin, SourceScope,
    };
    use crate::settings_manager::{InMemorySettingsStorage, SettingsManager};
    use serde_json::json;

    #[test]
    fn builds_config_resource_groups_like_pi_selector() {
        let groups = build_config_resource_groups(&sample_resolved_paths(), Some("/Users/alice"));

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].label, "pkg-a (user)");
        assert_eq!(
            groups[0].subgroups[0].resource_type,
            ConfigResourceType::Extension
        );
        assert_eq!(groups[0].subgroups[0].items[0].display_name, "alpha.js");
        assert_eq!(
            groups[0].subgroups[1].resource_type,
            ConfigResourceType::Prompt
        );
        assert_eq!(groups[1].label, "User (~/.pi/agent/)");
        assert_eq!(groups[1].subgroups[0].items[0].display_name, "review");
        assert_eq!(groups[2].label, "Project (/workspace/.pi/agent/)");
        assert_eq!(
            groups[2].subgroups[0].items[0].display_name,
            "custom/main.js"
        );
    }

    #[test]
    fn config_resource_list_filters_with_headers_and_selects_first_item() {
        let groups = build_config_resource_groups(&sample_resolved_paths(), Some("/Users/alice"));
        let mut state = ConfigResourceListState::new(groups);

        state.filter("review");

        assert_eq!(state.filtered_entries().len(), 3);
        assert!(matches!(
            &state.filtered_entries()[0],
            ConfigResourceEntry::Group { label, .. } if label == "User (~/.pi/agent/)"
        ));
        assert!(matches!(
            &state.filtered_entries()[1],
            ConfigResourceEntry::Subgroup { resource_type, .. } if *resource_type == ConfigResourceType::Skill
        ));
        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("review")
        );
        assert_eq!(state.selected_index(), 2);
    }

    #[test]
    fn config_resource_list_moves_between_items_and_skips_headers() {
        let groups = build_config_resource_groups(&sample_resolved_paths(), Some("/Users/alice"));
        let mut state = ConfigResourceListState::new(groups);

        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("alpha.js")
        );
        state.move_selection(1);
        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("plan.md")
        );
        state.move_selection(-1);
        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("alpha.js")
        );
    }

    #[test]
    fn config_resource_list_pages_by_visible_window_and_skips_headers() {
        let groups = build_config_resource_groups(&many_resolved_paths(), Some("/Users/alice"));
        let mut state = ConfigResourceListState::new(groups);

        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("ext-0.js")
        );
        state.page_selection(4);
        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("ext-4.js")
        );
        state.page_selection(-4);
        assert_eq!(
            state.selected_item().map(|item| item.display_name.as_str()),
            Some("ext-0.js")
        );
    }

    #[test]
    fn config_resource_list_updates_enabled_state_for_selected_item() {
        let groups = build_config_resource_groups(&sample_resolved_paths(), Some("/Users/alice"));
        let mut state = ConfigResourceListState::new(groups);

        state.set_selected_item_enabled(false);

        assert_eq!(state.selected_item().map(|item| item.enabled), Some(false));
        assert!(matches!(
            state.filtered_entries().get(state.selected_index()),
            Some(ConfigResourceEntry::Item(item)) if item.display_name == "alpha.js" && !item.enabled
        ));
    }

    #[test]
    fn config_resource_list_renders_search_items_and_scroll_indicator() {
        let groups = build_config_resource_groups(&many_resolved_paths(), Some("/Users/alice"));
        let state = ConfigResourceListState::new(groups);

        let lines = state.render_lines("", 32, 4);

        assert_eq!(lines[0], "");
        assert!(lines.iter().any(|line| line == "  pkg-a (user)"));
        assert!(lines.iter().any(|line| line == "    Extensions"));
        assert!(lines.iter().any(|line| line == ">     [x] ext-0.js"));
        assert!(lines.iter().any(|line| line == "  (1/6)"));
    }

    #[test]
    fn config_resource_list_renders_no_resources_for_empty_filter() {
        let groups = build_config_resource_groups(&sample_resolved_paths(), Some("/Users/alice"));
        let mut state = ConfigResourceListState::new(groups);

        state.filter("missing");

        assert_eq!(
            state.render_lines("missing", 32, 5),
            vec![
                "missing".to_string(),
                "".to_string(),
                "  No resources found".to_string()
            ]
        );
    }

    #[test]
    fn config_resource_toggle_patterns_replace_existing_prefixes() {
        let updated = apply_config_resource_toggle_pattern(
            &[
                "+extensions/old.js".to_string(),
                "!skills/review/SKILL.md".to_string(),
                "-skills/review/SKILL.md".to_string(),
            ],
            "skills/review/SKILL.md",
            true,
        );

        assert_eq!(
            updated,
            vec![
                "+extensions/old.js".to_string(),
                "+skills/review/SKILL.md".to_string()
            ]
        );
    }

    #[test]
    fn config_resource_patterns_match_top_level_and_package_rules() {
        let top_level = ConfigResourceItem {
            path: "/workspace/.pm-agent/skills/review/SKILL.md".to_string(),
            enabled: true,
            metadata: PathMetadata {
                source: "auto".to_string(),
                scope: SourceScope::Project,
                origin: SourceOrigin::TopLevel,
                base_dir: None,
            },
            resource_type: ConfigResourceType::Skill,
            display_name: "review".to_string(),
            group_key: "top-level:project:auto:".to_string(),
            subgroup_key: "top-level:project:auto::skills".to_string(),
        };
        let package = ConfigResourceItem {
            path: "/pkg/extensions/custom/main.js".to_string(),
            enabled: true,
            metadata: PathMetadata {
                source: "pkg-a".to_string(),
                scope: SourceScope::User,
                origin: SourceOrigin::Package,
                base_dir: Some("/pkg".to_string()),
            },
            resource_type: ConfigResourceType::Extension,
            display_name: "custom/main.js".to_string(),
            group_key: "package:user:pkg-a:/pkg".to_string(),
            subgroup_key: "package:user:pkg-a:/pkg:extensions".to_string(),
        };

        assert_eq!(
            config_resource_pattern(&top_level, "/workspace", "/Users/alice/.pm-agent/agent"),
            "skills/review/SKILL.md"
        );
        assert_eq!(
            config_resource_pattern(&package, "/workspace", "/Users/alice/.pm-agent/agent"),
            "extensions/custom/main.js"
        );
    }

    #[test]
    fn config_resource_toggle_writes_top_level_settings_scope() {
        let storage = InMemorySettingsStorage::new();
        let mut settings = SettingsManager::from_storage(storage);
        let item = ConfigResourceItem {
            path: "/Users/alice/.pm-agent/agent/skills/review/SKILL.md".to_string(),
            enabled: false,
            metadata: PathMetadata {
                source: "auto".to_string(),
                scope: SourceScope::User,
                origin: SourceOrigin::TopLevel,
                base_dir: Some("/Users/alice/.pm-agent/agent".to_string()),
            },
            resource_type: ConfigResourceType::Skill,
            display_name: "review".to_string(),
            group_key: "top-level:user:auto:/Users/alice/.pm-agent/agent".to_string(),
            subgroup_key: "top-level:user:auto:/Users/alice/.pm-agent/agent:skills".to_string(),
        };

        apply_config_resource_toggle_to_settings(
            &mut settings,
            &item,
            true,
            "/workspace",
            "/Users/alice/.pm-agent/agent",
        );

        assert_eq!(
            settings.global_settings().skills,
            Some(vec!["+skills/review/SKILL.md".to_string()])
        );
        assert_eq!(settings.project_settings().skills, None);
    }

    #[test]
    fn config_resource_toggle_writes_package_filters_and_converts_string_source() {
        let mut settings = SettingsManager::in_memory(json!({
            "packages": ["pkg-a"]
        }));
        let item = ConfigResourceItem {
            path: "/pkg/extensions/custom/main.js".to_string(),
            enabled: true,
            metadata: PathMetadata {
                source: "pkg-a".to_string(),
                scope: SourceScope::User,
                origin: SourceOrigin::Package,
                base_dir: Some("/pkg".to_string()),
            },
            resource_type: ConfigResourceType::Extension,
            display_name: "custom/main.js".to_string(),
            group_key: "package:user:pkg-a:/pkg".to_string(),
            subgroup_key: "package:user:pkg-a:/pkg:extensions".to_string(),
        };

        apply_config_resource_toggle_to_settings(
            &mut settings,
            &item,
            false,
            "/workspace",
            "/Users/alice/.pm-agent/agent",
        );

        assert_eq!(
            settings.global_settings().packages,
            Some(vec![json!({
                "source": "pkg-a",
                "extensions": ["-extensions/custom/main.js"]
            })])
        );
    }

    fn sample_resolved_paths() -> ResolvedPaths {
        ResolvedPaths {
            extensions: vec![
                resource(
                    "/pkg/extensions/alpha.js",
                    true,
                    "pkg-a",
                    SourceScope::User,
                    SourceOrigin::Package,
                    Some("/pkg"),
                ),
                resource(
                    "/workspace/.pi/extensions/custom/main.js",
                    false,
                    "auto",
                    SourceScope::Project,
                    SourceOrigin::TopLevel,
                    Some("/workspace/.pi/agent"),
                ),
            ],
            skills: vec![resource(
                "/Users/alice/.pi/agent/skills/review/SKILL.md",
                true,
                "auto",
                SourceScope::User,
                SourceOrigin::TopLevel,
                Some("/Users/alice/.pi/agent"),
            )],
            prompts: vec![resource(
                "/pkg/prompts/plan.md",
                true,
                "pkg-a",
                SourceScope::User,
                SourceOrigin::Package,
                Some("/pkg"),
            )],
            themes: Vec::new(),
        }
    }

    fn many_resolved_paths() -> ResolvedPaths {
        let mut resolved = ResolvedPaths {
            extensions: Vec::new(),
            skills: Vec::new(),
            prompts: Vec::new(),
            themes: Vec::new(),
        };
        for index in 0..6 {
            resolved.extensions.push(resource(
                &format!("/pkg/extensions/ext-{index}.js"),
                true,
                "pkg-a",
                SourceScope::User,
                SourceOrigin::Package,
                Some("/pkg"),
            ));
        }
        resolved
    }

    fn resource(
        path: &str,
        enabled: bool,
        source: &str,
        scope: SourceScope,
        origin: SourceOrigin,
        base_dir: Option<&str>,
    ) -> ResolvedResource {
        ResolvedResource {
            path: path.to_string(),
            enabled,
            metadata: PathMetadata {
                source: source.to_string(),
                scope,
                origin,
                base_dir: base_dir.map(str::to_string),
            },
        }
    }
}
