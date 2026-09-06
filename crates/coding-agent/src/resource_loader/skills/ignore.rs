use std::fs;
use std::path::Path;

const IGNORE_FILE_NAMES: &[&str] = &[".gitignore", ".ignore", ".fdignore"];

#[derive(Default)]
pub struct IgnoreRules {
    patterns: Vec<String>,
}

impl IgnoreRules {
    pub fn add_rules_from_dir(&mut self, dir: &Path, root: &Path) {
        let relative_dir = dir.strip_prefix(root).unwrap_or(dir);
        let prefix = if relative_dir.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", to_posix_path(relative_dir))
        };

        for filename in IGNORE_FILE_NAMES {
            let ignore_path = dir.join(filename);
            let Ok(content) = fs::read_to_string(ignore_path) else {
                continue;
            };
            for line in content.lines() {
                if let Some(pattern) = prefix_ignore_pattern(line, &prefix) {
                    self.patterns.push(pattern);
                }
            }
        }
    }

    pub fn ignores(&self, root: &Path, path: &Path, is_dir: bool) -> bool {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let mut rel = to_posix_path(rel);
        if is_dir && !rel.ends_with('/') {
            rel.push('/');
        }
        let mut ignored = false;
        for pattern in &self.patterns {
            let (negated, pattern) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |rest| (true, rest));
            if pattern_matches(pattern, &rel) {
                ignored = !negated;
            }
        }
        ignored
    }

    pub fn may_contain_negated_match(&self, root: &Path, dir: &Path) -> bool {
        let rel = dir.strip_prefix(root).unwrap_or(dir);
        let mut rel = to_posix_path(rel);
        if !rel.ends_with('/') {
            rel.push('/');
        }
        self.patterns.iter().any(|pattern| {
            pattern
                .strip_prefix('!')
                .is_some_and(|pattern| pattern.starts_with(&rel))
        })
    }
}

fn prefix_ignore_pattern(line: &str, prefix: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("\\#")) {
        return None;
    }

    let mut pattern = line.trim().to_string();
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

fn pattern_matches(pattern: &str, rel: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    let rel = rel.trim_end_matches('/');
    path_pattern_matches(pattern, rel)
        || rel.starts_with(&format!("{pattern}/"))
        || rel
            .rsplit('/')
            .next()
            .is_some_and(|file_name| path_pattern_matches(pattern, file_name))
}

fn path_pattern_matches(pattern: &str, value: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return value == pattern;
    }
    glob_matches(pattern.as_bytes(), value.as_bytes())
}

fn glob_matches(pattern: &[u8], value: &[u8]) -> bool {
    let (mut pattern_index, mut value_index) = (0, 0);
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && pattern[pattern_index] != b'*'
            && pattern[pattern_index] != b'?'
            && pattern[pattern_index] == value[value_index]
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'?' {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn to_posix_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
