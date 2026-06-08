use serde_json::Value;

use crate::CodingToolResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionOptions {
    pub show_images: bool,
    pub image_width_cells: usize,
}

impl Default for ToolExecutionOptions {
    fn default() -> Self {
        Self {
            show_images: true,
            image_width_cells: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolExecutionContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionResult {
    pub content: Vec<ToolExecutionContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionBackground {
    Pending,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionState {
    tool_name: String,
    tool_call_id: String,
    args: Value,
    expanded: bool,
    show_images: bool,
    image_width_cells: usize,
    is_partial: bool,
    execution_started: bool,
    args_complete: bool,
    result: Option<ToolExecutionResult>,
}

impl ToolExecutionState {
    pub fn new(
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        args: Value,
        options: ToolExecutionOptions,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_call_id: tool_call_id.into(),
            args,
            expanded: false,
            show_images: options.show_images,
            image_width_cells: options.image_width_cells.max(1),
            is_partial: true,
            execution_started: false,
            args_complete: false,
            result: None,
        }
    }

    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn show_images(&self) -> bool {
        self.show_images
    }

    pub fn image_width_cells(&self) -> usize {
        self.image_width_cells
    }

    pub fn execution_started(&self) -> bool {
        self.execution_started
    }

    pub fn args_complete(&self) -> bool {
        self.args_complete
    }

    pub fn update_args(&mut self, args: Value) {
        self.args = args;
    }

    pub fn mark_execution_started(&mut self) {
        self.execution_started = true;
    }

    pub fn set_args_complete(&mut self) {
        self.args_complete = true;
    }

    pub fn update_result(&mut self, result: ToolExecutionResult, is_partial: bool) {
        self.result = Some(result);
        self.is_partial = is_partial;
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn set_show_images(&mut self, show_images: bool) {
        self.show_images = show_images;
    }

    pub fn set_image_width_cells(&mut self, width: usize) {
        self.image_width_cells = width.max(1);
    }

    pub fn background(&self) -> ToolExecutionBackground {
        if self.is_partial {
            return ToolExecutionBackground::Pending;
        }

        match self.result.as_ref().map(|result| result.is_error) {
            Some(true) => ToolExecutionBackground::Error,
            _ => ToolExecutionBackground::Success,
        }
    }

    pub fn text_output(&self) -> String {
        let Some(result) = &self.result else {
            return String::new();
        };

        result
            .content
            .iter()
            .filter_map(|block| match block {
                ToolExecutionContentBlock::Text { text } => Some(text.as_str().to_string()),
                ToolExecutionContentBlock::Image { mime_type, .. } if !self.show_images => {
                    Some(format!("[Image: {mime_type}]"))
                }
                ToolExecutionContentBlock::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn format_tool_execution(&self) -> String {
        let mut text = self.tool_name.clone();
        if let Ok(content) = serde_json::to_string_pretty(&self.args) {
            if !content.is_empty() {
                text.push_str("\n\n");
                text.push_str(&content);
            }
        }

        let output = self.text_output();
        if !output.is_empty() {
            text.push('\n');
            text.push_str(&output);
        }

        text
    }

    pub fn render_lines(&self) -> Vec<String> {
        self.format_tool_execution()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

impl From<CodingToolResult> for ToolExecutionResult {
    fn from(result: CodingToolResult) -> Self {
        let content = result
            .content
            .map(|blocks| {
                blocks
                    .into_iter()
                    .map(|block| match block {
                        crate::CodingContentBlock::Text { text } => {
                            ToolExecutionContentBlock::Text { text }
                        }
                        crate::CodingContentBlock::Image { data, mime_type } => {
                            ToolExecutionContentBlock::Image { data, mime_type }
                        }
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![ToolExecutionContentBlock::Text {
                    text: result.output,
                }]
            });

        Self {
            content,
            details: result.details,
            is_error: !result.success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_execution_default_renderer_formats_title_args_and_text_result_like_pi() {
        let mut execution = ToolExecutionState::new(
            "read",
            "call_1",
            json!({
                "path": "src/main.rs",
                "limit": 20
            }),
            ToolExecutionOptions::default(),
        );

        execution.update_result(
            ToolExecutionResult {
                content: vec![ToolExecutionContentBlock::Text {
                    text: "file contents".to_string(),
                }],
                details: None,
                is_error: false,
            },
            false,
        );

        assert_eq!(execution.background(), ToolExecutionBackground::Success);
        assert_eq!(
            execution.render_lines(),
            vec![
                "read".to_string(),
                "".to_string(),
                "{".to_string(),
                "  \"limit\": 20,".to_string(),
                "  \"path\": \"src/main.rs\"".to_string(),
                "}".to_string(),
                "file contents".to_string(),
            ]
        );
    }

    #[test]
    fn tool_execution_text_output_hides_images_when_requested() {
        let mut execution = ToolExecutionState::new(
            "screenshot",
            "call_2",
            json!({}),
            ToolExecutionOptions {
                show_images: false,
                image_width_cells: 60,
            },
        );

        execution.update_result(
            ToolExecutionResult {
                content: vec![
                    ToolExecutionContentBlock::Text {
                        text: "before".to_string(),
                    },
                    ToolExecutionContentBlock::Image {
                        data: "base64".to_string(),
                        mime_type: "image/png".to_string(),
                    },
                ],
                details: None,
                is_error: true,
            },
            false,
        );

        assert_eq!(execution.background(), ToolExecutionBackground::Error);
        assert_eq!(execution.text_output(), "before\n[Image: image/png]");
    }

    #[test]
    fn tool_execution_partial_result_keeps_pending_background() {
        let mut execution = ToolExecutionState::new(
            "bash",
            "call_3",
            json!({ "command": "pwd" }),
            Default::default(),
        );

        execution.mark_execution_started();
        execution.set_args_complete();
        execution.update_result(
            ToolExecutionResult {
                content: vec![ToolExecutionContentBlock::Text {
                    text: "/tmp".to_string(),
                }],
                details: None,
                is_error: false,
            },
            true,
        );

        assert!(execution.execution_started());
        assert!(execution.args_complete());
        assert_eq!(execution.background(), ToolExecutionBackground::Pending);
    }

    #[test]
    fn tool_execution_result_adapts_current_coding_tool_result() {
        let result = crate::CodingToolResult {
            success: false,
            output: "fallback output".to_string(),
            details: Some(json!({ "exitCode": 1 })),
            content: None,
        };

        let execution_result = ToolExecutionResult::from(result);

        assert_eq!(execution_result.is_error, true);
        assert_eq!(
            execution_result.content,
            vec![ToolExecutionContentBlock::Text {
                text: "fallback output".to_string()
            }]
        );
        assert_eq!(execution_result.details, Some(json!({ "exitCode": 1 })));
    }
}
