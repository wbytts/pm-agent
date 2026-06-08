use agent::AgentMessage;
use serde_json::{json, Value};

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

pub fn emit_message_end(
    extensions: &[Extension],
    message: AgentMessage,
    mut report_error: impl FnMut(ExtensionError),
) -> Option<AgentMessage> {
    let mut current_message = message;
    let mut modified = false;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("message_end") else {
            continue;
        };
        for handler in handlers {
            let event = ExtensionEvent {
                kind: ExtensionEventKind::MessageEnd,
                payload: json!({
                    "type": "message_end",
                    "message": current_message,
                }),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event)));
            match result {
                Ok(Some(value)) => {
                    let Some(next_message) = parse_message_result(&value) else {
                        continue;
                    };
                    if next_message.role != current_message.role {
                        report_error(ExtensionError {
                            extension_path: extension.path.clone(),
                            event: Some("message_end".to_string()),
                            message:
                                "message_end handlers must return a message with the same role"
                                    .to_string(),
                        });
                        continue;
                    }
                    current_message = next_message;
                    modified = true;
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("message_end".to_string()),
                    message: "Extension message_end handler panicked".to_string(),
                }),
            }
        }
    }

    modified.then_some(current_message)
}

fn parse_message_result(value: &Value) -> Option<AgentMessage> {
    let message = value.get("message")?;
    serde_json::from_value(message.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use ai::MessageRole;
    use std::sync::Arc;

    #[test]
    fn returns_modified_message_when_role_matches() {
        let mut extension = Extension::new(
            "/extensions/message.ts",
            create_synthetic_source_info("/extensions/message.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "message_end".to_string(),
            vec![Arc::new(|event| {
                let mut message = event.payload["message"].clone();
                message["content"] = json!("changed");
                Some(json!({ "message": message }))
            })],
        );

        let message = emit_message_end(
            &[extension],
            AgentMessage::new(MessageRole::Assistant, "original".to_string()),
            |_| {},
        )
        .expect("message should be modified");

        assert_eq!(message.content, "changed");
    }

    #[test]
    fn rejects_role_mismatch() {
        let mut extension = Extension::new(
            "/extensions/bad-message.ts",
            create_synthetic_source_info("/extensions/bad-message.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "message_end".to_string(),
            vec![Arc::new(|_| {
                Some(json!({
                    "message": {
                        "role": "User",
                        "content": "bad"
                    }
                }))
            })],
        );
        let mut errors = Vec::new();

        let result = emit_message_end(
            &[extension],
            AgentMessage::new(MessageRole::Assistant, "original".to_string()),
            |error| errors.push(error.message),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec!["message_end handlers must return a message with the same role"]
        );
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-message.ts",
            create_synthetic_source_info("/extensions/panic-message.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "message_end".to_string(),
            vec![Arc::new(|_| panic!("bad message handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_message_end(
            &[extension],
            AgentMessage::new(MessageRole::Assistant, "original".to_string()),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-message.ts".to_string(),
                Some("message_end".to_string())
            )]
        );
    }
}
