use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Value>,
    pub system_prompt: String,
    pub system_prompt_options: Value,
}

impl BeforeAgentStartEvent {
    pub fn new(
        prompt: impl Into<String>,
        images: Option<Value>,
        system_prompt: impl Into<String>,
        system_prompt_options: Value,
    ) -> Self {
        Self {
            event_type: "before_agent_start".to_string(),
            prompt: prompt.into(),
            images,
            system_prompt: system_prompt.into(),
            system_prompt_options,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

pub fn emit_before_agent_start(
    extensions: &[Extension],
    event: BeforeAgentStartEvent,
    mut report_error: impl FnMut(ExtensionError),
) -> Option<BeforeAgentStartResult> {
    let mut current_system_prompt = event.system_prompt;
    let mut messages = Vec::new();
    let mut system_prompt_modified = false;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("before_agent_start") else {
            continue;
        };
        for handler in handlers {
            let extension_event = ExtensionEvent {
                kind: ExtensionEventKind::BeforeAgentStart,
                payload: json!({
                    "type": "before_agent_start",
                    "prompt": event.prompt,
                    "images": event.images,
                    "systemPrompt": current_system_prompt,
                    "systemPromptOptions": event.system_prompt_options,
                }),
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(extension_event)));
            match result {
                Ok(Some(value)) => {
                    if let Some(message) = value.get("message").cloned() {
                        messages.push(message);
                    }
                    if let Some(system_prompt) = value.get("systemPrompt").and_then(Value::as_str) {
                        current_system_prompt = system_prompt.to_string();
                        system_prompt_modified = true;
                    }
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("before_agent_start".to_string()),
                    message: "Extension before_agent_start handler panicked".to_string(),
                }),
            }
        }
    }

    if messages.is_empty() && !system_prompt_modified {
        return None;
    }

    Some(BeforeAgentStartResult {
        messages: (!messages.is_empty()).then_some(messages),
        system_prompt: system_prompt_modified.then_some(current_system_prompt),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn collects_messages_and_chains_system_prompt() {
        let mut extension = Extension::new(
            "/extensions/before-agent-start.ts",
            create_synthetic_source_info(
                "/extensions/before-agent-start.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "before_agent_start".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["systemPrompt"], "original system");
                    Some(json!({
                        "message": { "customType": "notice", "content": "first" },
                        "systemPrompt": "first system"
                    }))
                }),
                Arc::new(|event| {
                    assert_eq!(event.payload["systemPrompt"], "first system");
                    Some(json!({
                        "message": { "customType": "notice", "content": "second" },
                        "systemPrompt": "second system"
                    }))
                }),
            ],
        );

        let result = emit_before_agent_start(
            &[extension],
            BeforeAgentStartEvent::new(
                "hello",
                None,
                "original system",
                json!({ "cwd": "/tmp/project" }),
            ),
            |_| {},
        )
        .expect("before agent start should be modified");

        assert_eq!(result.messages.expect("messages")[1]["content"], "second");
        assert_eq!(result.system_prompt.as_deref(), Some("second system"));
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-before-agent-start.ts",
            create_synthetic_source_info(
                "/extensions/panic-before-agent-start.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "before_agent_start".to_string(),
            vec![Arc::new(|_| panic!("bad before agent start handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_before_agent_start(
            &[extension],
            BeforeAgentStartEvent::new("hello", None, "system", json!({})),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-before-agent-start.ts".to_string(),
                Some("before_agent_start".to_string())
            )]
        );
    }
}
