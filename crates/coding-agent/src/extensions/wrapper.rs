use serde_json::Value;

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
) -> Vec<WrappedTool> {
    registered_tools
        .iter()
        .map(|tool| wrap_registered_tool(tool, runner))
        .collect()
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
