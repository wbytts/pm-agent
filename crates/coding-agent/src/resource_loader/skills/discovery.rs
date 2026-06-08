use agent::harness::Skill;
use std::fs;
use std::path::Path;

use super::diagnostics::warning_diagnostic;
use super::ignore::IgnoreRules;
use super::load_skill_file;
use crate::diagnostics::ResourceDiagnostic;

pub fn load_skills_from_dir(dir: &Path) -> (Vec<Skill>, Vec<ResourceDiagnostic>) {
    if !dir.exists() {
        return (Vec::new(), Vec::new());
    }

    let mut skills = Vec::new();
    let mut diagnostics = Vec::new();
    let mut ignore_rules = IgnoreRules::default();
    load_skills_from_dir_internal(
        dir,
        dir,
        true,
        &mut ignore_rules,
        &mut skills,
        &mut diagnostics,
    );
    (skills, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_directory_returns_empty_like_pi_load_skills_from_dir() {
        let dir = std::env::temp_dir().join(format!(
            "pm-agent-missing-skills-dir-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let (skills, diagnostics) = load_skills_from_dir(&dir);

        assert!(skills.is_empty());
        assert!(diagnostics.is_empty());
    }
}

fn load_skills_from_dir_internal(
    dir: &Path,
    root: &Path,
    include_root_files: bool,
    ignore_rules: &mut IgnoreRules,
    skills: &mut Vec<Skill>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    ignore_rules.add_rules_from_dir(dir, root);

    let Ok(entries) = fs::read_dir(dir) else {
        diagnostics.push(warning_diagnostic("failed to read skill directory", dir));
        return;
    };
    let entries = entries.filter_map(Result::ok).collect::<Vec<_>>();

    for entry in &entries {
        if entry.file_name().to_string_lossy() != "SKILL.md" {
            continue;
        }
        let full_path = entry.path();
        if !entry_is_file(entry) || ignore_rules.ignores(root, &full_path, false) {
            continue;
        }
        let (skill, mut skill_diagnostics) = load_skill_file(&full_path);
        diagnostics.append(&mut skill_diagnostics);
        if let Some(skill) = skill {
            skills.push(skill);
        }
        return;
    }

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }

        let full_path = entry.path();
        let is_dir = entry_is_dir(&entry);
        let is_file = entry_is_file(&entry);
        let ignored = ignore_rules.ignores(root, &full_path, is_dir);
        if ignored && !(is_dir && ignore_rules.may_contain_negated_match(root, &full_path)) {
            continue;
        }

        if is_dir {
            load_skills_from_dir_internal(
                &full_path,
                root,
                false,
                ignore_rules,
                skills,
                diagnostics,
            );
            continue;
        }

        if !is_file || !include_root_files || !name.ends_with(".md") {
            continue;
        }

        let (skill, mut skill_diagnostics) = load_skill_file(&full_path);
        diagnostics.append(&mut skill_diagnostics);
        if let Some(skill) = skill {
            skills.push(skill);
        }
    }
}

fn entry_is_dir(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .map(|file_type| {
            if file_type.is_symlink() {
                fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir())
            } else {
                file_type.is_dir()
            }
        })
        .unwrap_or(false)
}

fn entry_is_file(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .map(|file_type| {
            if file_type.is_symlink() {
                fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file())
            } else {
                file_type.is_file()
            }
        })
        .unwrap_or(false)
}
