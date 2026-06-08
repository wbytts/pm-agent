use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};
use crate::types::CodingToolResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolResultEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub content: Value,
    pub details: Option<Value>,
    pub is_error: bool,
}

impl ExtensionToolResultEvent {
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
        content: Value,
        details: Option<Value>,
        is_error: bool,
    ) -> Self {
        Self {
            event_type: "tool_result".to_string(),
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            content,
            details,
            is_error,
        }
    }

    pub fn from_coding_tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
        result: CodingToolResult,
    ) -> Self {
        let content = result
            .content
            .as_ref()
            .and_then(|content| serde_json::to_value(content).ok())
            .unwrap_or_else(|| json!([{ "type": "text", "text": result.output }]));
        Self::new(
            tool_call_id,
            tool_name,
            input,
            content,
            result.details,
            !result.success,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultUpdate {
    pub content: Value,
    pub details: Option<Value>,
    pub is_error: bool,
}

pub fn emit_tool_result(
    extensions: &[Extension],
    event: ExtensionToolResultEvent,
    mut report_error: impl FnMut(ExtensionError),
) -> Option<ToolResultUpdate> {
    let mut current_event = event;
    let mut modified = false;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("tool_result") else {
            continue;
        };
        for handler in handlers {
            let extension_event = ExtensionEvent {
                kind: ExtensionEventKind::ToolResult,
                payload: serde_json::to_value(&current_event).unwrap_or(Value::Null),
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(extension_event)));
            match result {
                Ok(Some(value)) => {
                    let update = parse_tool_result_update(&value);
                    if let Some(content) = update.content {
                        current_event.content = content;
                        modified = true;
                    }
                    if let Some(details) = update.details {
                        current_event.details = Some(details);
                        modified = true;
                    }
                    if let Some(is_error) = update.is_error {
                        current_event.is_error = is_error;
                        modified = true;
                    }
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("tool_result".to_string()),
                    message: "Extension tool_result handler panicked".to_string(),
                }),
            }
        }
    }

    modified.then_some(ToolResultUpdate {
        content: current_event.content,
        details: current_event.details,
        is_error: current_event.is_error,
    })
}

struct PartialToolResultUpdate {
    content: Option<Value>,
    details: Option<Value>,
    is_error: Option<bool>,
}

fn parse_tool_result_update(value: &Value) -> PartialToolResultUpdate {
    PartialToolResultUpdate {
        content: value.get("content").cloned(),
        details: value.get("details").cloned(),
        is_error: value.get("isError").and_then(Value::as_bool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use std::sync::Arc;

    #[test]
    fn returns_modified_content_and_error_flag() {
        let mut extension = Extension::new(
            "/extensions/tool.ts",
            create_synthetic_source_info("/extensions/tool.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "tool_result".to_string(),
            vec![Arc::new(|event| {
                assert_eq!(event.payload["type"], "tool_result");
                assert_eq!(event.payload["toolName"], "bash");
                Some(json!({
                    "content": [{ "type": "text", "text": "rewritten" }],
                    "isError": true
                }))
            })],
        );

        let update = emit_tool_result(
            &[extension],
            ExtensionToolResultEvent::new(
                "call-1",
                "bash",
                json!({ "command": "pwd" }),
                json!([{ "type": "text", "text": "original" }]),
                None,
                false,
            ),
            |_| {},
        )
        .expect("tool result should be modified");

        assert_eq!(update.content[0]["text"], "rewritten");
        assert!(update.is_error);
    }

    #[test]
    fn chains_details_update_between_handlers() {
        let mut extension = Extension::new(
            "/extensions/tool-details.ts",
            create_synthetic_source_info("/extensions/tool-details.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "tool_result".to_string(),
            vec![
                Arc::new(|_| Some(json!({ "details": { "phase": "first" } }))),
                Arc::new(|event| {
                    assert_eq!(event.payload["details"]["phase"], "first");
                    Some(json!({ "details": { "phase": "second" } }))
                }),
            ],
        );

        let update = emit_tool_result(
            &[extension],
            ExtensionToolResultEvent::new(
                "call-1",
                "read",
                json!({ "path": "README.md" }),
                json!([{ "type": "text", "text": "ok" }]),
                None,
                false,
            ),
            |_| {},
        )
        .expect("details should be modified");

        assert_eq!(update.details.expect("details")["phase"], "second");
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-tool.ts",
            create_synthetic_source_info("/extensions/panic-tool.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "tool_result".to_string(),
            vec![Arc::new(|_| panic!("bad tool handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_tool_result(
            &[extension],
            ExtensionToolResultEvent::new(
                "call-1",
                "grep",
                json!({ "pattern": "todo" }),
                json!([{ "type": "text", "text": "ok" }]),
                None,
                false,
            ),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-tool.ts".to_string(),
                Some("tool_result".to_string())
            )]
        );
    }
}
