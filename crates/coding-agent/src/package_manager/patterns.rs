use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const IGNORE_FILE_NAMES: &[&str] = &[".gitignore", ".ignore", ".fdignore"];

pub(super) fn apply_patterns(
    all_paths: &[PathBuf],
    patterns: &[String],
    base_dir: &Path,
) -> HashSet<PathBuf> {
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    let mut force_includes = Vec::new();
    let mut force_excludes = Vec::new();

    for pattern in patterns {
        if let Some(pattern) = pattern.strip_prefix('+') {
            force_includes.push(pattern.to_string());
        } else if let Some(pattern) = pattern.strip_prefix('-') {
            force_excludes.push(pattern.to_string());
        } else if let Some(pattern) = pattern.strip_prefix('!') {
            excludes.push(pattern.to_string());
        } else {
            includes.push(pattern.to_string());
        }
    }

    let mut result = if includes.is_empty() {
        all_paths.to_vec()
    } else {
        all_paths
            .iter()
            .filter(|path| matches_any_pattern(path, &includes, base_dir))
            .cloned()
            .collect::<Vec<_>>()
    };

    if !excludes.is_empty() {
        result.retain(|path| !matches_any_pattern(path, &excludes, base_dir));
    }
    if !force_includes.is_empty() {
        for path in all_paths {
            if !result.contains(path) && matches_any_exact_pattern(path, &force_includes, base_dir)
            {
                result.push(path.clone());
            }
        }
    }
    if !force_excludes.is_empty() {
        result.retain(|path| !matches_any_exact_pattern(path, &force_excludes, base_dir));
    }

    result.into_iter().collect()
}

pub(super) fn is_enabled_by_overrides(path: &Path, patterns: &[String], base_dir: &Path) -> bool {
    let excludes = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('!').map(ToString::to_string))
        .collect::<Vec<_>>();
    let force_includes = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('+').map(ToString::to_string))
        .collect::<Vec<_>>();
    let force_excludes = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('-').map(ToString::to_string))
        .collect::<Vec<_>>();

    let mut enabled = true;
    if !excludes.is_empty() && matches_any_pattern(path, &excludes, base_dir) {
        enabled = false;
    }
    if !force_includes.is_empty() && matches_any_exact_pattern(path, &force_includes, base_dir) {
        enabled = true;
    }
    if !force_excludes.is_empty() && matches_any_exact_pattern(path, &force_excludes, base_dir) {
        enabled = false;
    }
    enabled
}

pub(super) fn filter_paths(
    paths: Vec<PathBuf>,
    patterns: Option<&[String]>,
    base_dir: &Path,
) -> Vec<PathBuf> {
    let Some(patterns) = patterns else {
        return paths;
    };
    if patterns.is_empty() {
        return Vec::new();
    }
    if patterns.iter().any(|pattern| is_pattern(pattern)) {
        let enabled = apply_patterns(&paths, patterns, base_dir);
        return paths
            .into_iter()
            .filter(|path| enabled.contains(path))
            .collect();
    }
    paths
        .into_iter()
        .filter(|path| matches_any_exact_pattern(path, patterns, base_dir))
        .collect()
}

pub(super) fn load_ignore_rules(dir: &Path, root_dir: &Path) -> Vec<String> {
    let prefix = dir
        .strip_prefix(root_dir)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| format!("{}/", to_posix(relative)))
        .unwrap_or_default();

    let mut patterns = Vec::new();
    for filename in IGNORE_FILE_NAMES {
        let path = dir.join(filename);
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            if let Some(pattern) = prefix_ignore_pattern(line, &prefix) {
                patterns.push(pattern);
            }
        }
    }
    patterns
}

pub(super) fn is_ignored_by_rules(
    path: &Path,
    is_dir: bool,
    root_dir: &Path,
    rules: &[String],
) -> bool {
    let rel = path
        .strip_prefix(root_dir)
        .map(to_posix)
        .unwrap_or_else(|_| to_posix(path));
    let ignore_path = if is_dir { format!("{rel}/") } else { rel };
    let mut ignored = false;
    for rule in rules {
        if let Some(rule) = rule.strip_prefix('!') {
            if glob_match(&ignore_path, rule) || glob_match(&path_name(path), rule) {
                ignored = false;
            }
        } else if glob_match(&ignore_path, rule) || glob_match(&path_name(path), rule) {
            ignored = true;
        }
    }
    ignored
}

fn matches_any_pattern(path: &Path, patterns: &[String], base_dir: &Path) -> bool {
    let rel = path
        .strip_prefix(base_dir)
        .map(to_posix)
        .unwrap_or_else(|_| to_posix(path));
    let name = path_name(path);
    let full = to_posix(path);
    let skill_parent = skill_parent_match_values(path, base_dir);

    patterns.iter().any(|pattern| {
        let normalized = normalize_slashes(pattern);
        glob_match(&rel, &normalized)
            || glob_match(&name, &normalized)
            || glob_match(&full, &normalized)
            || skill_parent
                .iter()
                .any(|value| glob_match(value, &normalized))
    })
}

fn matches_any_exact_pattern(path: &Path, patterns: &[String], base_dir: &Path) -> bool {
    let rel = path
        .strip_prefix(base_dir)
        .map(to_posix)
        .unwrap_or_else(|_| to_posix(path));
    let full = to_posix(path);
    let skill_parent = skill_parent_match_values(path, base_dir);

    patterns.iter().any(|pattern| {
        let normalized = normalize_exact_pattern(pattern);
        normalized == rel
            || normalized == full
            || skill_parent.iter().any(|value| *value == normalized)
    })
}

fn skill_parent_match_values(path: &Path, base_dir: &Path) -> Vec<String> {
    if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
        return Vec::new();
    }
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    vec![
        parent
            .strip_prefix(base_dir)
            .map(to_posix)
            .unwrap_or_else(|_| to_posix(parent)),
        path_name(parent),
        to_posix(parent),
    ]
}

fn is_pattern(value: &str) -> bool {
    value.starts_with('!')
        || value.starts_with('+')
        || value.starts_with('-')
        || value.contains('*')
        || value.contains('?')
}

fn normalize_exact_pattern(pattern: &str) -> String {
    let pattern = pattern
        .strip_prefix("./")
        .or_else(|| pattern.strip_prefix(".\\"))
        .unwrap_or(pattern);
    normalize_slashes(pattern)
}

fn normalize_slashes(value: &str) -> String {
    value.replace('\\', "/")
}

fn prefix_ignore_pattern(line: &str, prefix: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("\\#")) {
        return None;
    }

    let mut pattern = line.to_string();
    let mut negated = false;
    if let Some(rest) = pattern.strip_prefix('!') {
        negated = true;
        pattern = rest.to_string();
    } else if let Some(rest) = pattern.strip_prefix("\\!") {
        pattern = rest.to_string();
    }
    if let Some(rest) = pattern.strip_prefix('/') {
        pattern = rest.to_string();
    }

    let prefixed = if prefix.is_empty() {
        pattern
    } else {
        format!("{prefix}{pattern}")
    };
    Some(if negated {
        format!("!{prefixed}")
    } else {
        prefixed
    })
}

fn glob_match(value: &str, pattern: &str) -> bool {
    glob_match_chars(
        &value.chars().collect::<Vec<_>>(),
        &pattern.chars().collect::<Vec<_>>(),
    )
}

fn glob_match_chars(value: &[char], pattern: &[char]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    if pattern[0] == '[' {
        if let Some(end) = pattern.iter().position(|ch| *ch == ']') {
            if !value.is_empty() && glob_character_class_matches(value[0], &pattern[1..end]) {
                return glob_match_chars(&value[1..], &pattern[end + 1..]);
            }
            return false;
        }
    }
    match pattern[0] {
        '*' => {
            glob_match_chars(value, &pattern[1..])
                || (!value.is_empty() && glob_match_chars(&value[1..], pattern))
        }
        '?' => !value.is_empty() && glob_match_chars(&value[1..], &pattern[1..]),
        character => {
            !value.is_empty()
                && value[0] == character
                && glob_match_chars(&value[1..], &pattern[1..])
        }
    }
}

fn glob_character_class_matches(value: char, class: &[char]) -> bool {
    if class.is_empty() {
        return false;
    }
    let negated = class[0] == '!' || class[0] == '^';
    let class = if negated { &class[1..] } else { class };
    let mut matched = false;
    let mut index = 0;
    while index < class.len() {
        if index + 2 < class.len() && class[index + 1] == '-' {
            if class[index] <= value && value <= class[index + 2] {
                matched = true;
            }
            index += 3;
        } else {
            if class[index] == value {
                matched = true;
            }
            index += 1;
        }
    }
    if negated {
        !matched
    } else {
        matched
    }
}

fn path_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn to_posix(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_include_exclude_and_force_patterns() {
        let base = PathBuf::from("/repo");
        let paths = vec![
            base.join("a.md"),
            base.join("draft.md"),
            base.join("keep.md"),
            base.join("nested").join("force.md"),
        ];
        let enabled = apply_patterns(
            &paths,
            &[
                "*.md".to_string(),
                "!draft.md".to_string(),
                "+nested/force.md".to_string(),
                "-keep.md".to_string(),
            ],
            &base,
        );

        assert!(enabled.contains(&base.join("a.md")));
        assert!(enabled.contains(&base.join("nested").join("force.md")));
        assert!(!enabled.contains(&base.join("draft.md")));
        assert!(!enabled.contains(&base.join("keep.md")));
    }

    #[test]
    fn matches_minimatch_globstar_and_character_classes_like_pi_filters() {
        let base = PathBuf::from("/repo");
        let paths = vec![
            base.join("a.md"),
            base.join("nested").join("b.md"),
            base.join("nested").join("c.txt"),
        ];

        let globstar = apply_patterns(&paths, &["**/*.md".to_string()], &base);
        assert!(globstar.contains(&base.join("a.md")));
        assert!(globstar.contains(&base.join("nested").join("b.md")));
        assert!(!globstar.contains(&base.join("nested").join("c.txt")));

        let class = apply_patterns(&paths, &["nested/[bc].*".to_string()], &base);
        assert!(class.contains(&base.join("nested").join("b.md")));
        assert!(class.contains(&base.join("nested").join("c.txt")));

        let range = apply_patterns(&paths, &["nested/[a-c].*".to_string()], &base);
        assert!(range.contains(&base.join("nested").join("b.md")));
        assert!(range.contains(&base.join("nested").join("c.txt")));

        let negated = apply_patterns(&paths, &["nested/[!b].*".to_string()], &base);
        assert!(!negated.contains(&base.join("nested").join("b.md")));
        assert!(negated.contains(&base.join("nested").join("c.txt")));
    }

    #[test]
    fn pattern_matching_is_case_sensitive_like_pi_minimatch_defaults() {
        let base = PathBuf::from("/repo");
        let paths = vec![base.join("a.md"), base.join("A.md")];

        let enabled = apply_patterns(&paths, &["A.md".to_string()], &base);

        assert!(!enabled.contains(&base.join("a.md")));
        assert!(enabled.contains(&base.join("A.md")));
    }
}
