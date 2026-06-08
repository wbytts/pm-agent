use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolCallEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
}

impl ExtensionToolCallEvent {
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            event_type: "tool_call".to_string(),
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDecision {
    pub block: bool,
    pub reason: Option<String>,
}

pub fn emit_tool_call(
    extensions: &[Extension],
    event: ExtensionToolCallEvent,
    mut report_error: impl FnMut(ExtensionError),
) -> Option<ToolCallDecision> {
    let mut result = None;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("tool_call") else {
            continue;
        };
        for handler in handlers {
            let extension_event = ExtensionEvent {
                kind: ExtensionEventKind::ToolCall,
                payload: serde_json::to_value(&event).unwrap_or(Value::Null),
            };
            let handler_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(extension_event)));
            match handler_result {
                Ok(Some(value)) => {
                    let Some(decision) = parse_tool_call_decision(&value) else {
                        continue;
                    };
                    let should_block = decision.block;
                    result = Some(decision);
                    if should_block {
                        return result;
                    }
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("tool_call".to_string()),
                    message: "Extension tool_call handler panicked".to_string(),
                }),
            }
        }
    }

    result
}

fn parse_tool_call_decision(value: &Value) -> Option<ToolCallDecision> {
    let block = value.get("block").and_then(Value::as_bool).unwrap_or(false);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    if value.get("block").is_none() && reason.is_none() {
        return None;
    }

    Some(ToolCallDecision { block, reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn returns_last_non_blocking_decision() {
        let mut extension = Extension::new(
            "/extensions/tool-call.ts",
            create_synthetic_source_info("/extensions/tool-call.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "tool_call".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["type"], "tool_call");
                    assert_eq!(event.payload["toolName"], "bash");
                    Some(json!({ "reason": "first" }))
                }),
                Arc::new(|_| Some(json!({ "reason": "second" }))),
            ],
        );

        let decision = emit_tool_call(
            &[extension],
            ExtensionToolCallEvent::new("call-1", "bash", json!({ "command": "pwd" })),
            |_| {},
        )
        .expect("decision should be returned");

        assert!(!decision.block);
        assert_eq!(decision.reason.as_deref(), Some("second"));
    }

    #[test]
    fn stops_on_blocking_decision() {
        let mut extension = Extension::new(
            "/extensions/tool-block.ts",
            create_synthetic_source_info("/extensions/tool-block.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "tool_call".to_string(),
            vec![
                Arc::new(|_| Some(json!({ "block": true, "reason": "blocked" }))),
                Arc::new(|_| Some(json!({ "reason": "should not run" }))),
            ],
        );

        let decision = emit_tool_call(
            &[extension],
            ExtensionToolCallEvent::new("call-1", "write", json!({ "path": "README.md" })),
            |_| {},
        )
        .expect("decision should be returned");

        assert!(decision.block);
        assert_eq!(decision.reason.as_deref(), Some("blocked"));
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-tool-call.ts",
            create_synthetic_source_info(
                "/extensions/panic-tool-call.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "tool_call".to_string(),
            vec![Arc::new(|_| panic!("bad tool call handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_tool_call(
            &[extension],
            ExtensionToolCallEvent::new("call-1", "grep", json!({ "pattern": "todo" })),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-tool-call.ts".to_string(),
                Some("tool_call".to_string())
            )]
        );
    }
}
