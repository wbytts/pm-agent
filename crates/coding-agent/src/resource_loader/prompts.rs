use agent::harness::{load_prompt_templates, PromptTemplate};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::diagnostics::{
    ResourceCollision, ResourceDiagnostic, ResourceDiagnosticKind, ResourceType,
};

pub fn load_prompts(paths: &[PathBuf]) -> (Vec<PromptTemplate>, Vec<ResourceDiagnostic>) {
    let (prompts, diagnostics) = load_prompt_templates(paths);
    let mut diagnostics = diagnostics
        .into_iter()
        .map(|diagnostic| ResourceDiagnostic {
            kind: ResourceDiagnosticKind::Error,
            message: diagnostic.message,
            path: Some(diagnostic.path),
            collision: None,
        })
        .collect::<Vec<_>>();
    let (prompts, mut collision_diagnostics) = dedupe_prompts(prompts);
    diagnostics.append(&mut collision_diagnostics);
    (prompts, diagnostics)
}

fn dedupe_prompts(prompts: Vec<PromptTemplate>) -> (Vec<PromptTemplate>, Vec<ResourceDiagnostic>) {
    let mut seen = BTreeMap::<String, PromptTemplate>::new();
    let mut diagnostics = Vec::new();
    for prompt in prompts {
        if let Some(existing) = seen.get(&prompt.name) {
            diagnostics.push(ResourceDiagnostic {
                kind: ResourceDiagnosticKind::Collision,
                message: format!("name \"/{}\" collision", prompt.name),
                path: Some(prompt.file_path.clone()),
                collision: Some(ResourceCollision {
                    resource_type: ResourceType::Prompt,
                    name: prompt.name.clone(),
                    winner_path: existing.file_path.clone(),
                    loser_path: prompt.file_path,
                    winner_source: None,
                    loser_source: None,
                }),
            });
        } else {
            seen.insert(prompt.name.clone(), prompt);
        }
    }
    (seen.into_values().collect(), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{ResourceDiagnosticKind, ResourceType};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn duplicate_prompt_names_report_collision_like_pi() {
        let dir = temp_dir();
        let first_dir = dir.join("first");
        let second_dir = dir.join("second");
        fs::create_dir_all(&first_dir).expect("first dir");
        fs::create_dir_all(&second_dir).expect("second dir");
        let first = first_dir.join("review.md");
        let second = second_dir.join("review.md");
        fs::write(&first, "# Review\nFirst").expect("first prompt");
        fs::write(&second, "# Review\nSecond").expect("second prompt");

        let (prompts, diagnostics) = load_prompts(&[first.clone(), second.clone()]);

        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].content, "# Review\nFirst");
        let collision = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == ResourceDiagnosticKind::Collision)
            .and_then(|diagnostic| diagnostic.collision.as_ref())
            .expect("duplicate prompt should report collision");
        assert_eq!(collision.resource_type, ResourceType::Prompt);
        assert_eq!(collision.name, "review");
        assert_eq!(collision.winner_path, first.to_string_lossy());
        assert_eq!(collision.loser_path, second.to_string_lossy());
    }

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-prompts-test-{nanos}-{count}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
