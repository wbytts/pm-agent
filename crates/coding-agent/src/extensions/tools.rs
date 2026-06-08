use std::collections::BTreeSet;

use super::types::{Extension, RegisteredTool, ToolDefinition};

pub fn resolve_registered_tools(extensions: &[Extension]) -> Vec<RegisteredTool> {
    let mut seen = BTreeSet::<String>::new();
    let mut tools = Vec::new();

    for extension in extensions {
        for tool in extension.tools.values() {
            let name = tool.definition.definition.name.clone();
            if seen.insert(name) {
                tools.push(tool.clone());
            }
        }
    }

    tools
}

pub fn find_tool_definition(extensions: &[Extension], tool_name: &str) -> Option<ToolDefinition> {
    for extension in extensions {
        if let Some(tool) = extension.tools.get(tool_name) {
            return Some(tool.definition.definition.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::types::{ExecutableToolDefinition, ToolDefinition};
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn resolves_tools_with_first_registration_wins_like_pi() {
        let mut first = Extension::new(
            "/extensions/one.ts",
            create_synthetic_source_info("/extensions/one.ts", "local", None, None, None),
        );
        let mut second = Extension::new(
            "/extensions/two.ts",
            create_synthetic_source_info("/extensions/two.ts", "local", None, None, None),
        );
        first.tools.insert(
            "demo_tool".to_string(),
            registered_tool("demo_tool", "first", &first),
        );
        second.tools.insert(
            "demo_tool".to_string(),
            registered_tool("demo_tool", "second", &second),
        );

        let tools = resolve_registered_tools(&[first, second]);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].definition.definition.description, "first");
    }

    fn registered_tool(name: &str, description: &str, extension: &Extension) -> RegisteredTool {
        RegisteredTool {
            definition: ExecutableToolDefinition {
                definition: ToolDefinition {
                    name: name.to_string(),
                    label: None,
                    description: description.to_string(),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute: Arc::new(|input, _ctx| Ok(input)),
            },
            source_info: extension.source_info.clone(),
        }
    }
}
