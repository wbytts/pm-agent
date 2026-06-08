use agent::harness::{format_prompt_template_invocation, parse_command_args, PromptTemplate};

pub fn expand_prompt_template(text: &str, templates: &[PromptTemplate]) -> String {
    let Some(invocation) = parse_prompt_template_invocation(text) else {
        return text.to_string();
    };

    let Some(template) = templates
        .iter()
        .find(|template| template.name == invocation.name)
    else {
        return text.to_string();
    };

    let args = parse_command_args(invocation.args);
    format_prompt_template_invocation(template, &args)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptTemplateInvocation<'a> {
    name: &'a str,
    args: &'a str,
}

fn parse_prompt_template_invocation(text: &str) -> Option<PromptTemplateInvocation<'_>> {
    let rest = text.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }

    let name_end = rest
        .char_indices()
        .find(|(_, character)| character.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }

    let args = rest[name_end..].trim_start();
    Some(PromptTemplateInvocation { name, args })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_matching_prompt_template_with_args() {
        let templates = vec![template("review", "Review $1 with $ARGUMENTS and ${@:2}")];

        assert_eq!(
            expand_prompt_template(r#"/review "src lib" carefully now"#, &templates),
            "Review src lib with src lib carefully now and carefully now"
        );
    }

    #[test]
    fn returns_original_text_for_non_template_input() {
        let templates = vec![template("review", "Review $ARGUMENTS")];

        assert_eq!(
            expand_prompt_template("review src", &templates),
            "review src"
        );
        assert_eq!(expand_prompt_template("/", &templates), "/");
        assert_eq!(
            expand_prompt_template("/missing src", &templates),
            "/missing src"
        );
    }

    #[test]
    fn parses_template_invocation_like_pi() {
        assert_eq!(
            parse_prompt_template_invocation("/review src/main.rs"),
            Some(PromptTemplateInvocation {
                name: "review",
                args: "src/main.rs"
            })
        );
        assert_eq!(
            parse_prompt_template_invocation("/review"),
            Some(PromptTemplateInvocation {
                name: "review",
                args: ""
            })
        );
    }

    fn template(name: &str, content: &str) -> PromptTemplate {
        PromptTemplate {
            name: name.to_string(),
            description: None,
            argument_hint: None,
            content: content.to_string(),
            file_path: String::new(),
            source_info: None,
        }
    }
}
