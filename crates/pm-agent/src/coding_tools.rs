use coding_agent::{execute_tool, CodingToolRequest, CodingToolResult, CodingWorkspace};
use std::path::PathBuf;

pub fn execute_coding_tool(
    cwd: impl Into<PathBuf>,
    request: CodingToolRequest,
) -> Result<CodingToolResult, String> {
    let workspace = CodingWorkspace { cwd: cwd.into() };
    execute_tool(&workspace, request).map_err(|error| error.to_string())
}
