use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use agent::harness::{format_skills_for_system_prompt, Skill};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContextFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SystemPromptPaths {
    pub readme_path: PathBuf,
    pub docs_path: PathBuf,
    pub examples_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BuildSystemPromptOptions {
    pub custom_prompt: Option<String>,
    pub selected_tools: Option<Vec<String>>,
    pub tool_snippets: HashMap<String, String>,
    pub prompt_guidelines: Vec<String>,
    pub append_system_prompt: Option<String>,
    pub cwd: PathBuf,
    pub context_files: Vec<PromptContextFile>,
    pub skills: Vec<Skill>,
    pub paths: SystemPromptPaths,
    pub current_date: Option<String>,
}

pub fn build_system_prompt(options: &BuildSystemPromptOptions) -> String {
    let prompt_cwd = slash_path(&options.cwd);
    let date = options
        .current_date
        .clone()
        .unwrap_or_else(current_date_string);
    let append_section = options
        .append_system_prompt
        .as_ref()
        .filter(|value| !value.is_empty())
        .map(|value| format!("\n\n{value}"))
        .unwrap_or_default();

    if let Some(custom_prompt) = &options.custom_prompt {
        let mut prompt = custom_prompt.clone();
        prompt.push_str(&append_section);
        append_project_context(&mut prompt, &options.context_files);
        if prompt_has_read_tool(options) && !options.skills.is_empty() {
            prompt.push_str(&format_skills_for_system_prompt(&options.skills));
        }
        append_date_and_cwd(&mut prompt, &date, &prompt_cwd);
        return prompt;
    }

    let tools = selected_tools(options);
    let visible_tools = tools
        .iter()
        .filter_map(|name| {
            options
                .tool_snippets
                .get(name)
                .map(|snippet| format!("- {name}: {snippet}"))
        })
        .collect::<Vec<_>>();
    let tools_list = if visible_tools.is_empty() {
        "(none)".to_string()
    } else {
        visible_tools.join("\n")
    };

    let guidelines = build_guidelines(&tools, &options.prompt_guidelines)
        .into_iter()
        .map(|guideline| format!("- {guideline}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut prompt = format!(
        "You are an expert coding assistant operating inside pi, a coding agent harness. You help users by reading files, executing commands, editing code, and writing new files.\n\n\
Available tools:\n{tools_list}\n\n\
In addition to the tools above, you may have access to other custom tools depending on the project.\n\n\
Guidelines:\n{guidelines}\n\n\
Pi documentation (read only when the user asks about pi itself, its SDK, extensions, themes, skills, or TUI):\n\
- Main documentation: {}\n\
- Additional docs: {}\n\
- Examples: {} (extensions, custom tools, SDK)\n\
- When reading pi docs or examples, resolve docs/... under Additional docs and examples/... under Examples, not the current working directory\n\
- When asked about: extensions (docs/extensions.md, examples/extensions/), themes (docs/themes.md), skills (docs/skills.md), prompt templates (docs/prompt-templates.md), TUI components (docs/tui.md), keybindings (docs/keybindings.md), SDK integrations (docs/sdk.md), custom providers (docs/custom-provider.md), adding models (docs/models.md), pi packages (docs/packages.md)\n\
- When working on pi topics, read the docs and examples, and follow .md cross-references before implementing\n\
- Always read pi .md files completely and follow links to related docs (e.g., tui.md for TUI API details)",
        slash_path(&options.paths.readme_path),
        slash_path(&options.paths.docs_path),
        slash_path(&options.paths.examples_path),
    );

    prompt.push_str(&append_section);
    append_project_context(&mut prompt, &options.context_files);
    if tools.iter().any(|tool| tool == "read") && !options.skills.is_empty() {
        prompt.push_str(&format_skills_for_system_prompt(&options.skills));
    }
    append_date_and_cwd(&mut prompt, &date, &prompt_cwd);
    prompt
}

fn selected_tools(options: &BuildSystemPromptOptions) -> Vec<String> {
    options.selected_tools.clone().unwrap_or_else(|| {
        ["read", "bash", "edit", "write"]
            .into_iter()
            .map(str::to_string)
            .collect()
    })
}

fn prompt_has_read_tool(options: &BuildSystemPromptOptions) -> bool {
    options
        .selected_tools
        .as_ref()
        .map_or(true, |tools| tools.iter().any(|tool| tool == "read"))
}

fn build_guidelines(tools: &[String], prompt_guidelines: &[String]) -> Vec<String> {
    let has_bash = tools.iter().any(|tool| tool == "bash");
    let has_grep = tools.iter().any(|tool| tool == "grep");
    let has_find = tools.iter().any(|tool| tool == "find");
    let has_ls = tools.iter().any(|tool| tool == "ls");

    let mut seen = BTreeSet::new();
    let mut guidelines = Vec::new();
    let mut add = |guideline: String| {
        if seen.insert(guideline.clone()) {
            guidelines.push(guideline);
        }
    };

    if has_bash && !has_grep && !has_find && !has_ls {
        add("Use bash for file operations like ls, rg, find".to_string());
    } else if has_bash && (has_grep || has_find || has_ls) {
        add("Prefer grep/find/ls tools over bash for file exploration (faster, respects .gitignore)".to_string());
    }

    for guideline in prompt_guidelines {
        let normalized = guideline.trim();
        if !normalized.is_empty() {
            add(normalized.to_string());
        }
    }

    add("Be concise in your responses".to_string());
    add("Show file paths clearly when working with files".to_string());
    guidelines
}

fn append_project_context(prompt: &mut String, context_files: &[PromptContextFile]) {
    if context_files.is_empty() {
        return;
    }

    prompt.push_str("\n\n<project_context>\n\n");
    prompt.push_str("Project-specific instructions and guidelines:\n\n");
    for file in context_files {
        prompt.push_str(&format!(
            "<project_instructions path=\"{}\">\n{}\n</project_instructions>\n\n",
            file.path, file.content
        ));
    }
    prompt.push_str("</project_context>\n");
}

fn append_date_and_cwd(prompt: &mut String, date: &str, cwd: &str) {
    prompt.push_str(&format!("\nCurrent date: {date}"));
    prompt.push_str(&format!("\nCurrent working directory: {cwd}"));
}

fn current_date_string() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() / 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn slash_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_options() -> BuildSystemPromptOptions {
        BuildSystemPromptOptions {
            custom_prompt: None,
            selected_tools: None,
            tool_snippets: HashMap::from([
                ("read".to_string(), "Read files".to_string()),
                ("bash".to_string(), "Run commands".to_string()),
            ]),
            prompt_guidelines: vec![" Be direct ".to_string()],
            append_system_prompt: Some("Extra rules".to_string()),
            cwd: PathBuf::from("/workspace"),
            context_files: vec![PromptContextFile {
                path: "AGENTS.md".to_string(),
                content: "Rules".to_string(),
            }],
            skills: vec![Skill {
                name: "rust".to_string(),
                description: "Rust work".to_string(),
                content: String::new(),
                file_path: "/skills/rust/SKILL.md".to_string(),
                source_info: None,
                disable_model_invocation: false,
            }],
            paths: SystemPromptPaths {
                readme_path: PathBuf::from("/pi/README.md"),
                docs_path: PathBuf::from("/pi/docs"),
                examples_path: PathBuf::from("/pi/examples"),
            },
            current_date: Some("2026-06-07".to_string()),
        }
    }

    #[test]
    fn builds_default_system_prompt_with_context_and_skills() {
        let prompt = build_system_prompt(&base_options());
        assert!(prompt.contains("Available tools:\n- read: Read files\n- bash: Run commands"));
        assert!(prompt.contains("- Be direct"));
        assert!(prompt.contains("<project_instructions path=\"AGENTS.md\">"));
        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("Current date: 2026-06-07"));
        assert!(prompt.contains("Current working directory: /workspace"));
    }

    #[test]
    fn custom_prompt_keeps_append_context_and_cwd() {
        let mut options = base_options();
        options.custom_prompt = Some("Custom".to_string());
        options.selected_tools = Some(vec!["bash".to_string()]);
        let prompt = build_system_prompt(&options);
        assert!(prompt.starts_with("Custom\n\nExtra rules"));
        assert!(prompt.contains("<project_context>"));
        assert!(!prompt.contains("<available_skills>"));
        assert!(prompt.contains("Current working directory: /workspace"));
    }

    #[test]
    fn formats_current_date_as_iso_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_240), (2025, 6, 1));
    }
}
