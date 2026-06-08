use super::paths::{display_path, expand_home, normalize_path};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsFile {
    pub path: String,
    pub content: String,
}

pub fn load_project_context_files(cwd: &Path, agent_dir: &Path) -> Vec<AgentsFile> {
    let mut context_files = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(file) = load_context_file_from_dir(agent_dir) {
        seen.insert(file.path.clone());
        context_files.push(file);
    }

    let mut ancestor_files = Vec::new();
    let mut current = normalize_path(cwd);
    loop {
        if let Some(file) = load_context_file_from_dir(&current) {
            if seen.insert(file.path.clone()) {
                ancestor_files.insert(0, file);
            }
        }
        let Some(parent) = current.parent().map(Path::to_path_buf) else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }
    context_files.extend(ancestor_files);
    context_files
}

pub fn resolve_prompt_input(input: Option<&str>) -> Option<String> {
    let input = input?;
    let path = expand_home(input);
    if path.exists() {
        fs::read_to_string(path).ok()
    } else {
        Some(input.to_string())
    }
}

pub fn discover_first_file(dir: &Path, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let path = dir.join(name);
        path.exists().then(|| display_path(path))
    })
}

fn load_context_file_from_dir(dir: &Path) -> Option<AgentsFile> {
    for filename in ["AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"] {
        let path = dir.join(filename);
        if path.exists() {
            let content = fs::read_to_string(&path).ok()?;
            return Some(AgentsFile {
                path: display_path(&path),
                content,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_context_files_from_agent_dir_and_project_ancestors() {
        let dir = temp_dir();
        let agent_dir = dir.join("agent");
        let project = dir.join("project/sub");
        fs::create_dir_all(&agent_dir).expect("agent dir");
        fs::create_dir_all(&project).expect("project dir");
        fs::write(agent_dir.join("AGENTS.md"), "global").expect("global context");
        fs::write(dir.join("project/AGENTS.md"), "project").expect("project context");

        let files = load_project_context_files(&project, &agent_dir);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].content, "global");
        assert_eq!(files[1].content, "project");
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-context-loader-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
