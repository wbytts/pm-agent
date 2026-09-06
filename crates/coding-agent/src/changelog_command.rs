use serde::{Deserialize, Serialize};

use crate::utils::{changelog_path, parse_changelog, AppConfigPaths, ChangelogEntry};

pub const NO_CHANGELOG_ENTRIES: &str = "No changelog entries found.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogSummary {
    pub title: String,
    pub markdown: String,
    pub entries: Vec<ChangelogEntry>,
}

pub fn changelog_summary(config: &AppConfigPaths) -> ChangelogSummary {
    changelog_summary_from_entries(parse_changelog(changelog_path(config)))
}

pub fn changelog_summary_from_entries(entries: Vec<ChangelogEntry>) -> ChangelogSummary {
    let markdown = if entries.is_empty() {
        NO_CHANGELOG_ENTRIES.to_string()
    } else {
        entries
            .iter()
            .rev()
            .map(|entry| entry.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    ChangelogSummary {
        title: "What's New".to_string(),
        markdown,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn builds_changelog_markdown_like_pi_changelog_command() {
        let summary = changelog_summary_from_entries(vec![
            entry(1, 0, 0, "## 1.0.0\n- older"),
            entry(1, 1, 0, "## 1.1.0\n- newer"),
        ]);

        assert_eq!(summary.title, "What's New");
        assert_eq!(summary.markdown, "## 1.1.0\n- newer\n\n## 1.0.0\n- older");
        assert_eq!(summary.entries.len(), 2);
    }

    #[test]
    fn returns_pi_empty_message_without_entries() {
        let summary = changelog_summary_from_entries(Vec::new());

        assert_eq!(summary.markdown, NO_CHANGELOG_ENTRIES);
        assert!(summary.entries.is_empty());
    }

    #[test]
    fn reads_changelog_from_app_config_path() {
        let dir = temp_dir();
        fs::write(
            dir.join("CHANGELOG.md"),
            "# Changelog\n\n## 0.1.0\n- first\n\n## 0.2.0\n- second\n",
        )
        .expect("changelog should write");
        let mut config = AppConfigPaths::new("/home/alice");
        config.package_dir = dir;

        let summary = changelog_summary(&config);

        assert_eq!(summary.markdown, "## 0.2.0\n- second\n\n## 0.1.0\n- first");
    }

    fn entry(major: u64, minor: u64, patch: u64, content: &str) -> ChangelogEntry {
        ChangelogEntry {
            major,
            minor,
            patch,
            content: content.to_string(),
        }
    }

    fn temp_dir() -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-changelog-command-{id}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
