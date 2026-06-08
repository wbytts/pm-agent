use serde_json::Value;
use std::collections::BTreeSet;

use super::runner::ExtensionRunner;
use super::types::{ExtensionContext, RegisteredTool, ToolDefinition};

#[derive(Clone)]
pub struct WrappedTool {
    pub definition: ToolDefinition,
}

pub fn wrap_registered_tool(
    registered_tool: &RegisteredTool,
    _runner: &ExtensionRunner,
) -> WrappedTool {
    WrappedTool {
        definition: registered_tool.definition.definition.clone(),
    }
}

pub fn wrap_registered_tools(
    registered_tools: &[RegisteredTool],
    runner: &ExtensionRunner,
    allowed_tool_names: Option<&[String]>,
) -> Vec<WrappedTool> {
    let allowed_tool_names = allowed_tool_names.map(|names| names.iter().collect::<BTreeSet<_>>());
    registered_tools
        .iter()
        .filter(|tool| {
            allowed_tool_names
                .as_ref()
                .is_none_or(|names| names.contains(&tool.definition.definition.name))
        })
        .map(|tool| wrap_registered_tool(tool, runner))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::types::{ExecutableToolDefinition, Extension, ExtensionRuntime};
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn wraps_only_allowed_extension_tools_like_pi_tools_allowlist() {
        let extension = Extension::new(
            "/extensions/demo.ts",
            create_synthetic_source_info("/extensions/demo.ts", "local", None, None, None),
        );
        let registered_tools = vec![
            registered_tool("dynamic_tool", &extension),
            registered_tool("hidden_tool", &extension),
        ];
        let runner = ExtensionRunner::new(Vec::new(), ExtensionRuntime::default());

        let wrapped = wrap_registered_tools(
            &registered_tools,
            &runner,
            Some(&["read".to_string(), "dynamic_tool".to_string()]),
        );

        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].definition.name, "dynamic_tool");
    }

    #[test]
    fn wraps_all_extension_tools_without_allowlist_like_pi_no_builtin_tools() {
        let extension = Extension::new(
            "/extensions/demo.ts",
            create_synthetic_source_info("/extensions/demo.ts", "local", None, None, None),
        );
        let registered_tools = vec![
            registered_tool("dynamic_tool", &extension),
            registered_tool("other_tool", &extension),
        ];
        let runner = ExtensionRunner::new(Vec::new(), ExtensionRuntime::default());

        let wrapped = wrap_registered_tools(&registered_tools, &runner, None);

        assert_eq!(
            wrapped
                .iter()
                .map(|tool| tool.definition.name.as_str())
                .collect::<Vec<_>>(),
            vec!["dynamic_tool", "other_tool"]
        );
    }

    fn registered_tool(name: &str, extension: &Extension) -> RegisteredTool {
        RegisteredTool {
            definition: ExecutableToolDefinition {
                definition: ToolDefinition {
                    name: name.to_string(),
                    label: None,
                    description: format!("{name} description"),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute: Arc::new(|input, _ctx| Ok(input)),
            },
            source_info: extension.source_info.clone(),
        }
    }
}

pub fn execute_registered_tool(
    registered_tool: &RegisteredTool,
    _runner: &ExtensionRunner,
    input: Value,
) -> Result<Value, String> {
    let ctx = ExtensionContext {
        extension_path: registered_tool.source_info.path.clone(),
        source_info: registered_tool.source_info.clone(),
    };
    (registered_tool.definition.execute)(input, ctx)
}
