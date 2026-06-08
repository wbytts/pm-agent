use agent::AgentMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextEventResult {
    pub messages: Vec<AgentMessage>,
}

pub fn emit_context(
    extensions: &[Extension],
    messages: Vec<AgentMessage>,
    mut report_error: impl FnMut(ExtensionError),
) -> Vec<AgentMessage> {
    let mut current_messages = messages;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("context") else {
            continue;
        };
        for handler in handlers {
            let event = ExtensionEvent {
                kind: ExtensionEventKind::Context,
                payload: json!({
                    "type": "context",
                    "messages": current_messages,
                }),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event)));
            match result {
                Ok(Some(value)) => {
                    if let Some(messages) = parse_context_messages(&value) {
                        current_messages = messages;
                    }
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("context".to_string()),
                    message: "Extension context handler panicked".to_string(),
                }),
            }
        }
    }

    current_messages
}

fn parse_context_messages(value: &Value) -> Option<Vec<AgentMessage>> {
    let messages = value.get("messages")?;
    serde_json::from_value(messages.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use ai::MessageRole;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn chains_context_messages() {
        let mut extension = Extension::new(
            "/extensions/context.ts",
            create_synthetic_source_info("/extensions/context.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "context".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["type"], "context");
                    Some(json!({
                        "messages": [{ "role": "User", "content": "first" }]
                    }))
                }),
                Arc::new(|event| {
                    assert_eq!(event.payload["messages"][0]["content"], "first");
                    Some(json!({
                        "messages": [{ "role": "User", "content": "second" }]
                    }))
                }),
            ],
        );

        let messages = emit_context(
            &[extension],
            vec![AgentMessage::new(MessageRole::User, "original".to_string())],
            |_| {},
        );

        assert_eq!(messages[0].content, "second");
    }

    #[test]
    fn reports_panicking_handler_and_keeps_messages() {
        let mut extension = Extension::new(
            "/extensions/panic-context.ts",
            create_synthetic_source_info("/extensions/panic-context.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "context".to_string(),
            vec![Arc::new(|_| panic!("bad context handler"))],
        );
        let mut errors = Vec::new();

        let messages = emit_context(
            &[extension],
            vec![AgentMessage::new(MessageRole::User, "original".to_string())],
            |error| errors.push((error.extension_path, error.event)),
        );

        assert_eq!(messages[0].content, "original");
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-context.ts".to_string(),
                Some("context".to_string())
            )]
        );
    }
}
