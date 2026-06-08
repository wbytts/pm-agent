use std::fs;
use std::path::Path;

use crate::tools::truncate::{format_size, truncate_head, TruncationOptions, DEFAULT_MAX_BYTES};
use crate::types::{CodingAgentResult, CodingToolResult};

pub fn success(output: impl Into<String>) -> CodingAgentResult<CodingToolResult> {
    Ok(CodingToolResult {
        success: true,
        output: output.into(),
        details: None,
        content: None,
    })
}

pub fn command_result(
    success: bool,
    output: impl Into<String>,
) -> CodingAgentResult<CodingToolResult> {
    Ok(CodingToolResult {
        success,
        output: output.into(),
        details: None,
        content: None,
    })
}

pub fn ignored_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".git" || name == "node_modules")
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreMatcher {
    rules: Vec<IgnoreRule>,
}

#[derive(Debug, Clone)]
struct IgnoreRule {
    pattern: String,
    negated: bool,
    directory_only: bool,
}

impl IgnoreMatcher {
    pub fn load(root: &Path) -> Self {
        let mut matcher = Self::default();
        matcher.load_from(root);
        matcher
    }

    pub fn load_from(&mut self, dir: &Path) {
        let path = dir.join(".gitignore");
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            if let Some(rule) = parse_ignore_rule(line) {
                self.rules.push(rule);
            }
        }
    }

    pub fn is_ignored(&self, root: &Path, path: &Path) -> bool {
        if ignored_path(path) {
            return true;
        }
        let relative = relative_display(root, path);
        let is_dir = path.is_dir();
        let mut ignored = false;
        for rule in &self.rules {
            if rule.matches(&relative, path, is_dir) {
                ignored = !rule.negated;
            }
        }
        ignored
    }
}

pub fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn parse_ignore_rule(line: &str) -> Option<IgnoreRule> {
    let mut value = line.trim();
    if value.is_empty() || value.starts_with('#') {
        return None;
    }
    let negated = value.starts_with('!');
    if negated {
        value = value.trim_start_matches('!');
    }
    value = value.trim_start_matches('/');
    let directory_only = value.ends_with('/');
    value = value.trim_end_matches('/');
    (!value.is_empty()).then(|| IgnoreRule {
        pattern: value.to_string(),
        negated,
        directory_only,
    })
}

impl IgnoreRule {
    fn matches(&self, relative: &str, path: &Path, is_dir: bool) -> bool {
        if self.directory_only && !is_dir && !relative.starts_with(&format!("{}/", self.pattern)) {
            return false;
        }

        if self.pattern.contains('/') {
            return glob_match(relative, &self.pattern)
                || relative.starts_with(&format!("{}/", self.pattern));
        }

        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| glob_match(name, &self.pattern))
            || relative
                .split('/')
                .any(|segment| glob_match(segment, &self.pattern))
    }
}

pub fn normalize_glob(pattern: &str) -> String {
    if pattern.contains('/') || pattern.starts_with('*') {
        pattern.to_string()
    } else {
        format!("**/{pattern}")
    }
}

pub fn glob_match(value: &str, pattern: &str) -> bool {
    let value = value.chars().collect::<Vec<_>>();
    let pattern = normalize_glob(pattern).chars().collect::<Vec<_>>();
    glob_match_chars(&value, &pattern)
}

#[cfg(test)]
pub fn collect_temp_workspace(prefix: &str) -> std::path::PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{id}"))
}

pub fn truncate_list_output(raw_output: &str, mut notices: Vec<String>) -> String {
    let truncation = truncate_head(
        raw_output,
        TruncationOptions {
            max_lines: usize::MAX,
            max_bytes: DEFAULT_MAX_BYTES,
        },
    );
    if truncation.truncated {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }

    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    output
}

fn glob_match_chars(value: &[char], pattern: &[char]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    if pattern.starts_with(&['*', '*', '/']) {
        return glob_match_chars(value, &pattern[3..])
            || (!value.is_empty() && glob_match_chars(&value[1..], pattern));
    }
    match pattern[0] {
        '*' => {
            glob_match_chars(value, &pattern[1..])
                || (!value.is_empty() && value[0] != '/' && glob_match_chars(&value[1..], pattern))
        }
        '?' => !value.is_empty() && value[0] != '/' && glob_match_chars(&value[1..], &pattern[1..]),
        character => {
            !value.is_empty()
                && value[0] == character
                && glob_match_chars(&value[1..], &pattern[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pi_like_default_glob_scope() {
        assert!(glob_match("main.rs", "*.rs"));
        assert!(glob_match("src/main.rs", "src/*.rs"));
        assert!(!glob_match("src/main.ts", "*.rs"));
    }
}
