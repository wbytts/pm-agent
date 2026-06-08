use ai::ContentBlock;

use crate::cli::args::CliArgs;

#[derive(Debug)]
pub struct InitialMessageInput<'a> {
    pub parsed: &'a mut CliArgs,
    pub file_text: Option<String>,
    pub file_images: Vec<ContentBlock>,
    pub stdin_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialMessageResult {
    pub initial_message: Option<String>,
    pub initial_images: Vec<ContentBlock>,
}

pub fn build_initial_message(input: InitialMessageInput<'_>) -> InitialMessageResult {
    let mut parts = Vec::new();
    if let Some(stdin_content) = input.stdin_content {
        parts.push(stdin_content);
    }
    if let Some(file_text) = input.file_text.filter(|text| !text.is_empty()) {
        parts.push(file_text);
    }
    if !input.parsed.messages.is_empty() {
        parts.push(input.parsed.messages.remove(0));
    }

    InitialMessageResult {
        initial_message: (!parts.is_empty()).then(|| parts.join("")),
        initial_images: input.file_images,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse_args;

    #[test]
    fn combines_stdin_file_text_and_first_message() {
        let mut args = parse_args(&["first".to_string(), "second".to_string()]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut args,
            file_text: Some("<file>content</file>\n".to_string()),
            file_images: Vec::new(),
            stdin_content: Some("stdin\n".to_string()),
        });

        assert_eq!(
            result.initial_message.as_deref(),
            Some("stdin\n<file>content</file>\nfirst")
        );
        assert_eq!(args.messages, vec!["second"]);
    }

    #[test]
    fn returns_none_without_content() {
        let mut args = parse_args(&[]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut args,
            file_text: None,
            file_images: Vec::new(),
            stdin_content: None,
        });

        assert_eq!(result.initial_message, None);
        assert!(result.initial_images.is_empty());
    }
}
