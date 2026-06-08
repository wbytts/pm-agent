use std::fs;
use std::path::{Path, PathBuf};

use crate::harness::types::PromptTemplate;
use serde_yaml::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptTemplateDiagnosticCode {
    FileInfoFailed,
    ListFailed,
    ReadFailed,
    ParseFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplateDiagnostic {
    pub code: PromptTemplateDiagnosticCode,
    pub message: String,
    pub path: String,
}

pub fn load_prompt_templates(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> (Vec<PromptTemplate>, Vec<PromptTemplateDiagnostic>) {
    let mut prompt_templates = Vec::new();
    let mut diagnostics = Vec::new();

    for path in paths {
        let path = path.as_ref();
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                diagnostics.push(diagnostic(
                    PromptTemplateDiagnosticCode::FileInfoFailed,
                    error.to_string(),
                    path,
                ));
                continue;
            }
        };

        if metadata.is_dir() {
            let (loaded, nested_diagnostics) = load_templates_from_dir(path);
            prompt_templates.extend(loaded);
            diagnostics.extend(nested_diagnostics);
        } else if metadata.is_file() && path.extension().is_some_and(|extension| extension == "md")
        {
            match load_template_from_file(path) {
                Ok(template) => prompt_templates.push(template),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
    }

    (prompt_templates, diagnostics)
}

fn load_templates_from_dir(dir: &Path) -> (Vec<PromptTemplate>, Vec<PromptTemplateDiagnostic>) {
    let mut prompt_templates = Vec::new();
    let mut diagnostics = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(diagnostic(
                PromptTemplateDiagnosticCode::ListFailed,
                error.to_string(),
                dir,
            ));
            return (prompt_templates, diagnostics);
        }
    };

    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        match load_template_from_file(&path) {
            Ok(template) => prompt_templates.push(template),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    (prompt_templates, diagnostics)
}

fn load_template_from_file(path: &Path) -> Result<PromptTemplate, PromptTemplateDiagnostic> {
    let raw_content = fs::read_to_string(path).map_err(|error| {
        diagnostic(
            PromptTemplateDiagnosticCode::ReadFailed,
            error.to_string(),
            path,
        )
    })?;
    let (frontmatter, body) = parse_frontmatter(&raw_content)
        .map_err(|message| diagnostic(PromptTemplateDiagnosticCode::ParseFailed, message, path))?;
    let description = frontmatter
        .description
        .or_else(|| first_content_line(&body).map(first_line_description));

    Ok(PromptTemplate {
        name: basename_without_md(path),
        description,
        argument_hint: frontmatter.argument_hint,
        content: body,
        file_path: path.to_string_lossy().to_string(),
        source_info: None,
    })
}

#[derive(Debug, Default)]
struct Frontmatter {
    description: Option<String>,
    argument_hint: Option<String>,
}

fn parse_frontmatter(content: &str) -> Result<(Frontmatter, String), String> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---") {
        return Ok((Frontmatter::default(), normalized));
    }

    let Some(end_index) = normalized[3..].find("\n---").map(|index| index + 3) else {
        return Ok((Frontmatter::default(), normalized));
    };
    let yaml = normalized[4..end_index].trim();
    let body = normalized[end_index + 4..].trim().to_string();
    let frontmatter = parse_yaml_frontmatter(yaml)?;

    Ok((frontmatter, body))
}

fn parse_yaml_frontmatter(yaml: &str) -> Result<Frontmatter, String> {
    let parsed = serde_yaml::from_str::<Value>(yaml).map_err(|error| error.to_string())?;
    let Some(mapping) = parsed.as_mapping() else {
        return Ok(Frontmatter::default());
    };

    Ok(Frontmatter {
        description: string_field(mapping, "description"),
        argument_hint: string_field(mapping, "argument-hint"),
    })
}

fn string_field(mapping: &serde_yaml::Mapping, key: &str) -> Option<String> {
    mapping
        .get(Value::String(key.to_string()))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn parse_command_args(args_string: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;

    for character in args_string.chars() {
        if let Some(quote) = in_quote {
            if character == quote {
                in_quote = None;
            } else {
                current.push(character);
            }
        } else if character == '"' || character == '\'' {
            in_quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

pub fn substitute_args(content: &str, args: &[String]) -> String {
    let all_args = args.join(" ");
    let chars = content.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            output.push(chars[index]);
            index += 1;
            continue;
        }

        if content[index..].starts_with("$ARGUMENTS") {
            output.push_str(&all_args);
            index += "$ARGUMENTS".len();
            continue;
        }

        if content[index..].starts_with("$@") {
            output.push_str(&all_args);
            index += "$@".len();
            continue;
        }

        if content[index..].starts_with("${@:") {
            let after_start = index + "${@:".len();
            if let Some(close_offset) = content[after_start..].find('}') {
                let end = after_start + close_offset;
                let expression = &content[after_start..end];
                if let Some(replacement) = slice_args(expression, args) {
                    output.push_str(&replacement);
                } else {
                    output.push_str(&content[index..=end]);
                }
                index = end + 1;
                continue;
            }
        }

        if chars.get(index + 1).is_some_and(char::is_ascii_digit) {
            let mut end = index + 1;
            while chars.get(end).is_some_and(char::is_ascii_digit) {
                end += 1;
            }
            let number = chars[index + 1..end]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .unwrap_or(0);
            if number > 0 {
                output.push_str(args.get(number - 1).map_or("", String::as_str));
            }
            index = end;
            continue;
        }

        output.push(chars[index]);
        index += 1;
    }
    output
}

pub fn format_prompt_template_invocation(template: &PromptTemplate, args: &[String]) -> String {
    substitute_args(&template.content, args)
}

fn slice_args(expression: &str, args: &[String]) -> Option<String> {
    let parts = expression.split(':').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > 2
        || parts.iter().any(|part| {
            part.is_empty() || !part.chars().all(|character| character.is_ascii_digit())
        })
    {
        return None;
    }

    let start = parts
        .first()
        .and_then(|value| value.parse::<usize>().ok())?
        .saturating_sub(1);
    let slice = if let Some(length) = parts.get(1).and_then(|value| value.parse::<usize>().ok()) {
        args.iter().skip(start).take(length)
    } else {
        args.iter().skip(start).take(usize::MAX)
    };
    Some(slice.cloned().collect::<Vec<_>>().join(" "))
}

fn first_content_line(body: &str) -> Option<&str> {
    body.lines().find(|line| !line.trim().is_empty())
}

fn first_line_description(line: &str) -> String {
    if line.len() > 60 {
        format!("{}...", &line[..60])
    } else {
        line.to_string()
    }
}

fn basename_without_md(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_end_matches(".md")
        .to_string()
}

fn diagnostic(
    code: PromptTemplateDiagnosticCode,
    message: impl Into<String>,
    path: &Path,
) -> PromptTemplateDiagnostic {
    PromptTemplateDiagnostic {
        code,
        message: message.into(),
        path: display_path(path),
    }
}

fn display_path(path: &Path) -> String {
    PathBuf::from(path).to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn substitutes_prompt_template_arguments() {
        let args = parse_command_args(r#"one "two words" three"#);
        assert_eq!(args, vec!["one", "two words", "three"]);
        assert_eq!(
            substitute_args("$1|$2|$@|${@:2}|${@:2:1}|$ARGUMENTS", &args),
            "one|two words|one two words three|two words three|two words|one two words three"
        );
    }

    #[test]
    fn parses_newlines_as_argument_separators_like_pi() {
        let args = parse_command_args("label-2\n\nHere is some description #2.");

        assert_eq!(
            args,
            vec!["label-2", "Here", "is", "some", "description", "#2."]
        );
    }

    #[test]
    fn leaves_invalid_slice_expressions_unchanged_like_pi() {
        let args = vec!["one".to_string(), "two".to_string()];

        assert_eq!(
            substitute_args("${@:x}|${@:1:x}|${@:}", &args),
            "${@:x}|${@:1:x}|${@:}"
        );
    }

    #[test]
    fn does_not_recursively_substitute_patterns_inside_argument_values_like_pi() {
        let args = vec!["$1".to_string(), "$@".to_string(), "${@:1}".to_string()];

        assert_eq!(substitute_args("$ARGUMENTS", &args), "$1 $@ ${@:1}");
        assert_eq!(substitute_args("${@:2}", &args), "$@ ${@:1}");
    }

    #[test]
    fn loads_markdown_prompt_templates_from_directory() {
        let dir = temp_dir();
        fs::write(
            dir.join("review.md"),
            "---\ndescription: Review code\n---\nUse $ARGUMENTS",
        )
        .expect("template should be written");

        let (templates, diagnostics) = load_prompt_templates([&dir]);
        assert!(diagnostics.is_empty());
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "review");
        assert_eq!(templates[0].description.as_deref(), Some("Review code"));
    }

    #[test]
    fn loads_prompt_templates_from_symlinked_directory_like_pi() {
        let dir = temp_dir();
        let target = dir.join("target");
        let link = dir.join("linked-prompts");
        fs::create_dir_all(&target).expect("target dir should be created");
        fs::write(
            target.join("review.md"),
            "---\ndescription: Review symlink\n---\nUse $ARGUMENTS",
        )
        .expect("template should be written");
        symlink_dir(&target, &link);

        let (templates, diagnostics) = load_prompt_templates([&link]);

        assert!(diagnostics.is_empty());
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "review");
        assert_eq!(templates[0].description.as_deref(), Some("Review symlink"));
    }

    #[test]
    fn loads_argument_hint_from_prompt_template_frontmatter_like_pi() {
        let dir = temp_dir();
        fs::write(
            dir.join("review.md"),
            "---\ndescription: Review code\nargument-hint: \"<file> <focus>\"\n---\nUse $ARGUMENTS",
        )
        .expect("template should be written");

        let (templates, diagnostics) = load_prompt_templates([&dir]);
        assert!(diagnostics.is_empty());
        assert_eq!(templates.len(), 1);
        assert_eq!(
            templates[0].argument_hint.as_deref(),
            Some("<file> <focus>")
        );
    }

    #[test]
    fn parses_yaml_prompt_template_frontmatter_like_pi() {
        let dir = temp_dir();
        fs::write(
            dir.join("review.md"),
            "---\ndescription: |\n  Line one\n  Line two\nargument-hint: '<file>'\n---\nUse $ARGUMENTS",
        )
        .expect("template should be written");

        let (templates, diagnostics) = load_prompt_templates([&dir]);

        assert!(diagnostics.is_empty());
        assert_eq!(templates.len(), 1);
        assert_eq!(
            templates[0].description.as_deref(),
            Some("Line one\nLine two\n")
        );
        assert_eq!(templates[0].argument_hint.as_deref(), Some("<file>"));
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-prompt-template-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[cfg(unix)]
    fn symlink_dir(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).expect("directory symlink should be created");
    }

    #[cfg(windows)]
    fn symlink_dir(source: &Path, target: &Path) {
        std::os::windows::fs::symlink_dir(source, target)
            .expect("directory symlink should be created");
    }
}
