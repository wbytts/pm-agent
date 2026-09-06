use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub content: String,
}

pub fn parse_changelog(path: impl AsRef<Path>) -> Vec<ChangelogEntry> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_changelog_content(&content)
}

pub fn compare_versions(left: &ChangelogEntry, right: &ChangelogEntry) -> i8 {
    match (
        left.major.cmp(&right.major),
        left.minor.cmp(&right.minor),
        left.patch.cmp(&right.patch),
    ) {
        (std::cmp::Ordering::Greater, _, _)
        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Greater, _)
        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Equal, std::cmp::Ordering::Greater) => 1,
        (std::cmp::Ordering::Less, _, _)
        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Less, _)
        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Equal, std::cmp::Ordering::Less) => -1,
        _ => 0,
    }
}

pub fn get_new_entries(entries: &[ChangelogEntry], last_version: &str) -> Vec<ChangelogEntry> {
    let last = parse_version(last_version).unwrap_or(ChangelogEntry {
        major: 0,
        minor: 0,
        patch: 0,
        content: String::new(),
    });
    entries
        .iter()
        .filter(|entry| compare_versions(entry, &last) > 0)
        .cloned()
        .collect()
}

fn parse_changelog_content(content: &str) -> Vec<ChangelogEntry> {
    let mut entries = Vec::new();
    let mut current_version: Option<ChangelogEntry> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            push_current_entry(&mut entries, current_version.take(), &mut current_lines);
            current_version = parse_header_version(line);
            if current_version.is_some() {
                current_lines.push(line.to_string());
            }
        } else if current_version.is_some() {
            current_lines.push(line.to_string());
        }
    }

    push_current_entry(&mut entries, current_version, &mut current_lines);
    entries
}

fn push_current_entry(
    entries: &mut Vec<ChangelogEntry>,
    version: Option<ChangelogEntry>,
    lines: &mut Vec<String>,
) {
    if let Some(mut entry) = version {
        if !lines.is_empty() {
            entry.content = lines.join("\n").trim().to_string();
            entries.push(entry);
        }
    }
    lines.clear();
}

fn parse_header_version(line: &str) -> Option<ChangelogEntry> {
    let rest = line.strip_prefix("## ")?.trim_start();
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    let version = rest
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .next()?;
    parse_version(version)
}

fn parse_version(version: &str) -> Option<ChangelogEntry> {
    let mut parts = version.split('.').map(str::parse::<u64>);
    Some(ChangelogEntry {
        major: parts.next()?.ok()?,
        minor: parts.next()?.ok()?,
        patch: parts.next()?.ok()?,
        content: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_changelog_entries_from_headers() {
        let entries = parse_changelog_content(
            "# Log\n\n## [1.2.0] - 2026-01-01\n- a\n\n## 1.1.0\n- b\n\n## bad\n- c",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].minor, 2);
        assert!(entries[0].content.contains("- a"));
        assert_eq!(entries[1].minor, 1);
    }

    #[test]
    fn filters_new_entries() {
        let entries = parse_changelog_content("## 1.2.0\n- a\n## 1.1.0\n- b\n");
        let newer = get_new_entries(&entries, "1.1.0");
        assert_eq!(newer.len(), 1);
        assert_eq!(newer[0].minor, 2);
    }

    #[test]
    fn compares_versions() {
        let a = parse_version("1.2.0").expect("version");
        let b = parse_version("1.1.9").expect("version");
        assert_eq!(compare_versions(&a, &b), 1);
        assert_eq!(compare_versions(&b, &a), -1);
        assert_eq!(compare_versions(&a, &a), 0);
    }
}
