mod diagnostics;
mod discovery;
mod ignore;
mod validation;

use super::paths::{basename_without_extension, display_path};
use crate::diagnostics::ResourceDiagnostic;
use crate::utils::frontmatter::parse_frontmatter;
use agent::harness::Skill;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use diagnostics::{collision_diagnostic, warning_diagnostic};
use discovery::load_skills_from_dir;
use validation::{validate_description, validate_name};

pub fn load_skills(
    paths: impl IntoIterator<Item = PathBuf>,
) -> (Vec<Skill>, Vec<ResourceDiagnostic>) {
    let mut skill_map = BTreeMap::<String, Skill>::new();
    let mut real_paths = BTreeSet::<String>::new();
    let mut diagnostics = Vec::new();
    let mut collision_diagnostics = Vec::new();

    for path in paths {
        if !path.exists() {
            diagnostics.push(warning_diagnostic("skill path does not exist", &path));
            continue;
        }
        load_skill_path(
            &path,
            &mut skill_map,
            &mut real_paths,
            &mut diagnostics,
            &mut collision_diagnostics,
        );
    }

    diagnostics.extend(collision_diagnostics);
    (skill_map.into_values().collect(), diagnostics)
}

fn load_skill_path(
    path: &Path,
    skill_map: &mut BTreeMap<String, Skill>,
    real_paths: &mut BTreeSet<String>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
    collision_diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(warning_diagnostic(
                format!("failed to read skill path: {error}"),
                path,
            ));
            return;
        }
    };

    if metadata.is_dir() {
        let result = load_skills_from_dir(path);
        add_skills(result.0, skill_map, real_paths, collision_diagnostics);
        diagnostics.extend(result.1);
        return;
    }

    if metadata.is_file() && path.extension().and_then(|extension| extension.to_str()) == Some("md")
    {
        let (skill, mut skill_diagnostics) = load_skill_file(path);
        diagnostics.append(&mut skill_diagnostics);
        if let Some(skill) = skill {
            add_skills(vec![skill], skill_map, real_paths, collision_diagnostics);
        }
        return;
    }

    diagnostics.push(warning_diagnostic(
        "skill path is not a markdown file",
        path,
    ));
}

pub(super) fn load_skill_file(path: &Path) -> (Option<Skill>, Vec<ResourceDiagnostic>) {
    let mut diagnostics = Vec::new();
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(warning_diagnostic(
                format!("failed to parse skill file: {error}"),
                path,
            ));
            return (None, diagnostics);
        }
    };

    let parsed = match parse_frontmatter(&content) {
        Ok(parsed) => parsed,
        Err(error) => {
            diagnostics.push(warning_diagnostic(
                format!("failed to parse skill file: {error}"),
                path,
            ));
            return (None, diagnostics);
        }
    };

    let name = frontmatter_string(&parsed.frontmatter, "name")
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| basename_without_extension(path));
    let description = frontmatter_string(&parsed.frontmatter, "description").unwrap_or_default();

    for error in validate_name(&name) {
        diagnostics.push(warning_diagnostic(error, path));
    }
    for error in validate_description(&description) {
        diagnostics.push(warning_diagnostic(error, path));
    }
    if description.trim().is_empty() {
        return (None, diagnostics);
    }

    let disable_model_invocation =
        frontmatter_bool(&parsed.frontmatter, "disable-model-invocation")
            || frontmatter_bool(&parsed.frontmatter, "disable_model_invocation")
            || frontmatter_bool(&parsed.frontmatter, "disableModelInvocation");

    (
        Some(Skill {
            name,
            description,
            content: parsed.body,
            file_path: display_path(path),
            source_info: None,
            disable_model_invocation,
        }),
        diagnostics,
    )
}

fn add_skills(
    skills: Vec<Skill>,
    skill_map: &mut BTreeMap<String, Skill>,
    real_paths: &mut BTreeSet<String>,
    collision_diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    for skill in skills {
        let real_path = fs::canonicalize(&skill.file_path)
            .map(display_path)
            .unwrap_or_else(|_| skill.file_path.clone());
        if real_paths.contains(&real_path) {
            continue;
        }

        if let Some(existing) = skill_map.get(&skill.name) {
            collision_diagnostics.push(collision_diagnostic(existing, &skill));
        } else {
            real_paths.insert(real_path);
            skill_map.insert(skill.name.clone(), skill);
        }
    }
}

fn frontmatter_string(frontmatter: &Value, key: &str) -> Option<String> {
    frontmatter
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn frontmatter_bool(frontmatter: &Value, key: &str) -> bool {
    match frontmatter.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => matches!(value.as_str(), "true" | "1" | "yes"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::ResourceDiagnosticKind;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn loads_recursive_skill_roots_like_pi() {
        let dir = temp_dir();
        let nested = dir.join("team/backend");
        fs::create_dir_all(&nested).expect("nested skill dir");
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: backend-review\ndescription: Backend review\n---\nUse backend checks.",
        )
        .expect("skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "backend-review");
        assert_eq!(skills[0].description, "Backend review");
    }

    #[test]
    fn skill_root_stops_recursive_discovery_like_pi() {
        let dir = temp_dir();
        let child = dir.join("child");
        fs::create_dir_all(&child).expect("child dir");
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: root-skill\ndescription: Root skill\n---\nRoot body",
        )
        .expect("root skill");
        fs::write(
            child.join("SKILL.md"),
            "---\nname: child-skill\ndescription: Child skill\n---\nChild body",
        )
        .expect("child skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["root-skill"]
        );
    }

    #[test]
    fn root_markdown_files_are_loaded_but_nested_markdown_files_are_not() {
        let dir = temp_dir();
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            dir.join("root.md"),
            "---\nname: root-file\ndescription: Root file\n---\nRoot",
        )
        .expect("root md");
        fs::write(
            nested.join("loose.md"),
            "---\nname: nested-file\ndescription: Nested file\n---\nNested",
        )
        .expect("nested md");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "root-file");
    }

    #[test]
    fn ignore_files_skip_matching_skills() {
        let dir = temp_dir();
        let ignored = dir.join("ignored");
        let kept = dir.join("kept");
        fs::create_dir_all(&ignored).expect("ignored dir");
        fs::create_dir_all(&kept).expect("kept dir");
        fs::write(dir.join(".ignore"), "ignored\n").expect("ignore");
        fs::write(
            ignored.join("SKILL.md"),
            "---\nname: ignored-skill\ndescription: Ignored skill\n---\nIgnored",
        )
        .expect("ignored skill");
        fs::write(
            kept.join("SKILL.md"),
            "---\nname: kept-skill\ndescription: Kept skill\n---\nKept",
        )
        .expect("kept skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "kept-skill");
    }

    #[test]
    fn ignore_negation_allows_matching_skill_like_pi() {
        let dir = temp_dir();
        let ignored = dir.join("ignored");
        let allowed = ignored.join("allowed");
        let skipped = ignored.join("skipped");
        fs::create_dir_all(&allowed).expect("allowed dir");
        fs::create_dir_all(&skipped).expect("skipped dir");
        fs::write(dir.join(".ignore"), "ignored\n!ignored/allowed\n").expect("ignore");
        fs::write(
            allowed.join("SKILL.md"),
            "---\nname: allowed-skill\ndescription: Allowed skill\n---\nAllowed",
        )
        .expect("allowed skill");
        fs::write(
            skipped.join("SKILL.md"),
            "---\nname: skipped-skill\ndescription: Skipped skill\n---\nSkipped",
        )
        .expect("skipped skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "allowed-skill");
    }

    #[test]
    fn parses_disable_model_invocation_frontmatter() {
        let dir = temp_dir();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: explicit-only\ndescription: Explicit only\ndisable-model-invocation: true\n---\nBody",
        )
        .expect("skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(skills.len(), 1);
        assert!(skills[0].disable_model_invocation);
    }

    #[test]
    fn missing_description_reports_warning_and_skips_skill() {
        let dir = temp_dir();
        fs::write(dir.join("SKILL.md"), "---\nname: no-description\n---\nBody").expect("skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert!(skills.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, ResourceDiagnosticKind::Warning);
        assert_eq!(diagnostics[0].message, "description is required");
    }

    #[test]
    fn duplicate_skill_names_report_collision() {
        let dir = temp_dir();
        let first = dir.join("first");
        let second = dir.join("second");
        fs::create_dir_all(&first).expect("first dir");
        fs::create_dir_all(&second).expect("second dir");
        fs::write(
            first.join("SKILL.md"),
            "---\nname: duplicate\ndescription: First\n---\nFirst",
        )
        .expect("first skill");
        fs::write(
            second.join("SKILL.md"),
            "---\nname: duplicate\ndescription: Second\n---\nSecond",
        )
        .expect("second skill");

        let (skills, diagnostics) = load_skills(vec![dir]);

        assert_eq!(skills.len(), 1);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ResourceDiagnosticKind::Collision));
    }

    fn temp_dir() -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-skills-loader-test-{id}"));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("old temp dir should be removed");
        }
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
