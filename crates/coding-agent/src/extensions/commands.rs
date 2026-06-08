use std::collections::{BTreeMap, BTreeSet};

use super::types::{Extension, RegisteredCommand};

#[derive(Clone)]
pub struct ResolvedCommand {
    pub invocation_name: String,
    pub command: RegisteredCommand,
}

pub fn resolve_registered_commands(extensions: &[Extension]) -> Vec<ResolvedCommand> {
    let mut commands = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();

    for extension in extensions {
        for command in extension.commands.values() {
            *counts.entry(command.name.clone()).or_default() += 1;
            commands.push(command.clone());
        }
    }

    let mut seen = BTreeMap::<String, usize>::new();
    let mut taken_invocation_names = BTreeSet::<String>::new();

    commands
        .into_iter()
        .map(|command| {
            let occurrence = seen.entry(command.name.clone()).or_default();
            *occurrence += 1;

            let mut invocation_name = if counts.get(&command.name).copied().unwrap_or_default() > 1
            {
                format!("{}:{}", command.name, occurrence)
            } else {
                command.name.clone()
            };

            if taken_invocation_names.contains(&invocation_name) {
                let mut suffix = *occurrence;
                loop {
                    suffix += 1;
                    invocation_name = format!("{}:{suffix}", command.name);
                    if !taken_invocation_names.contains(&invocation_name) {
                        break;
                    }
                }
            }

            taken_invocation_names.insert(invocation_name.clone());
            ResolvedCommand {
                invocation_name,
                command,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use std::sync::Arc;

    #[test]
    fn resolves_duplicate_command_invocation_names_like_pi() {
        let mut first = Extension::new(
            "/extensions/one.ts",
            create_synthetic_source_info("/extensions/one.ts", "local", None, None, None),
        );
        let mut second = Extension::new(
            "/extensions/two.ts",
            create_synthetic_source_info("/extensions/two.ts", "local", None, None, None),
        );
        first.commands.insert(
            "demo".to_string(),
            RegisteredCommand {
                name: "demo".to_string(),
                description: None,
                handler: Arc::new(|_| Ok(())),
                source_info: first.source_info.clone(),
            },
        );
        second.commands.insert(
            "demo".to_string(),
            RegisteredCommand {
                name: "demo".to_string(),
                description: None,
                handler: Arc::new(|_| Ok(())),
                source_info: second.source_info.clone(),
            },
        );

        let commands = resolve_registered_commands(&[first, second]);

        assert_eq!(
            commands
                .iter()
                .map(|command| command.invocation_name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo:1", "demo:2"]
        );
    }
}
