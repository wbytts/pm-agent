use agent::harness::{parse_command_args, PromptTemplate, Skill};

use crate::extensions::{ExtensionCommandContext, ResolvedCommand};
use crate::prompt_templates::expand_prompt_template;
use crate::skill_commands::{expand_skill_command, SkillCommandExpansion};

#[derive(Default)]
pub struct PromptInputProcessor {
    skills: Vec<Skill>,
    prompt_templates: Vec<PromptTemplate>,
    expand_prompt_templates: bool,
    extension_commands: Vec<ResolvedCommand>,
}

impl PromptInputProcessor {
    pub fn new() -> Self {
        Self {
            expand_prompt_templates: true,
            ..Self::default()
        }
    }

    pub fn set_prompt_resources(
        &mut self,
        skills: Vec<Skill>,
        prompt_templates: Vec<PromptTemplate>,
    ) {
        self.skills = skills;
        self.prompt_templates = prompt_templates;
    }

    pub fn set_expand_prompt_templates(&mut self, enabled: bool) {
        self.expand_prompt_templates = enabled;
    }

    pub fn set_extension_commands(&mut self, commands: Vec<ResolvedCommand>) {
        self.extension_commands = commands;
    }

    pub fn try_execute_extension_command(&self, message: &str) -> Result<bool, String> {
        let Some((command_name, args)) = parse_extension_command_invocation(message) else {
            return Ok(false);
        };
        let Some(command) = self
            .extension_commands
            .iter()
            .find(|command| command.invocation_name == command_name)
        else {
            return Ok(false);
        };

        let args = parse_command_args(args);
        run_registered_extension_command(command, args)?;
        Ok(true)
    }

    pub fn expand_prompt_text(&self, message: &str) -> Result<String, String> {
        if !self.expand_prompt_templates {
            return Ok(message.to_string());
        }

        let expanded = match expand_skill_command(message, &self.skills)? {
            SkillCommandExpansion::Expanded(text)
            | SkillCommandExpansion::NotSkillCommand(text)
            | SkillCommandExpansion::UnknownSkill(text) => text,
        };
        Ok(expand_prompt_template(&expanded, &self.prompt_templates))
    }
}

pub(crate) fn parse_extension_command_invocation(message: &str) -> Option<(&str, &str)> {
    let rest = message.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }
    let split_index = rest.find(char::is_whitespace);
    let command_name = split_index.map_or(rest, |index| &rest[..index]);
    if command_name.is_empty() {
        return None;
    }
    let args = split_index.map_or("", |index| rest[index + 1..].trim());
    Some((command_name, args))
}

fn run_registered_extension_command(
    command: &ResolvedCommand,
    args: Vec<String>,
) -> Result<(), String> {
    let ctx = ExtensionCommandContext {
        extension_path: command.command.source_info.path.clone(),
        command_name: command.command.name.clone(),
        args,
        source_info: command.command.source_info.clone(),
    };
    (command.command.handler)(ctx)
}
