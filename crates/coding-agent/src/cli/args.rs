use ai::ModelThinkingLevel;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliMode {
    Text,
    Json,
    Rpc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Interactive,
    Print,
    Json,
    Rpc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliDiagnosticType {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliDiagnostic {
    pub r#type: CliDiagnosticType,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Vec<String>,
    pub thinking: Option<ModelThinkingLevel>,
    pub continue_session: bool,
    pub resume: bool,
    pub help: bool,
    pub version: bool,
    pub mode: Option<CliMode>,
    pub no_session: bool,
    pub session: Option<String>,
    pub fork: Option<String>,
    pub session_dir: Option<String>,
    pub models: Vec<String>,
    pub tools: Vec<String>,
    pub no_tools: bool,
    pub no_builtin_tools: bool,
    pub extensions: Vec<String>,
    pub no_extensions: bool,
    pub print: bool,
    pub export: Option<String>,
    pub no_skills: bool,
    pub skills: Vec<String>,
    pub prompt_templates: Vec<String>,
    pub no_prompt_templates: bool,
    pub themes: Vec<String>,
    pub no_themes: bool,
    pub no_context_files: bool,
    pub list_models: Option<Option<String>>,
    pub offline: bool,
    pub verbose: bool,
    pub messages: Vec<String>,
    pub file_args: Vec<String>,
    pub unknown_flags: BTreeMap<String, UnknownFlagValue>,
    pub diagnostics: Vec<CliDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownFlagValue {
    Bool(bool),
    String(String),
}

pub fn is_valid_thinking_level(level: &str) -> bool {
    parse_thinking_level(level).is_some()
}

pub fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--help" | "-h" => result.help = true,
            "--version" | "-v" => result.version = true,
            "--mode" => {
                if let Some(value) = take_value(args, &mut index) {
                    result.mode = parse_mode(value);
                }
            }
            "--continue" | "-c" => result.continue_session = true,
            "--resume" | "-r" => result.resume = true,
            "--provider" => assign_value(args, &mut index, |value| result.provider = Some(value)),
            "--model" => assign_value(args, &mut index, |value| result.model = Some(value)),
            "--api-key" => assign_value(args, &mut index, |value| result.api_key = Some(value)),
            "--system-prompt" => {
                assign_value(args, &mut index, |value| result.system_prompt = Some(value))
            }
            "--append-system-prompt" => assign_value(args, &mut index, |value| {
                result.append_system_prompt.push(value)
            }),
            "--no-session" => result.no_session = true,
            "--session" => assign_value(args, &mut index, |value| result.session = Some(value)),
            "--fork" => assign_value(args, &mut index, |value| result.fork = Some(value)),
            "--session-dir" => {
                assign_value(args, &mut index, |value| result.session_dir = Some(value))
            }
            "--models" => assign_value(args, &mut index, |value| {
                result.models = split_csv(&value);
            }),
            "--no-tools" | "-nt" => result.no_tools = true,
            "--no-builtin-tools" | "-nbt" => result.no_builtin_tools = true,
            "--tools" | "-t" => assign_value(args, &mut index, |value| {
                result.tools = split_csv(&value);
            }),
            "--thinking" => {
                if let Some(value) = take_value(args, &mut index) {
                    if let Some(level) = parse_thinking_level(value) {
                        result.thinking = Some(level);
                    } else {
                        result.diagnostics.push(CliDiagnostic {
                            r#type: CliDiagnosticType::Warning,
                            message: format!(
                                "Invalid thinking level \"{value}\". Valid values: off, minimal, low, medium, high, xhigh"
                            ),
                        });
                    }
                }
            }
            "--print" | "-p" => {
                result.print = true;
                if let Some(next) = args.get(index + 1) {
                    if !next.starts_with('@') && (!next.starts_with('-') || next.starts_with("---"))
                    {
                        result.messages.push(next.clone());
                        index += 1;
                    }
                }
            }
            "--export" => assign_value(args, &mut index, |value| result.export = Some(value)),
            "--extension" | "-e" => {
                assign_value(args, &mut index, |value| result.extensions.push(value))
            }
            "--no-extensions" | "-ne" => result.no_extensions = true,
            "--skill" => assign_value(args, &mut index, |value| result.skills.push(value)),
            "--prompt-template" => assign_value(args, &mut index, |value| {
                result.prompt_templates.push(value)
            }),
            "--theme" => assign_value(args, &mut index, |value| result.themes.push(value)),
            "--no-skills" | "-ns" => result.no_skills = true,
            "--no-prompt-templates" | "-np" => result.no_prompt_templates = true,
            "--no-themes" => result.no_themes = true,
            "--no-context-files" | "-nc" => result.no_context_files = true,
            "--list-models" => {
                if let Some(next) = args.get(index + 1) {
                    if !next.starts_with('-') && !next.starts_with('@') {
                        result.list_models = Some(Some(next.clone()));
                        index += 1;
                    } else {
                        result.list_models = Some(None);
                    }
                } else {
                    result.list_models = Some(None);
                }
            }
            "--verbose" => result.verbose = true,
            "--offline" => result.offline = true,
            _ if arg.starts_with('@') => result.file_args.push(arg[1..].to_string()),
            _ if arg.starts_with("--") => parse_unknown_flag(args, &mut index, &mut result),
            _ if arg.starts_with('-') => result.diagnostics.push(CliDiagnostic {
                r#type: CliDiagnosticType::Error,
                message: format!("Unknown option: {arg}"),
            }),
            _ => result.messages.push(arg.clone()),
        }
        index += 1;
    }
    result
}

pub fn resolve_app_mode(parsed: &CliArgs, stdin_is_tty: bool) -> AppMode {
    match parsed.mode {
        Some(CliMode::Rpc) => AppMode::Rpc,
        Some(CliMode::Json) => AppMode::Json,
        _ if parsed.print || !stdin_is_tty => AppMode::Print,
        _ => AppMode::Interactive,
    }
}

fn parse_mode(value: &str) -> Option<CliMode> {
    match value {
        "text" => Some(CliMode::Text),
        "json" => Some(CliMode::Json),
        "rpc" => Some(CliMode::Rpc),
        _ => None,
    }
}

fn parse_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn assign_value(args: &[String], index: &mut usize, mut assign: impl FnMut(String)) {
    if let Some(value) = take_value(args, index) {
        assign(value.to_string());
    }
}

fn take_value<'a>(args: &'a [String], index: &mut usize) -> Option<&'a str> {
    if *index + 1 >= args.len() {
        return None;
    }
    *index += 1;
    Some(args[*index].as_str())
}

fn parse_unknown_flag(args: &[String], index: &mut usize, result: &mut CliArgs) {
    let arg = &args[*index];
    if let Some((name, value)) = arg[2..].split_once('=') {
        result.unknown_flags.insert(
            name.to_string(),
            UnknownFlagValue::String(value.to_string()),
        );
        return;
    }

    let flag_name = arg[2..].to_string();
    if let Some(next) = args.get(*index + 1) {
        if !next.starts_with('-') && !next.starts_with('@') {
            result
                .unknown_flags
                .insert(flag_name, UnknownFlagValue::String(next.clone()));
            *index += 1;
            return;
        }
    }
    result
        .unknown_flags
        .insert(flag_name, UnknownFlagValue::Bool(true));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> CliArgs {
        parse_args(
            &values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn parses_core_cli_flags_like_pi() {
        let args = parse(&[
            "--provider",
            "openai",
            "--model",
            "gpt-5",
            "--api-key",
            "sk-test",
            "--append-system-prompt",
            "one",
            "--append-system-prompt",
            "two",
            "--models",
            "sonnet:high, haiku:low",
            "--tools",
            "read,grep,,find",
            "@prompt.md",
            "hello",
        ]);

        assert_eq!(args.provider.as_deref(), Some("openai"));
        assert_eq!(args.model.as_deref(), Some("gpt-5"));
        assert_eq!(args.api_key.as_deref(), Some("sk-test"));
        assert_eq!(args.append_system_prompt, vec!["one", "two"]);
        assert_eq!(args.models, vec!["sonnet:high", "haiku:low"]);
        assert_eq!(args.tools, vec!["read", "grep", "find"]);
        assert_eq!(args.file_args, vec!["prompt.md"]);
        assert_eq!(args.messages, vec!["hello"]);
    }

    #[test]
    fn print_option_consumes_following_prompt_like_pi() {
        let args = parse(&["-p", "List files", "--mode", "json"]);

        assert!(args.print);
        assert_eq!(args.messages, vec!["List files"]);
        assert_eq!(resolve_app_mode(&args, true), AppMode::Json);
    }

    #[test]
    fn invalid_thinking_level_is_warning_not_error() {
        let args = parse(&["--thinking", "huge"]);

        assert_eq!(args.thinking, None);
        assert_eq!(args.diagnostics.len(), 1);
        assert_eq!(args.diagnostics[0].r#type, CliDiagnosticType::Warning);
    }

    #[test]
    fn unknown_long_flags_are_extension_flags() {
        let args = parse(&["--plan", "--owner=wby", "--ticket", "PM-1", "-x"]);

        assert_eq!(
            args.unknown_flags.get("plan"),
            Some(&UnknownFlagValue::Bool(true))
        );
        assert_eq!(
            args.unknown_flags.get("owner"),
            Some(&UnknownFlagValue::String("wby".to_string()))
        );
        assert_eq!(
            args.unknown_flags.get("ticket"),
            Some(&UnknownFlagValue::String("PM-1".to_string()))
        );
        assert_eq!(args.diagnostics[0].r#type, CliDiagnosticType::Error);
    }

    #[test]
    fn resolves_app_modes_like_pi() {
        assert_eq!(resolve_app_mode(&parse(&[]), true), AppMode::Interactive);
        assert_eq!(resolve_app_mode(&parse(&[]), false), AppMode::Print);
        assert_eq!(
            resolve_app_mode(&parse(&["--mode", "rpc"]), true),
            AppMode::Rpc
        );
        assert_eq!(
            resolve_app_mode(&parse(&["--mode", "json"]), true),
            AppMode::Json
        );
    }
}
