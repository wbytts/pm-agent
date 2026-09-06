use agent::harness::{PromptTemplate, Skill};
use serde_json::{json, Value};

use crate::extensions::ResolvedCommand;
use crate::rpc::{RpcSlashCommand, RpcSlashCommandSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandSource {
    Extension,
    Prompt,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandInfo {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub source: SlashCommandSource,
    pub source_info: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSlashCommand {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSlashCommandInfo {
    pub name: String,
    pub description: String,
}

pub const BUILTIN_SLASH_COMMANDS: &[BuiltinSlashCommand] = &[
    BuiltinSlashCommand {
        name: "settings",
        description: "Open settings menu",
    },
    BuiltinSlashCommand {
        name: "model",
        description: "Select model (opens selector UI)",
    },
    BuiltinSlashCommand {
        name: "scoped-models",
        description: "Enable/disable models for Ctrl+P cycling",
    },
    BuiltinSlashCommand {
        name: "export",
        description: "Export session (HTML default, or specify path: .html/.jsonl)",
    },
    BuiltinSlashCommand {
        name: "import",
        description: "Import and resume a session from a JSONL file",
    },
    BuiltinSlashCommand {
        name: "share",
        description: "Share session as a secret GitHub gist",
    },
    BuiltinSlashCommand {
        name: "copy",
        description: "Copy last agent message to clipboard",
    },
    BuiltinSlashCommand {
        name: "name",
        description: "Set session display name",
    },
    BuiltinSlashCommand {
        name: "session",
        description: "Show session info and stats",
    },
    BuiltinSlashCommand {
        name: "changelog",
        description: "Show changelog entries",
    },
    BuiltinSlashCommand {
        name: "hotkeys",
        description: "Show all keyboard shortcuts",
    },
    BuiltinSlashCommand {
        name: "fork",
        description: "Create a new fork from a previous user message",
    },
    BuiltinSlashCommand {
        name: "clone",
        description: "Duplicate the current session at the current position",
    },
    BuiltinSlashCommand {
        name: "tree",
        description: "Navigate session tree (switch branches)",
    },
    BuiltinSlashCommand {
        name: "login",
        description: "Configure provider authentication",
    },
    BuiltinSlashCommand {
        name: "logout",
        description: "Remove provider authentication",
    },
    BuiltinSlashCommand {
        name: "new",
        description: "Start a new session",
    },
    BuiltinSlashCommand {
        name: "compact",
        description: "Manually compact the session context",
    },
    BuiltinSlashCommand {
        name: "resume",
        description: "Resume a different session",
    },
    BuiltinSlashCommand {
        name: "reload",
        description: "Reload keybindings, extensions, skills, prompts, and themes",
    },
    BuiltinSlashCommand {
        name: "quit",
        description: "Quit PM Agent",
    },
];

pub fn builtin_slash_commands(app_name: &str) -> Vec<BuiltinSlashCommandInfo> {
    BUILTIN_SLASH_COMMANDS
        .iter()
        .map(|command| BuiltinSlashCommandInfo {
            name: command.name.to_string(),
            description: if command.name == "quit" {
                format!("Quit {app_name}")
            } else {
                command.description.to_string()
            },
        })
        .collect()
}

pub fn builtin_slash_command(name: &str) -> Option<&'static BuiltinSlashCommand> {
    BUILTIN_SLASH_COMMANDS
        .iter()
        .find(|command| command.name == name.trim_start_matches('/'))
}

pub fn path_command_argument(text: &str, command: &str) -> Option<String> {
    if text == command {
        return None;
    }
    let prefix = format!("{command} ");
    if !text.starts_with(&prefix) {
        return None;
    }

    let argument = text[command.len() + 1..].trim_start();
    if argument.is_empty() {
        return None;
    }
    let first = argument.chars().next()?;
    if first == '"' || first == '\'' {
        let closing = argument[first.len_utf8()..].find(first)?;
        let end = first.len_utf8() + closing;
        return Some(argument[first.len_utf8()..end].to_string());
    }

    let end = argument.find(char::is_whitespace).unwrap_or(argument.len());
    Some(argument[..end].to_string())
}

pub fn prompt_template_commands(templates: &[PromptTemplate]) -> Vec<SlashCommandInfo> {
    templates
        .iter()
        .map(|template| SlashCommandInfo {
            name: template.name.clone(),
            description: template.description.clone(),
            argument_hint: template.argument_hint.clone(),
            source: SlashCommandSource::Prompt,
            source_info: template
                .source_info
                .clone()
                .unwrap_or_else(|| json!({ "path": template.file_path })),
        })
        .collect()
}

pub fn skill_commands(skills: &[Skill]) -> Vec<SlashCommandInfo> {
    skills
        .iter()
        .map(|skill| SlashCommandInfo {
            name: format!("skill:{}", skill.name),
            description: Some(skill.description.clone()),
            argument_hint: None,
            source: SlashCommandSource::Skill,
            source_info: skill
                .source_info
                .clone()
                .unwrap_or_else(|| json!({ "path": skill.file_path })),
        })
        .collect()
}

pub fn resource_slash_commands(
    templates: &[PromptTemplate],
    skills: &[Skill],
) -> Vec<SlashCommandInfo> {
    let mut commands = prompt_template_commands(templates);
    commands.extend(skill_commands(skills));
    commands
}

pub fn extension_commands(commands: &[ResolvedCommand]) -> Vec<SlashCommandInfo> {
    commands
        .iter()
        .map(|command| SlashCommandInfo {
            name: command.invocation_name.clone(),
            description: command.command.description.clone(),
            argument_hint: None,
            source: SlashCommandSource::Extension,
            source_info: serde_json::to_value(&command.command.source_info)
                .unwrap_or_else(|_| json!({ "path": command.command.source_info.path })),
        })
        .collect()
}

pub fn compose_slash_commands(
    extension_commands: Vec<SlashCommandInfo>,
    resource_commands: Vec<SlashCommandInfo>,
) -> Vec<SlashCommandInfo> {
    let mut commands = extension_commands;
    commands.extend(resource_commands);
    commands
}

pub fn slash_command_to_rpc(command: &SlashCommandInfo) -> RpcSlashCommand {
    RpcSlashCommand {
        name: command.name.clone(),
        description: command.description.clone(),
        argument_hint: command.argument_hint.clone(),
        source: match command.source {
            SlashCommandSource::Extension => RpcSlashCommandSource::Extension,
            SlashCommandSource::Prompt => RpcSlashCommandSource::Prompt,
            SlashCommandSource::Skill => RpcSlashCommandSource::Skill,
        },
        source_info: command.source_info.clone(),
    }
}

pub fn slash_commands_to_rpc(commands: &[SlashCommandInfo]) -> Vec<RpcSlashCommand> {
    commands.iter().map(slash_command_to_rpc).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_builtin_slash_commands() {
        assert!(builtin_slash_command("/model").is_some());
        assert!(BUILTIN_SLASH_COMMANDS
            .iter()
            .any(|command| command.name == "compact"));
    }

    #[test]
    fn extracts_path_command_arguments_like_pi_interactive_mode() {
        assert_eq!(
            path_command_argument(r#"/import "path/to/session.jsonl""#, "/import"),
            Some("path/to/session.jsonl".to_string())
        );
        assert_eq!(
            path_command_argument(r#"/import "path with spaces/session.jsonl""#, "/import"),
            Some("path with spaces/session.jsonl".to_string())
        );
        assert_eq!(
            path_command_argument("/import john's/session.jsonl", "/import"),
            Some("john's/session.jsonl".to_string())
        );
        assert_eq!(
            path_command_argument("/import /tmp/session.jsonl", "/import"),
            Some("/tmp/session.jsonl".to_string())
        );
        assert_eq!(
            path_command_argument(
                r#"/import "path with spaces/session.jsonl" trailing"#,
                "/import"
            ),
            Some("path with spaces/session.jsonl".to_string())
        );
        assert_eq!(
            path_command_argument("/import alpha beta", "/import"),
            Some("alpha".to_string())
        );
        assert_eq!(
            path_command_argument(r#"/import "unterminated"#, "/import"),
            None
        );
        assert_eq!(
            path_command_argument("/important /tmp/session.jsonl", "/import"),
            None
        );
        assert_eq!(path_command_argument("/exporter out.html", "/export"), None);
    }

    #[test]
    fn builds_builtin_quit_description_from_app_name_like_pi() {
        let commands = builtin_slash_commands("π");
        let quit = commands
            .iter()
            .find(|command| command.name == "quit")
            .expect("quit command should exist");

        assert_eq!(quit.description, "Quit π");
    }

    #[test]
    fn builds_resource_slash_commands_like_pi() {
        let templates = vec![PromptTemplate {
            name: "review".to_string(),
            description: Some("Review code".to_string()),
            argument_hint: Some("<file>".to_string()),
            content: String::new(),
            file_path: "/prompts/review.md".to_string(),
            source_info: None,
        }];
        let skills = vec![Skill {
            name: "rust".to_string(),
            description: "Rust help".to_string(),
            content: String::new(),
            file_path: "/skills/rust/SKILL.md".to_string(),
            source_info: None,
            disable_model_invocation: false,
        }];

        let commands = resource_slash_commands(&templates, &skills);

        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "review");
        assert_eq!(commands[0].source, SlashCommandSource::Prompt);
        assert_eq!(commands[0].argument_hint.as_deref(), Some("<file>"));
        assert_eq!(commands[1].name, "skill:rust");
        assert_eq!(commands[1].source, SlashCommandSource::Skill);
    }

    #[test]
    fn converts_resource_commands_to_rpc_shape() {
        let command = SlashCommandInfo {
            name: "skill:rust".to_string(),
            description: Some("Rust help".to_string()),
            argument_hint: None,
            source: SlashCommandSource::Skill,
            source_info: json!({ "path": "/skills/rust/SKILL.md" }),
        };

        let rpc = slash_command_to_rpc(&command);

        assert_eq!(rpc.name, "skill:rust");
        assert_eq!(rpc.source, RpcSlashCommandSource::Skill);
        assert_eq!(rpc.argument_hint, None);
        assert_eq!(rpc.source_info["path"], "/skills/rust/SKILL.md");
    }

    #[test]
    fn converts_extension_commands_to_slash_commands() {
        use crate::extensions::RegisteredCommand;
        use crate::source_info::create_synthetic_source_info;
        use std::sync::Arc;

        let registered = ResolvedCommand {
            invocation_name: "demo".to_string(),
            command: RegisteredCommand {
                name: "demo".to_string(),
                description: Some("Demo command".to_string()),
                handler: Arc::new(|_| Ok(())),
                source_info: create_synthetic_source_info(
                    "/extensions/demo.ts",
                    "local",
                    None,
                    None,
                    None,
                ),
            },
        };

        let commands = extension_commands(&[registered]);

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "demo");
        assert_eq!(commands[0].source, SlashCommandSource::Extension);
        assert_eq!(commands[0].source_info["path"], "/extensions/demo.ts");
    }

    #[test]
    fn composes_commands_in_pi_order() {
        let extension = SlashCommandInfo {
            name: "ext".to_string(),
            description: None,
            argument_hint: None,
            source: SlashCommandSource::Extension,
            source_info: json!({ "path": "/extensions/ext.ts" }),
        };
        let prompt = SlashCommandInfo {
            name: "prompt".to_string(),
            description: None,
            argument_hint: Some("<topic>".to_string()),
            source: SlashCommandSource::Prompt,
            source_info: json!({ "path": "/prompts/prompt.md" }),
        };
        let skill = SlashCommandInfo {
            name: "skill:demo".to_string(),
            description: None,
            argument_hint: None,
            source: SlashCommandSource::Skill,
            source_info: json!({ "path": "/skills/demo/SKILL.md" }),
        };

        let commands = compose_slash_commands(vec![extension], vec![prompt, skill]);

        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["ext", "prompt", "skill:demo"]
        );
    }
}
